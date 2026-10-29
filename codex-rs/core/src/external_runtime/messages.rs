use anyhow::Context;
use anyhow::bail;
use codex_protocol::models::AgentMessageInputContent;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ContentItemProvenance;
use codex_protocol::models::ImageReference;
use codex_protocol::models::ResponseItem;
use serde_json::Value;
use serde_json::json;

use super::input;
use crate::session::TurnInput;
use crate::session::session::Session;

pub(super) fn input_messages<'a>(
    items: impl IntoIterator<Item = &'a ResponseItem>,
) -> anyhow::Result<Vec<Value>> {
    let mut messages = Vec::new();
    for item in items {
        match item {
            ResponseItem::Message {
                content,
                internal_chat_message_metadata_passthrough: Some(metadata),
                ..
            } if metadata.content_item_kinds.as_ref().is_some_and(|kinds| {
                kinds.len() == 1 && kinds[0].as_str() == input::CONTEXT_KIND
            }) =>
            {
                for part in content {
                    if let ContentItem::InputText { text } = part {
                        messages.push(Value::String(text.clone()));
                    }
                }
            }
            ResponseItem::Message {
                role,
                content,
                internal_chat_message_metadata_passthrough: Some(metadata),
                ..
            } if role == "user"
                && metadata
                    .content_item_metadata
                    .as_ref()
                    .is_some_and(|parts| {
                        parts.len() == content.len()
                            && parts
                                .iter()
                                .all(|part| part.provenance == ContentItemProvenance::User)
                    }) =>
            {
                if content
                    .iter()
                    .all(|part| matches!(part, ContentItem::InputText { .. }))
                {
                    let texts = content
                        .iter()
                        .filter_map(|part| match part {
                            ContentItem::InputText { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    messages.push(Value::String(texts.join("\n")));
                } else {
                    let mut blocks = Vec::with_capacity(content.len());
                    for part in content {
                        blocks.push(match part {
                            ContentItem::InputText { text } if text.is_empty() => continue,
                            ContentItem::InputText { text } => json!({"type":"text","text":text}),
                            ContentItem::InputImage { image: ImageReference::Inline { image_url }, .. } => {
                                let (metadata, data) = image_url.split_once(',')
                                    .context("native image requires a base64 data URL")?;
                                let media_type = metadata.strip_prefix("data:")
                                    .and_then(|metadata| metadata.strip_suffix(";base64"))
                                    .filter(|media_type| matches!(*media_type,
                                        "image/png" | "image/jpeg" | "image/gif" | "image/webp"))
                                    .context("native image requires PNG, JPEG, GIF, or WebP base64 data")?;
                                json!({"type":"image","source":{"type":"base64","media_type":media_type,"data":data}})
                            }
                            _ => bail!("unsupported user history cannot be delivered to the native runtime"),
                        });
                    }
                    messages.push(Value::Array(blocks));
                }
            }
            // Native received mail uses AgentMessage; sent mail is an assistant Message.
            ResponseItem::AgentMessage {
                author,
                recipient,
                content,
                ..
            } => {
                let mut texts = Vec::new();
                for part in content {
                    let AgentMessageInputContent::InputText { text } = part else {
                        bail!("encrypted agent history cannot be delivered to the native runtime");
                    };
                    texts.push(text.as_str());
                }
                messages.push(Value::String(format!(
                    "Message from {author} to {recipient}:\n{}",
                    texts.join("\n")
                )));
            }
            _ => {}
        }
    }
    Ok(messages)
}

pub(super) fn input_prefix(messages: &[Value]) -> anyhow::Result<String> {
    use sha1::Digest;
    Ok(format!(
        "{:x}",
        sha1::Sha1::digest(serde_json::to_vec(messages)?)
    ))
}

pub(super) fn prompt_content(messages: &[Value]) -> anyhow::Result<codex_external_runtime::Prompt> {
    use codex_external_runtime::InputBlock;
    use codex_external_runtime::Prompt;
    if messages.iter().all(Value::is_string) {
        return Ok(Prompt::Text(
            messages
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n\n"),
        ));
    }
    let mut blocks = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        if index > 0 {
            blocks.push(InputBlock::Text {
                text: "\n\n".to_owned(),
            });
        }
        match message {
            Value::String(text) if text.is_empty() => {}
            Value::String(text) => blocks.push(InputBlock::Text { text: text.clone() }),
            Value::Array(content) => {
                for block in content {
                    blocks.push(match block["type"].as_str() {
                        Some("text") => InputBlock::Text {
                            text: block["text"]
                                .as_str()
                                .context("invalid saved external text input")?
                                .to_owned(),
                        },
                        Some("image") => InputBlock::Image {
                            mime_type: block["source"]["media_type"]
                                .as_str()
                                .context("invalid saved external image media type")?
                                .to_owned(),
                            data: block["source"]["data"]
                                .as_str()
                                .context("invalid saved external image data")?
                                .to_owned(),
                        },
                        _ => bail!("unsupported saved external input block"),
                    });
                }
            }
            _ => bail!("unsupported saved external input message"),
        }
    }
    Ok(Prompt::Blocks(blocks))
}

pub(super) fn validate_pending_prompt(
    session: &Session,
    messages: &[Value],
    pending: &[TurnInput],
    boundary: Option<&str>,
    validate: impl FnOnce(&codex_external_runtime::Prompt) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    // Preview the same normalized content that admission will record, without
    // changing history or the native delivery checkpoint on validation failure.
    let items = pending
        .iter()
        .map(|item| match item {
            TurnInput::UserInput { content, .. } => {
                session.response_item_from_user_input(content.clone())
            }
            TurnInput::ResponseItem(envelope) | TurnInput::FunctionCallOutput(envelope) => {
                envelope.item.clone()
            }
            TurnInput::InterAgentCommunication(message) => message.to_model_input_item(),
        })
        .collect::<Vec<_>>();
    let mut messages = messages.to_vec();
    messages.extend(input_messages(&items)?);
    if let Some(boundary) = boundary {
        messages.insert(/*index*/ 0, Value::String(boundary.to_owned()));
    }
    validate(&prompt_content(&messages)?)
}
