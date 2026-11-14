use anyhow::Context;
use anyhow::bail;
use codex_protocol::models::AgentMessageInputContent;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ContentItemKind;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ImageDetail;
use codex_protocol::models::ImageReference;
use codex_protocol::models::InternalChatMessageMetadataPassthrough;
use codex_protocol::models::ResponseItem;
use codex_protocol::user_input::UserInput;
use codex_utils_image::PromptImageDecoder;
use codex_utils_image::PromptImageMode;
use codex_utils_image::load_data_url_for_prompt;
use codex_utils_image::load_for_prompt_bytes;

use crate::session::TurnInput;

pub(super) const CONTEXT_KIND: &str = "asm.native_context";
pub(super) const SHELL_CONTEXT_KIND: &str = "asm.native_shell_context";

pub(super) fn filter_page_context<'a>(
    input: &mut Vec<TurnInput>,
    policy: codex_config::PageContextPolicy,
    mut history: impl Iterator<Item = &'a ResponseItem>,
) {
    use codex_config::PageContextPolicy;
    if policy == PageContextPolicy::Inherit {
        return;
    }
    if policy == PageContextPolicy::WhenPresent {
        // Native history must receive null snapshots that clear an earlier Page.
        if history.any(|item| page_context_part(item).is_some()) {
            return;
        }
        let snapshots: Vec<_> = input
            .iter()
            .filter_map(|item| match queued_page_context_part(item) {
                Some(PageContextPart::Snapshot(present)) => Some(present),
                Some(PageContextPart::Instructions) | None => None,
            })
            .collect();
        if snapshots.is_empty() || snapshots.iter().any(|present| *present != Some(false)) {
            return;
        }
    }
    input.retain(|item| queued_page_context_part(item).is_none());
}

enum PageContextPart {
    Instructions,
    // Unknown snapshots preserve Page context under when_present.
    Snapshot(Option<bool>),
}

fn queued_page_context_part(item: &TurnInput) -> Option<PageContextPart> {
    let envelope = match item {
        TurnInput::ResponseItem(envelope) => envelope,
        TurnInput::FunctionCallOutput(_)
        | TurnInput::UserInput { .. }
        | TurnInput::InterAgentCommunication(_) => return None,
    };
    page_context_part(&envelope.item)
}

fn page_context_part(item: &ResponseItem) -> Option<PageContextPart> {
    let ResponseItem::Message { content, .. } = item else {
        return None;
    };
    let [ContentItem::InputText { text }] = content.as_slice() else {
        return None;
    };
    let context;
    let text = if let Some(json) =
        text.strip_prefix("ASM supplied context (not the user's message); JSON data follows:\n")
    {
        context = serde_json::from_str::<serde_json::Value>(
            json.strip_suffix("\nEnd of ASM supplied context.")?,
        )
        .ok()?;
        context.get("text")?.as_str()?
    } else {
        text.as_str()
    };
    let text = text.trim();
    let (tag, body) = text.strip_prefix('<')?.split_once('>')?;
    let body = body.strip_suffix(&format!("</{tag}>"))?;
    if tag == "codex_apps_open_page_instructions"
        || tag.starts_with("codex_apps_selected_page_instructions_")
    {
        return Some(PageContextPart::Instructions);
    }
    let body = if tag == "external_codex_apps_open_page" {
        Some(body)
    } else if tag.starts_with("external_codex_apps_selected_page_part_") {
        body.find('{').map(|start| &body[start..])
    } else {
        return None;
    };
    let present = body
        .and_then(|body| serde_json::from_str::<serde_json::Value>(body.trim()).ok())
        .and_then(|snapshot| snapshot.get("page_id").map(|id| !id.is_null()));
    Some(PageContextPart::Snapshot(present))
}

pub(super) fn prepare_input(input: &mut Vec<TurnInput>) -> anyhow::Result<usize> {
    let mut skipped = 0;
    let mut prepared = Vec::with_capacity(input.len());
    for mut item in input.drain(..) {
        match &mut item {
            TurnInput::InterAgentCommunication(message) if message.encrypted_content.is_some() => {
                skipped += 1;
                continue;
            }
            TurnInput::ResponseItem(envelope) | TurnInput::FunctionCallOutput(envelope) => {
                let original = envelope.item.as_ref();
                let mut plaintext = original.clone();
                plaintext.clear_encrypted_content();
                if plaintext != *original || encrypted_parts(original) {
                    skipped += 1;
                    continue;
                }
                envelope.item = context_item(original)?.into();
            }
            TurnInput::UserInput { content, .. } => {
                for part in content {
                    let (encoded, detail) = match part {
                        UserInput::Text { .. } => continue,
                        UserInput::Image { image, detail } => {
                            let ImageReference::Inline { image_url } = image else {
                                bail!("Native images require a base64 data URL, not a file ID");
                            };
                            (
                                load_data_url_for_prompt(
                                    image_url,
                                    image_mode(*detail),
                                    PromptImageDecoder::Legacy,
                                )?,
                                *detail,
                            )
                        }
                        UserInput::LocalImage { path, detail } => {
                            let bytes = std::fs::read(&*path)
                                .with_context(|| format!("read native image {}", path.display()))?;
                            (
                                load_for_prompt_bytes(path, bytes, image_mode(*detail))?,
                                *detail,
                            )
                        }
                        _ => bail!(
                            "native runtimes support text and image attachments; use native text references for other files and skills"
                        ),
                    };
                    *part = UserInput::Image {
                        image: ImageReference::Inline {
                            image_url: encoded.into_data_url(),
                        },
                        detail,
                    };
                }
            }
            TurnInput::InterAgentCommunication(_) => {}
        }
        prepared.push(item);
    }
    *input = prepared;
    Ok(skipped)
}

