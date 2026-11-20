use crate::context_manager::ContextManager;
use codex_history::CodexHarnessMetadata;
use codex_history::HEARTBEAT_CONTENT_KIND;
use codex_history::InitialHistory;
use codex_history::ResponseItemEnvelope;
use codex_protocol::error::CodexErr;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ContentItemMetadata;
use codex_protocol::models::ContentItemProvenance;
use codex_protocol::models::ImageReference;
use codex_protocol::models::InternalChatMessageMetadataPassthrough;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TruncationPolicy;

pub(crate) struct PortableHistory;

pub(crate) fn project(
    history: &InitialHistory,
    max_history_bytes: usize,
    retain: impl Fn(&ResponseItemEnvelope) -> bool,
) -> Result<Vec<ResponseItemEnvelope>, CodexErr> {
    let reconstructed = ContextManager::reconstruct_rollout(
        history.get_rollout_items(),
        history.get_history_mode(Default::default()),
        ContextManager::new,
        TruncationPolicy::Bytes(0),
    );
    let mut items = Vec::new();
    for envelope in reconstructed.history.into_iter().filter(retain) {
        let ResponseItem::Message {
            role,
            content,
            phase,
            encrypted_content: None,
            internal_chat_message_metadata_passthrough: metadata,
            ..
        } = envelope.item.as_ref()
        else {
            continue;
        };
        if envelope
            .metadata
            .as_ref()
            .is_some_and(|meta| meta.compaction_output)
        {
            continue;
        }
        let mut portable = Vec::new();
        for (index, part) in crate::event_mapping::user_input_parts(content) {
            match (role.as_str(), part) {
                (
                    "assistant",
                    ContentItem::InputText { text } | ContentItem::OutputText { text, .. },
                ) if !text.is_empty() => portable.push(ContentItem::OutputText {
                    text: text.clone(),
                    annotations: None,
                    logprobs: None,
                }),
                ("user", part) if is_authored(metadata.as_ref(), index) => match part {
                    ContentItem::InputText { text } if !text.is_empty() => {
                        portable.push(part.clone())
                    }
                    ContentItem::InputImage {
                        image: ImageReference::Inline { image_url },
                        ..
                    } if image_url.split_once(',').is_some_and(|(header, data)| {
                        !data.is_empty()
                            && matches!(
                                header,
                                "data:image/png;base64"
                                    | "data:image/jpeg;base64"
                                    | "data:image/gif;base64"
                                    | "data:image/webp;base64"
                            )
                    }) =>
                    {
                        portable.push(part.clone())
                    }
                    ContentItem::InputImage { .. } | ContentItem::InputAudio { .. } => {
                        return Err(CodexErr::InvalidRequest(
                            "handoff supports only inline PNG, JPEG, GIF, and WebP media"
                                .to_owned(),
                        ));
                    }
                    ContentItem::InputText { .. } | ContentItem::OutputText { .. } => {}
                },
                _ => {}
            }
        }
        if portable.is_empty() {
            continue;
        }
        let attribution = (role == "user").then(|| InternalChatMessageMetadataPassthrough {
            content_item_metadata: Some(vec![ContentItemMetadata::user(); portable.len()]),
            ..Default::default()
        });
        items.push(ResponseItemEnvelope {
            item: ResponseItem::Message {
                id: None,
                status: None,
                role: role.clone(),
                content: portable,
                phase: phase.clone(),
                encrypted_content: None,
                internal_chat_message_metadata_passthrough: attribution,
            }
            .into(),
            metadata: Some(CodexHarnessMetadata {
                inherited_user_message: true,
                portable_handoff: true,
                ..Default::default()
            }),
        });
    }
    let messages: Vec<_> = items.iter().map(|envelope| &envelope.item).collect();
    let byte_count = serde_json::to_vec(&messages)?.len();
    if byte_count > max_history_bytes {
        return Err(CodexErr::InvalidRequest(format!(
            "handoff history needs {byte_count} bytes; handoff.max_history_bytes is {max_history_bytes}"
        )));
    }
    Ok(items)
}

fn is_authored(metadata: Option<&InternalChatMessageMetadataPassthrough>, index: usize) -> bool {
    let kind = metadata
        .and_then(|meta| meta.content_item_kinds.as_ref()?.get(index))
        .map(|kind| kind.as_str());
    match metadata
        .and_then(|meta| meta.content_item_metadata.as_ref()?.get(index))
        .map(|entry| &entry.provenance)
    {
        Some(ContentItemProvenance::User) => kind != Some(HEARTBEAT_CONTENT_KIND),
        None | Some(ContentItemProvenance::Unknown) => {
            matches!(kind, Some("user.text" | "user.image" | "user.audio"))
        }
        _ => false,
    }
}
