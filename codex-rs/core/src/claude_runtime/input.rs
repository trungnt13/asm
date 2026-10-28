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
use codex_utils_image::PromptImageMode;
use codex_utils_image::load_data_url_for_prompt;
use codex_utils_image::load_for_prompt_bytes;

use crate::session::TurnInput;

pub(super) const CONTEXT_KIND: &str = "asm.native_context";

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
                let original = &envelope.item;
                let mut plaintext = original.clone();
                plaintext.clear_encrypted_content();
                if plaintext != *original || encrypted_parts(original) {
                    skipped += 1;
                    continue;
                }
                let context = match original {
                    ResponseItem::Message { role, content, .. } => {
                        let mut texts = Vec::new();
                        for part in content {
                            match part {
                                ContentItem::InputText { text }
                                | ContentItem::OutputText { text, .. } => texts.push(text.as_str()),
                                ContentItem::InputImage { .. } | ContentItem::InputAudio { .. } => {
                                    bail!(
                                        "only plaintext injected context is supported by the native runtime"
                                    )
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
                // A persisted marker extends native input accounting without replaying old ASM context.
                envelope.item = ResponseItem::Message {
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
                    internal_chat_message_metadata_passthrough: Some(
                        InternalChatMessageMetadataPassthrough {
                            content_item_kinds: Some(vec![ContentItemKind(
                                CONTEXT_KIND.to_owned(),
                            )]),
                            ..Default::default()
                        },
                    ),
                };
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
                                load_data_url_for_prompt(image_url, image_mode(*detail))?,
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
