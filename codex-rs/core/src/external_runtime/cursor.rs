//! Projects Cursor SDK text and reasoning into the ASM transcript.

use super::events::MessageStream;
use super::events::complete_message;
use super::events::emit_message_started;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_protocol::ResponseItemId;
use codex_protocol::items::ReasoningItem;
use codex_protocol::items::TurnItem;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::AgentMessageContentDeltaEvent;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ReasoningRawContentDeltaEvent;
use codex_protocol::protocol::TokenUsage;
use serde_json::Value;

pub(super) struct CursorThinking {
    id: ResponseItemId,
    text: String,
}

#[tracing::instrument(skip_all)]
pub(super) async fn record_cursor_frame(
    session: &Session,
    turn: &TurnContext,
    frame: &Value,
    stream: &mut MessageStream,
) -> anyhow::Result<Option<String>> {
    let kind = frame["type"].as_str();
    match kind {
        Some("assistant_end") => return complete_cursor_message(session, turn, stream).await,
        Some("thinking_end") => {
            complete_cursor_thinking(session, turn, stream).await;
            return Ok(None);
        }
        _ => {}
    }
    let Some(delta) = frame["text"].as_str().filter(|text| !text.is_empty()) else {
        return Ok(None);
    };
    match kind {
        Some("assistant_delta") => {
            complete_cursor_thinking(session, turn, stream).await;
            let id = stream
                .id
                .get_or_insert_with(|| session.context_ids.next_item("msg").to_string());
            if !stream.started {
                emit_message_started(session, turn, id).await;
                stream.started = true;
            }
            stream.text.push_str(delta);
            session
                .send_event(
                    turn,
                    EventMsg::AgentMessageContentDelta(AgentMessageContentDeltaEvent {
                        thread_id: session.thread_id().to_string(),
                        turn_id: turn.sub_id.clone(),
                        item_id: id.clone(),
                        delta: delta.to_owned(),
                    }),
                )
                .await;
            Ok(None)
        }
        Some("thinking_delta") => {
            let message = if stream.started {
                complete_message(session, turn, stream).await?
            } else {
                None
            };
            if stream.cursor_thinking.is_none() {
                let id = session.context_ids.next_item("rs");
                session
                    .emit_turn_item_started(
                        turn,
                        &TurnItem::Reasoning(ReasoningItem {
                            id: id.to_string(),
                            summary_text: Vec::new(),
                            raw_content: Vec::new(),
                        }),
                    )
                    .await;
                stream.cursor_thinking = Some(CursorThinking {
                    id,
                    text: String::new(),
                });
            }
            let thinking = stream.cursor_thinking.as_mut().expect("reasoning started");
            thinking.text.push_str(delta);
            session
                .send_event(
                    turn,
                    EventMsg::ReasoningRawContentDelta(ReasoningRawContentDeltaEvent {
                        thread_id: session.thread_id().to_string(),
                        turn_id: turn.sub_id.clone(),
                        item_id: thinking.id.to_string(),
                        delta: delta.to_owned(),
                        content_index: 0,
                    }),
                )
                .await;
            Ok(message)
        }
        _ => Ok(None),
    }
}

#[tracing::instrument(skip_all)]
pub(super) async fn complete_cursor_message(
    session: &Session,
    turn: &TurnContext,
    stream: &mut MessageStream,
) -> anyhow::Result<Option<String>> {
    complete_cursor_thinking(session, turn, stream).await;
    if stream.started {
        complete_message(session, turn, stream).await
    } else {
        Ok(None)
    }
}

#[tracing::instrument(skip_all)]
async fn complete_cursor_thinking(
    session: &Session,
    turn: &TurnContext,
    stream: &mut MessageStream,
) {
    let Some(CursorThinking { id, text }) = stream.cursor_thinking.take() else {
        return;
    };
    let item_id = id.to_string();
    let item = ResponseItem::Reasoning {
        id: Some(id),
        summary: Vec::new(),
        content: Some(vec![ReasoningItemContent::ReasoningText {
            text: text.clone(),
        }]),
        encrypted_content: None,
        internal_chat_message_metadata_passthrough: None,
    };
    session
        .record_conversation_items(turn, &turn.capture_current_model_info(), &[item])
        .await;
    session
        .emit_turn_item_completed(
            turn,
            TurnItem::Reasoning(ReasoningItem {
                id: item_id,
                summary_text: Vec::new(),
                raw_content: vec![text],
            }),
        )
        .await;
}

pub(super) fn reported_usage(result: &Value) -> Option<TokenUsage> {
    let usage = &result["usage"];
    let count = |field: &str| usage[field].as_i64().filter(|count| *count >= 0);
    let output_tokens = count("outputTokens")?;
    let reasoning_output_tokens = if usage.get("reasoningTokens").is_some() {
        count("reasoningTokens").filter(|count| *count <= output_tokens)?
    } else {
        // The SDK reports reasoning only when available; TokenUsage has no optional counter.
        0
    };
    Some(TokenUsage {
        input_tokens: count("inputTokens")?,
        cached_input_tokens: count("cacheReadTokens")?,
        cache_write_input_tokens: count("cacheWriteTokens")?,
        output_tokens,
        reasoning_output_tokens,
        total_tokens: count("totalTokens")?,
        codex_rollout_budget_units: None,
    })
}