pub(crate) fn context_item(original: &ResponseItem) -> anyhow::Result<ResponseItem> {
    let kind = match original {
        ResponseItem::Message {
            internal_chat_message_metadata_passthrough: Some(metadata),
            ..
        } if metadata
            .content_item_kinds
            .as_ref()
            .is_some_and(|kinds| kinds.len() == 1 && kinds[0].as_str() == "shell.user_command") =>
        {
            SHELL_CONTEXT_KIND
        }
        _ => CONTEXT_KIND,
    };
    let context = match original {
        ResponseItem::Message { role, content, .. } => {
            let mut texts = Vec::new();
            for part in content {
                match part {
                    ContentItem::InputText { text } | ContentItem::OutputText { text, .. } => {
                        texts.push(text.as_str())
                    }
                    ContentItem::InputImage { .. } | ContentItem::InputAudio { .. } => {
                        bail!("only plaintext injected context is supported by the native runtime")
                    }
                }
            }
            serde_json::json!({"source": "ASM context", "role": role, "text": texts.join("\n")})
        }
        ResponseItem::FunctionCallOutput {
            name,
            namespace,
            output,
            ..
        } => {
            serde_json::json!({"source": "ASM tool output", "name": name, "namespace": namespace, "text": output_text(&output.body)?})
        }
        ResponseItem::CustomToolCallOutput { name, output, .. } => {
            serde_json::json!({"source": "ASM tool output", "name": name, "text": output_text(&output.body)?})
        }
        _ => bail!("unsupported injected item for the native runtime"),
    };
    Ok(ResponseItem::Message {
        encrypted_content: None,
        status: None,
        id: None,
        role: "developer".to_owned(),
        content: vec![ContentItem::InputText {
            text: format!(
                "ASM supplied context (not the user's message); JSON data follows:\n{}\nEnd of ASM supplied context.",
                serde_json::to_string(&context)?
            ),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: Some(InternalChatMessageMetadataPassthrough {
            content_item_kinds: Some(vec![ContentItemKind(kind.to_owned())]),
            ..Default::default()
        }),
    })
}

fn encrypted_parts(item: &ResponseItem) -> bool {
    match item {
        ResponseItem::AgentMessage { content, .. } => content
            .iter()
            .any(|part| matches!(part, AgentMessageInputContent::EncryptedContent { .. })),
        ResponseItem::FunctionCallOutput { output, .. }
        | ResponseItem::CustomToolCallOutput { output, .. } => {
            matches!(&output.body, FunctionCallOutputBody::ContentItems(parts) if parts.iter().any(|part| matches!(part, FunctionCallOutputContentItem::EncryptedContent { .. })))
        }
        _ => false,
    }
}

fn output_text(body: &FunctionCallOutputBody) -> anyhow::Result<String> {
    match body {
        FunctionCallOutputBody::Text(text) => Ok(text.clone()),
        FunctionCallOutputBody::ContentItems(parts) => {
            let mut texts = Vec::new();
            for part in parts {
                match part {
                    FunctionCallOutputContentItem::InputText { text } => texts.push(text.as_str()),
                    FunctionCallOutputContentItem::InputImage { .. }
                    | FunctionCallOutputContentItem::InputAudio { .. }
                    | FunctionCallOutputContentItem::EncryptedContent { .. } => bail!(
                        "only plaintext injected tool outputs are supported by the native runtime"
                    ),
                }
            }
            Ok(texts.join("\n"))
        }
    }
}

fn image_mode(detail: Option<ImageDetail>) -> PromptImageMode {
    match detail {
        Some(ImageDetail::Original) => PromptImageMode::Original,
        Some(ImageDetail::Auto | ImageDetail::Low | ImageDetail::High) | None => {
            PromptImageMode::ResizeToFit
        }
    }
}
