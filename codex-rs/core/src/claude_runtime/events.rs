use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::AgentMessageContentDeltaEvent;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::TokenUsage;
use serde_json::Value;
use serde_json::json;

#[derive(Default)]
pub(super) struct MessageStream {
    id: Option<String>,
    text: String,
    started: bool,
    pub(super) context: Option<RequestContext>,
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
    if frame["type"] == "stream_event" {
        let event = &frame["event"];
        if event["type"] == "message_start" {
            stream.id = event["message"]["id"].as_str().map(str::to_owned);
            stream.text.clear();
            stream.started = false;
            return Ok(None);
        }
        if event["delta"]["type"] == "text_delta"
            && let (Some(id), Some(delta)) = (stream.id.as_ref(), event["delta"]["text"].as_str())
        {
            if !stream.started {
                session
                    .emit_turn_item_started(
                        turn,
                        &TurnItem::AgentMessage(AgentMessageItem {
                            id: id.clone(),
                            content: Vec::new(),
                            phase: None,
                            memory_citation: None,
                            delivery: None,
                            questions: None,
                        }),
                    )
                    .await;
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
    let id = stream
        .id
        .take()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let text = std::mem::take(&mut stream.text);
    if text.is_empty() {
        stream.started = false;
        return Ok(None);
    }
    let item = serde_json::from_value(json!({"type":"message","id":id,"role":"assistant",
        "content":[{"type":"output_text","text":text}]}))?;
    session
        .record_conversation_items(turn, &turn.capture_current_model_info(), &[item])
        .await;
    if !stream.started {
        session
            .emit_turn_item_started(
                turn,
                &TurnItem::AgentMessage(AgentMessageItem {
                    id: id.clone(),
                    content: Vec::new(),
                    phase: None,
                    memory_citation: None,
                    delivery: None,
                    questions: None,
                }),
            )
            .await;
    }
    stream.started = false;
    session
        .emit_turn_item_completed(
            turn,
            TurnItem::AgentMessage(AgentMessageItem {
                id,
                content: vec![AgentMessageContent::Text { text: text.clone() }],
                phase: None,
                memory_citation: None,
                delivery: None,
                questions: None,
            }),
        )
        .await;
    Ok(Some(text))
}
