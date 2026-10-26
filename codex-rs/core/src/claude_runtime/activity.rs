use std::collections::HashMap;
use std::collections::HashSet;

use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_protocol::items::McpToolCallItem;
use codex_protocol::items::McpToolCallStatus;
use codex_protocol::items::TurnItem;
use serde_json::Value;
use serde_json::json;

#[derive(Default)]
pub(super) struct ActivityStream {
    calls: HashMap<String, Value>,
    seen: HashSet<String>,
    pub(super) messaging: bool,
    /// Native runtime ID shown as the activity server, such as `claude-code`.
    pub(super) server: &'static str,
    agent_tasks: HashSet<String>,
}

impl ActivityStream {
    pub(super) async fn record(
        &mut self,
        session: &Session,
        turn: &TurnContext,
        frame: &Value,
    ) -> anyhow::Result<()> {
        let scope = frame["parent_tool_use_id"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        if frame["type"] == "assistant" {
            for (index, block) in frame["message"]["content"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                if block["type"] == "tool_use" {
                    self.start(session, turn, &scope, block).await?;
                } else if !scope.is_empty() && block["type"] == "text" {
                    let id = frame["uuid"].as_str().or(frame["message"]["id"].as_str());
                    if let (Some(id), Some(text)) = (id, block["text"].as_str()) {
                        let block = json!({"id":format!("{id}:text:{index}"), "name":"__agent_message",
                            "input":{"parent_tool_use_id":scope, "agent_id":frame["agent_id"],
                                "subagent_type":frame["subagent_type"]}});
                        self.start(session, turn, &scope, &block).await?;
                        self.finish(
                            session,
                            turn,
                            &format!("{scope}:{id}:text:{index}"),
                            json!(text),
                            false,
                        )
                        .await?;
                    }
                }
            }
        }
        if frame["type"] == "system" {
            let subtype = frame["subtype"].as_str().unwrap_or("");
            let task_id = frame["task_id"].as_str().unwrap_or("");
            if subtype == "task_started" && frame["task_type"] == "local_agent" {
                self.agent_tasks.insert(task_id.to_owned());
            }
            let name = match subtype {
                "task_started" => Some("__agent_started"),
                "task_progress" | "task_updated" => Some("__agent_progress"),
                "task_notification" => Some("__agent_completed"),
                _ => None,
            };
            if let Some(name) = name.filter(|_| self.agent_tasks.contains(task_id)) {
                let id = frame["uuid"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        use std::hash::Hash;
                        use std::hash::Hasher;
                        let mut hash = std::collections::hash_map::DefaultHasher::new();
                        frame.to_string().hash(&mut hash);
                        format!("{task_id}:{subtype}:{:x}", hash.finish())
                    });
                let block = json!({"id":id, "name":name, "input":frame});
                self.start(session, turn, &scope, &block).await?;
                let failed = matches!(
                    frame["status"]
                        .as_str()
                        .or(frame["patch"]["status"].as_str()),
                    Some("failed" | "stopped")
                );
                self.finish(
                    session,
                    turn,
                    &format!("{scope}:{id}"),
                    frame.clone(),
                    failed,
                )
                .await?;
            }
        }
        if frame["type"] == "user" {
            for block in frame["message"]["content"].as_array().into_iter().flatten() {
                if block["type"] == "tool_result"
                    && let Some(id) = block["tool_use_id"].as_str()
                {
                    self.finish(
                        session,
                        turn,
                        &format!("{scope}:{id}"),
                        block["content"].clone(),
                        block["is_error"].as_bool().unwrap_or(false),
                    )
                    .await?;
                }
            }
        }
        Ok(())
    }

    pub(super) async fn start(
        &mut self,
        session: &Session,
        turn: &TurnContext,
        scope: &str,
        block: &Value,
    ) -> anyhow::Result<()> {
        let Some(id) = block["id"].as_str() else {
            return Ok(());
        };
        let Some(name) = block["name"].as_str() else {
            return Ok(());
        };
        // The bridge records its own calls; every tool on the `asm` server is bridged.
        if self.messaging && name.starts_with("mcp__asm__") {
            return Ok(());
        }
        let key = format!("{scope}:{id}");
        if !self.seen.insert(key.clone()) {
            return Ok(());
        }
        let server = if scope.is_empty() {
            self.server.to_owned()
        } else {
            format!("{}/{scope}", self.server)
        };
        let call = json!({
            "id": format!("{}:{}:{key}", turn.sub_id, self.server),
            "server": server, "tool": name, "arguments": block["input"],
            "status": "inProgress"
        });
        let item: McpToolCallItem = serde_json::from_value(call.clone())?;
        session
            .emit_turn_item_started(turn, &TurnItem::McpToolCall(item))
            .await;
        self.calls.insert(key, call);
        Ok(())
    }

    /// Applies a native update's title or arguments to a running call before completion.
    pub(super) fn update(&mut self, key: &str, name: &Value, input: &Value) {
        if let Some(call) = self.calls.get_mut(key) {
            if name.is_string() {
                call["tool"] = name.clone();
            }
            if !input.is_null() {
                call["arguments"] = input.clone();
            }
        }
    }

    pub(super) async fn finish(
        &mut self,
        session: &Session,
        turn: &TurnContext,
        key: &str,
        content: Value,
        failed: bool,
    ) -> anyhow::Result<()> {
        let Some(mut call) = self.calls.remove(key) else {
            return Ok(());
        };
        call["status"] = json!(if failed {
            McpToolCallStatus::Failed
        } else {
            McpToolCallStatus::Completed
        });
        let text = match content {
            Value::String(text) => text,
            Value::Array(blocks) => blocks
                .into_iter()
                .map(|block| match block["type"].as_str() {
                    Some("image" | "audio" | "document") => {
                        format!("[{} content]", block["type"].as_str().unwrap_or("binary"))
                    }
                    _ => block["text"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| block.to_string()),
                })
                .collect::<Vec<_>>()
                .join("\n"),
            content => content.to_string(),
        };
        call["result"] = json!({"content":[{"type":"text","text":text}], "isError": failed});
        session
            .emit_turn_item_completed(turn, TurnItem::McpToolCall(serde_json::from_value(call)?))
            .await;
        Ok(())
    }

    pub(super) async fn close(
        &mut self,
        session: &Session,
        turn: &TurnContext,
    ) -> anyhow::Result<()> {
        for key in self.calls.keys().cloned().collect::<Vec<_>>() {
            self.finish(
                session,
                turn,
                &key,
                json!("Native activity ended without a tool result"),
                true,
            )
            .await?;
        }
        Ok(())
    }
}
