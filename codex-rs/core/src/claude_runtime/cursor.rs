//! Projects Cursor ACP frames into the shared native transcript and activity cells.

use super::events::MessageStream;
use super::events::complete_message;
use super::events::emit_message_started;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_protocol::protocol::AgentMessageContentDeltaEvent;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::WarningEvent;
use serde_json::Value;
use serde_json::json;

#[tracing::instrument(skip_all)]
pub(super) async fn record_cursor_frame(
    session: &Session,
    turn: &TurnContext,
    frame: &Value,
    stream: &mut MessageStream,
) -> anyhow::Result<Option<String>> {
    let Some(method) = frame["method"].as_str() else {
        // The prompt response ends the turn and its last message.
        return complete_message(session, turn, stream).await;
    };
    if method != "session/update" {
        // The runtime already refused agent requests; make each refusal visible.
        if frame.get("id").is_some() {
            let message = if method == "session/request_permission" {
                let title = frame["params"]["toolCall"]["title"]
                    .as_str()
                    .unwrap_or("a tool call");
                format!("Cursor requested approval for {title}; ASM denied it.")
            } else {
                format!("Cursor requested unsupported `{method}`; ASM refused it.")
            };
            session
                .send_event(turn, EventMsg::Warning(WarningEvent { message }))
                .await;
        }
        return Ok(None);
    }
    let update = &frame["params"]["update"];
    match update["sessionUpdate"].as_str() {
        Some("agent_message_chunk") => {
            let Some(delta) = update["content"]["text"].as_str() else {
                return Ok(None);
            };
            let id = stream
                .id
                .get_or_insert_with(|| uuid::Uuid::new_v4().to_string())
                .clone();
            if !stream.started {
                emit_message_started(session, turn, &id).await;
                stream.started = true;
            }
            stream.text.push_str(delta);
            session
                .send_event(
                    turn,
                    EventMsg::AgentMessageContentDelta(AgentMessageContentDeltaEvent {
                        thread_id: session.thread_id().to_string(),
                        turn_id: turn.sub_id.clone(),
                        item_id: id,
                        delta: delta.to_owned(),
                    }),
                )
                .await;
            Ok(None)
        }
        Some("tool_call") => {
            // A tool call separates assistant messages.
            let text = complete_message(session, turn, stream).await?;
            let call = json!({"id": update["toolCallId"], "name": update["title"],
                "input": update["rawInput"]});
            stream.activity.start(session, turn, "", &call).await?;
            Ok(text)
        }
        Some("tool_call_update") => {
            let Some(id) = update["toolCallId"].as_str() else {
                return Ok(None);
            };
            // Cursor often sends the full command or title only in a status-free refresh.
            let key = format!(":{id}");
            stream
                .activity
                .update(&key, &update["title"], &update["rawInput"]);
            let status = update["status"].as_str();
            if matches!(status, Some("completed" | "failed")) {
                let output = if update["rawOutput"].is_null() {
                    update["content"].clone()
                } else {
                    update["rawOutput"].clone()
                };
                let failed = status == Some("failed");
                stream
                    .activity
                    .finish(session, turn, &key, output, failed)
                    .await?;
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}
