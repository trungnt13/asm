use crate::context::ContextualUserFragment;
use crate::context::RealtimeDelegation;
use crate::context_manager::ContextManager;
use codex_history::CodexHarnessMetadata;
use codex_history::HEARTBEAT_CONTENT_KIND;
use codex_history::InitialHistory;
use codex_history::ResponseItemEnvelope;
use codex_history::RolloutItem;
use codex_protocol::ThreadId;
use codex_protocol::error::CodexErr;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ContentItemMetadata;
use codex_protocol::models::ContentItemProvenance;
use codex_protocol::models::ImageReference;
use codex_protocol::models::InternalChatMessageMetadataPassthrough;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InterAgentCommunication;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_protocol::protocol::TruncationPolicy;
use codex_protocol::protocol::TurnCompleteEvent;
use codex_protocol::protocol::TurnStartedEvent;
use codex_utils_string::serialized_json_bytes;

pub(crate) enum PortableHistory {
    Visible,
    Hidden,
}

pub(crate) fn project(
    history: &InitialHistory,
    history_mode: ThreadHistoryMode,
    max_history_bytes: usize,
    retain: impl Fn(&ResponseItemEnvelope) -> bool,
) -> Result<Vec<ResponseItemEnvelope>, CodexErr> {
    let reconstructed = ContextManager::reconstruct_rollout(
        history.get_rollout_items(),
        history_mode,
        |context_ids| ContextManager::new(context_ids, /*retention*/ None),
        TruncationPolicy::Bytes(0),
    );
    let check_limit = |bytes| {
        if bytes > max_history_bytes {
            return Err(CodexErr::InvalidRequest(format!(
                "handoff history needs at least {bytes} bytes; handoff.max_history_bytes is {max_history_bytes}"
            )));
        }
        Ok(())
    };
    let mut bytes = 2usize; // JSON array brackets.
    check_limit(bytes)?;
    let mut items = Vec::new();
    let mut last_source_turn = None;
    let mut turn_id = crate::session::new_submission_id();
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
        if envelope.metadata.as_ref().is_some_and(|meta| {
            meta.compaction_output
                || meta.realtime_handoff.is_some()
                || meta.realtime_session_input.is_some()
                || meta.realtime_session_end_input
        }) || (role == "assistant" && InterAgentCommunication::is_message_content(content))
        {
            continue;
        }
        let mut portable = Vec::new();
        let prefix_bytes = bytes.saturating_add(usize::from(!items.is_empty()));
        let mut payload_bytes = prefix_bytes;
        for (index, part) in crate::event_mapping::user_input_parts(content) {
            let payload_len = match (role.as_str(), part) {
                (
                    "assistant",
                    ContentItem::InputText { text } | ContentItem::OutputText { text, .. },
                ) if !text.is_empty() => text.len(),
                ("user", part) if is_authored(metadata.as_ref(), index) => match part {
                    ContentItem::InputText { text }
                        if !text.is_empty() && !is_legacy_realtime_wrapper(text) =>
                    {
                        text.len()
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
                        image_url.len()
                    }
                    ContentItem::InputImage { .. } | ContentItem::InputAudio { .. } => {
                        return Err(CodexErr::InvalidRequest(
                            "handoff supports only inline PNG, JPEG, GIF, and WebP media"
                                .to_owned(),
                        ));
                    }
                    ContentItem::InputText { .. } | ContentItem::OutputText { .. } => continue,
                },
                _ => continue,
            };
            // A JSON string cannot be shorter than its raw UTF-8 payload.
            payload_bytes = payload_bytes.saturating_add(payload_len);
            check_limit(payload_bytes)?;
            portable.push(match part {
                ContentItem::InputText { text } | ContentItem::OutputText { text, .. }
                    if role == "assistant" =>
                {
                    ContentItem::OutputText {
                        text: text.clone(),
                        annotations: None,
                        logprobs: None,
                    }
                }
                part => part.clone(),
            });
        }
        if portable.is_empty() {
            continue;
        }
        let source_turn = envelope.item.turn_id();
        if source_turn.is_some_and(|id| last_source_turn.as_deref() != Some(id))
            || (source_turn.is_none() && role == "user")
        {
            turn_id = crate::session::new_submission_id();
            last_source_turn = source_turn.map(str::to_owned);
        }
        let envelope = ResponseItemEnvelope {
            item: ResponseItem::Message {
                id: None,
                status: None,
                role: role.clone(),
                phase: phase.clone(),
                encrypted_content: None,
                internal_chat_message_metadata_passthrough: Some(
                    InternalChatMessageMetadataPassthrough {
                        turn_id: Some(turn_id.clone()),
                        content_item_metadata: (role == "user")
                            .then(|| vec![ContentItemMetadata::user(); portable.len()]),
                        ..Default::default()
                    },
                ),
                content: portable.into(),
            }
            .into(),
            metadata: Some(CodexHarnessMetadata {
                retain_in_children: envelope
                    .metadata
                    .as_ref()
                    .and_then(|meta| meta.retain_in_children),
                retain_across_windows: envelope
                    .metadata
                    .as_ref()
                    .and_then(|meta| meta.retain_across_windows),
                inherited_user_message: true,
                portable_handoff: true,
                ..Default::default()
            }),
        };
        bytes = prefix_bytes.saturating_add(serialized_json_bytes(envelope.item.as_ref())?);
        check_limit(bytes)?;
        items.push(envelope);
    }
    Ok(items)
}

