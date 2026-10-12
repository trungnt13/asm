use super::*;
use codex_app_server_protocol::DynamicToolCallOutputContentItem;
use codex_app_server_protocol::DynamicToolCallParams;
use codex_app_server_protocol::DynamicToolCallResponse;
use codex_app_server_protocol::DynamicToolFunctionSpec;
use codex_app_server_protocol::DynamicToolSpec;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadAppendToolsParams;
use codex_app_server_protocol::ThreadAppendToolsResponse;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use serde::Deserialize;
use serde_json::json;

pub(super) const COMPANION_MESSAGE_TOOL: &str = "send_companion_message";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MessageArgs {
    message: String,
}

impl App {
    #[tracing::instrument(skip_all)]
    pub(super) async fn enable_companion_messages(
        &self,
        app_server: &AppServerSession,
        thread_id: ThreadId,
    ) -> Result<()> {
        app_server
            .request_handle()
            .request_typed::<ThreadAppendToolsResponse>(ClientRequest::ThreadAppendTools {
                request_id: RequestId::String(uuid::Uuid::new_v4().to_string()),
                params: ThreadAppendToolsParams {
                    thread_id: thread_id.to_string(),
                    tools: vec![DynamicToolSpec::Function(DynamicToolFunctionSpec {
                        name: COMPANION_MESSAGE_TOOL.to_string(),
                        description: "Send a message to the other conversation in the active main/companion pair. Deliver promptly to a running recipient or start an idle recipient. Returns after acceptance without waiting for a reply.".to_string(),
                        input_schema: json!({
                            "type": "object",
                            "properties": {"message": {"type": "string"}},
                            "required": ["message"],
                            "additionalProperties": false
                        }),
                        defer_loading: false,
                    })],
                },
            })
            .await?;
        Ok(())
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn send_companion_message(
        &mut self,
        app_server: &AppServerSession,
        request_id: RequestId,
        params: DynamicToolCallParams,
    ) {
        if self.dynamic_tool_tasks.contains_key(&request_id) {
            return;
        }
        let route = (|| {
            let source =
                ThreadId::from_string(&params.thread_id).map_err(|error| error.to_string())?;
            let args: MessageArgs = serde_json::from_value(params.arguments.clone())
                .map_err(|error| error.to_string())?;
            if args.message.trim().is_empty() {
                return Err("message must not be blank".to_string());
            }
            let (child, state) = self
                .side_threads
                .iter()
                .find(|(child, state)| **child == source || state.parent_thread_id == source)
                .ok_or_else(|| "No active companion conversation".to_string())?;
            if !state.messaging_available {
                return Err("Companion messaging is unavailable on this pair".to_string());
            }
            if self.abandoned_side_threads.contains(child)
                || self
                    .abandoned_side_threads
                    .contains(&state.parent_thread_id)
            {
                return Err("The companion conversation was closed".to_string());
            }
            let recipient = if source == *child {
                state.parent_thread_id
            } else {
                *child
            };
            let message = format!(
                "Message Type: MESSAGE\nTask name: {recipient}\nSender: {source}\nPayload:\n{}",
                args.message
            );
            Ok((recipient, message))
        })();
        let (recipient, message) = match route {
            Ok(route) => route,
            Err(error) => {
                self.app_event_tx.send(AppEvent::DynamicToolCallCompleted {
                    request_id,
                    response: crate::dynamic_tools::failure_response(error),
                });
                return;
            }
        };
        // Admission completes before the TUI can close or replace this pair. This
        // awaits only the atomic start/steer acknowledgement, never a model reply.
        let result = app_server
            .request_handle()
            .request_typed::<TurnStartResponse>(ClientRequest::TurnStart {
                request_id: RequestId::String(uuid::Uuid::new_v4().to_string()),
                params: TurnStartParams {
                    thread_id: recipient.to_string(),
                    assistant_message: Some(message),
                    ..TurnStartParams::default()
                },
            })
            .await;
        let response = match result {
            Ok(_) => DynamicToolCallResponse {
                success: true,
                content_items: vec![DynamicToolCallOutputContentItem::InputText {
                    text: json!({"status": "accepted", "recipient": recipient.to_string()})
                        .to_string(),
                }],
            },
            Err(error) => crate::dynamic_tools::failure_response(error.to_string()),
        };
        self.app_event_tx.send(AppEvent::DynamicToolCallCompleted {
            request_id,
            response,
        });
    }
}
