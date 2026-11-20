use super::active_handoff::HandoffCapture;
use super::active_handoff::HandoffCut;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::items::TurnItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::protocol::AgentMessageContentDeltaEvent;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::TokenUsage;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::time::Instant;

#[derive(Default)]
pub(super) struct MessageStream {
    pub(super) results: Vec<Value>,
    pub(super) activity: super::activity::ActivityStream,
    pub(super) id: Option<String>,
    pub(super) text: String,
    handoff_tools: HashMap<String, String>,
    pub(super) submitted_steering_count: usize,
    pub(super) started: bool,
    phase: Option<MessagePhase>,
    generation_started_at: Option<Instant>,
    non_reasoning_tokens: Option<i64>,
    pub(super) output_tps: Option<f64>,
    pub(super) context: Option<RequestContext>,
    /// Cursor child sessions by session ID.
    pub(super) cursor_children: HashMap<String, super::cursor::CursorChild>,
}

pub(super) struct RequestContext {
    pub(super) model: String,
    pub(super) usage: TokenUsage,
}

#[tracing::instrument(skip_all)]
pub(super) async fn record_assistant_frame(
    session: &Session,
    turn: &TurnContext,
    frame: &Value,
    stream: &mut MessageStream,
) -> anyhow::Result<Option<String>> {
    if frame["parent_tool_use_id"].is_null() && frame["type"] == "stream_event" {
        let event = &frame["event"];
        match event["type"].as_str() {
            Some("message_start") => {
                stream.generation_started_at = None;
                stream.non_reasoning_tokens = None;
                stream.phase = None;
            }
            Some("content_block_delta") => {
                let delta = &event["delta"];
                let output = match delta["type"].as_str() {
                    Some("text_delta") => delta["text"].as_str(),
                    Some("input_json_delta") => delta["partial_json"].as_str(),
                    _ => None,
                };
                if output.is_some_and(|text| !text.is_empty())
                    && stream.generation_started_at.is_none()
                {
                    stream.generation_started_at = Some(Instant::now());
                }
            }
            Some("message_delta") => {
                let stop_reason = event["delta"]["stop_reason"].as_str();
                stream.phase = matches!(stop_reason, Some("end_turn" | "stop_sequence"))
                    .then_some(MessagePhase::FinalAnswer);
                let usage = &event["usage"];
                stream.non_reasoning_tokens = (|| {
                    if !matches!(stop_reason, Some("end_turn" | "tool_use")) {
                        return None;
                    }
                    let output = usage["output_tokens"].as_i64()?;
                    let thinking = usage["output_tokens_details"]["thinking_tokens"].as_i64()?;
                    (thinking >= 0).then_some(())?;
                    output.checked_sub(thinking).filter(|tokens| *tokens > 0)
                })();
            }
            Some("message_stop") => {
                if let Some((start, tokens)) = stream
                    .generation_started_at
                    .take()
                    .zip(stream.non_reasoning_tokens.take())
                {
                    let duration = start.elapsed().as_millis();
                    if duration > 0 {
                        stream.output_tps = Some(tokens as f64 * 1_000.0 / duration as f64);
                    }
                }
            }
            _ => {}
        }
    }
    stream.activity.record(session, turn, frame).await?;
    // Native Claude agents have their own streams; do not present them as this thread.
    if !frame["parent_tool_use_id"].is_null() {
        return Ok(None);
    }
    let message = if frame["type"] == "assistant" {
        Some(&frame["message"])
    } else if frame["type"] == "stream_event" && frame["event"]["type"] == "message_start" {
        Some(&frame["event"]["message"])
    } else {
        None
    };
    if let Some(message) = message {
        stream.context = (|| {
            let model = message["model"].as_str()?.to_owned();
            let usage = &message["usage"];
            let uncached = usage["input_tokens"]
                .as_i64()
                .filter(|tokens| *tokens >= 0)?;
            let cached = usage["cache_read_input_tokens"]
                .as_i64()
                .filter(|tokens| *tokens >= 0)?;
            let written = usage["cache_creation_input_tokens"]
                .as_i64()
                .filter(|tokens| *tokens >= 0)?;
            let input = uncached.checked_add(cached)?.checked_add(written)?;
            Some(RequestContext {
                model,
                // Claude's context fill excludes output; spending is tracked separately.
                usage: TokenUsage {
                    input_tokens: input,
                    cached_input_tokens: cached,
                    cache_write_input_tokens: written,
                    total_tokens: input,
                    ..TokenUsage::default()
                },
            })
        })();
    }
    let handoff_capture = turn.extension_data.get::<HandoffCapture>();
    if frame["type"] == "stream_event" {
        let event = &frame["event"];
        if event["type"] == "message_start" {
            if let Some(capture) = &handoff_capture {
                capture.send_replace(None);
            }
            stream.id = event["message"]["id"].as_str().map(str::to_owned);
            stream.text.clear();
            stream.handoff_tools.clear();
            stream.started = false;
            return Ok(None);
        }
        if handoff_capture.is_some()
            && stream.id.is_some()
            && event["type"] == "content_block_start"
            && event["content_block"]["type"] == "tool_use"
            && let Some(id) = event["content_block"]["id"].as_str()
            && let Some(name) = event["content_block"]["name"].as_str()
            && name.starts_with("mcp__asm__")
        {
            stream.handoff_tools.insert(id.to_owned(), name.to_owned());
        }
        if event["delta"]["type"] == "text_delta"
            && let (Some(id), Some(delta)) = (stream.id.as_ref(), event["delta"]["text"].as_str())
        {
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
            return Ok(None);
        }
        if event["type"] != "message_stop" {
            return Ok(None);
        }
    } else if frame["type"] == "assistant" && stream.id.is_none() {
        // Without streaming, each assistant frame supplies its complete text block.
        stream.id = frame["uuid"].as_str().map(str::to_owned);
        stream.phase = matches!(
            frame["message"]["stop_reason"].as_str(),
            Some("end_turn" | "stop_sequence")
        )
        .then_some(MessagePhase::FinalAnswer);
        stream.text = frame["message"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|part| part["type"] == "text")
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
    } else {
        // The CLI emits assistant frames per block, before message_stop; deltas own that text.
        return Ok(None);
    }
    let publish_handoff = frame["type"] == "stream_event" && stream.id.is_some();
    let text = complete_message(session, turn, stream).await?;
    if publish_handoff
        && !stream.handoff_tools.is_empty()
        // A replay echo may show queuing, not consumption by this assistant request.
        && stream.submitted_steering_count == 0
        && let Some(capture) = handoff_capture
    {
        capture.send_replace(Some(HandoffCut {
            tools: std::mem::take(&mut stream.handoff_tools),
            history: session.clone_history().await,
        }));
    }
    Ok(text)
}