pub(crate) fn copied_turns(items: Vec<RolloutItem>, thread_id: ThreadId) -> Vec<RolloutItem> {
    let turn_id = |record: &RolloutItem| match record {
        RolloutItem::ResponseItem(item) => item.item.turn_id().unwrap_or_default().to_owned(),
        _ => unreachable!("a projected prefix contains only model messages"),
    };
    let mut rollout = Vec::new();
    for turn in items.chunk_by(|a, b| turn_id(a) == turn_id(b)) {
        let turn_id = turn_id(&turn[0]);
        rollout.push(RolloutItem::EventMsg(EventMsg::TurnStarted(
            TurnStartedEvent {
                turn_id: turn_id.clone(),
                root_turn_id: None,
                trace_id: None,
                started_at: None,
                turn_attribution: None,
                model_context_window: None,
                collaboration_mode_kind: Default::default(),
            },
        )));
        for record in turn {
            rollout.push(record.clone());
            if let RolloutItem::ResponseItem(envelope) = record
                && let Some(item) = crate::event_mapping::parse_turn_item(&envelope.item)
            {
                rollout.extend(
                    item.as_legacy_events(/*show_raw_agent_reasoning*/ false)
                        .into_iter()
                        .map(RolloutItem::EventMsg),
                );
                rollout.push(RolloutItem::EventMsg(EventMsg::ItemCompleted(
                    ItemCompletedEvent {
                        thread_id,
                        turn_id: turn_id.clone(),
                        item,
                        content_token_count: None,
                        started_at_ms: None,
                        completed_at_ms: crate::turn_timing::now_unix_timestamp_ms(),
                    },
                )));
            }
        }
        rollout.push(RolloutItem::EventMsg(EventMsg::TurnComplete(
            TurnCompleteEvent {
                turn_id,
                root_turn_id: None,
                last_agent_message: None,
                error: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
                time_to_first_token_ms: None,
            },
        )));
    }
    rollout
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

fn is_legacy_realtime_wrapper(text: &str) -> bool {
    // Old realtime writers recorded these rendered transcripts as ordinary user input.
    // Only the generated layout is ambiguous, not text merely enclosed in the tag.
    let (open, close) = RealtimeDelegation::type_markers();
    text.strip_prefix(open)
        .and_then(|body| body.strip_suffix(close))
        .map(|body| {
            body.strip_prefix("\n  <source>transcript_tail_flush</source>")
                .unwrap_or(body)
        })
        .and_then(|body| body.strip_prefix("\n  <input>"))
        .and_then(|body| body.split_once("</input>\n"))
        .is_some_and(|(_, tail)| {
            tail.is_empty()
                || (tail.starts_with("  <transcript_delta>")
                    && tail.ends_with("</transcript_delta>\n"))
        })
}
