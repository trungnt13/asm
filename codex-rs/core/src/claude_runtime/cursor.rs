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

/// A native child session announced during the turn.
pub(super) struct CursorChild {
    /// Activity key of the child's lifecycle cell.
    key: String,
    /// Child assistant text, which becomes the lifecycle cell result.
    text: String,
}

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
    // Child sessions stream into their own scope instead of the root transcript.
    let scope = frame["params"]["sessionId"]
        .as_str()
        .filter(|session| stream.cursor_children.contains_key(*session))
        .unwrap_or_default()
        .to_owned();
    match update["sessionUpdate"].as_str() {
        Some("agent_message_chunk") => {
            let Some(delta) = update["content"]["text"].as_str() else {
                return Ok(None);
            };
            if let Some(child) = stream.cursor_children.get_mut(&scope) {
                child.text.push_str(delta);
                return Ok(None);
            }
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
            // A root tool call separates assistant messages.
            let text = if scope.is_empty() {
                complete_message(session, turn, stream).await?
            } else {
                None
            };
            let call = json!({"id": update["toolCallId"], "name": update["title"],
                "input": update["rawInput"]});
            stream.activity.start(session, turn, &scope, &call).await?;
            Ok(text)
        }
        Some("subagent_spawned") => {
            let Some(child) = update["subagentSessionId"].as_str() else {
                return Ok(None);
            };
            let name = update["name"].as_str().unwrap_or("subagent");
            let call = json!({"id": child, "name": format!("Subagent {name}"),
                "input": {"task": update["task"], "cursor": update["_meta"]["cursor"]}});
            stream.activity.start(session, turn, &scope, &call).await?;
            let key = format!("{scope}:{child}");
            let text = String::new();
            stream
                .cursor_children
                .insert(child.to_owned(), CursorChild { key, text });
            Ok(None)
        }
        Some("subagent_state_update") => {
            let state = update["state"].as_str();
            if let Some(child) = update["subagentSessionId"]
                .as_str()
                .and_then(|child| stream.cursor_children.get_mut(child))
                && matches!(
                    state,
                    Some("completed" | "failed" | "cancelled" | "disconnected")
                )
            {
                let key = child.key.clone();
                let text = std::mem::take(&mut child.text);
                // `disconnected` means Cursor stopped waiting, not that the child stopped.
                let failed = state != Some("completed");
                let content = if failed {
                    json!(format!(
                        "Cursor subagent {}\n{text}",
                        state.unwrap_or_default()
                    ))
                } else {
                    json!(text)
                };
                stream
                    .activity
                    .finish(session, turn, &key, content, failed)
                    .await?;
            }
            Ok(None)
        }
        Some("tool_call_update") => {
            let Some(id) = update["toolCallId"].as_str() else {
                return Ok(None);
            };
            // Cursor often sends the full command or title only in a status-free refresh.
            let key = format!("{scope}:{id}");
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
                // Cursor reports finished tools as completed; typed top-level output fields
                // carry native failures and refusals. Missing fields mean unknown.
                let raw = &update["rawOutput"];
                let failed = status == Some("failed")
                    || raw["exitCode"].as_i64().is_some_and(|code| code != 0)
                    || raw["error"].is_string()
                    || raw["rejected"] == true
                    || raw["permissionDenied"] == true;
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