/// Records the streamed assistant message, if any, as one completed transcript item.
pub(super) async fn complete_message(
    session: &Session,
    turn: &TurnContext,
    stream: &mut MessageStream,
) -> anyhow::Result<Option<String>> {
    let id = stream
        .id
        .take()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let text = std::mem::take(&mut stream.text);
    let phase = stream.phase.take();
    if text.is_empty() {
        stream.started = false;
        return Ok(None);
    }
    let item = serde_json::from_value(
        json!({"type":"message","id":id,"role":"assistant","phase":phase,
        "content":[{"type":"output_text","text":text}]}),
    )?;
    session
        .record_conversation_items(turn, &turn.capture_current_model_info(), &[item])
        .await;
    if !stream.started {
        emit_message_started(session, turn, &id).await;
    }
    stream.started = false;
    session
        .emit_turn_item_completed(
            turn,
            TurnItem::AgentMessage(AgentMessageItem {
                id,
                content: vec![AgentMessageContent::Text { text: text.clone() }],
                phase,
                memory_citation: None,
                moderation: None,
                delivery: None,
                questions: None,
            }),
        )
        .await;
    Ok(Some(text))
}

pub(super) async fn emit_message_started(session: &Session, turn: &TurnContext, id: &str) {
    session
        .emit_turn_item_started(
            turn,
            &TurnItem::AgentMessage(AgentMessageItem {
                id: id.to_owned(),
                content: Vec::new(),
                phase: None,
                memory_citation: None,
                moderation: None,
                delivery: None,
                questions: None,
            }),
        )
        .await;
}
