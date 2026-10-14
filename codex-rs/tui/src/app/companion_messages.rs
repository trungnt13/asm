use super::*;
use crate::app_event::SideConversationMode;
use codex_app_server_protocol::DynamicToolCallOutputContentItem;
use codex_app_server_protocol::DynamicToolCallParams;
use codex_app_server_protocol::DynamicToolCallResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadCompanionLifecycleParams;
use codex_app_server_protocol::ThreadCompanionLifecycleResponse;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_protocol::companion::CompanionLifecycleEvent;
use codex_protocol::companion::CompanionRelationship;
use serde::Deserialize;
use serde_json::json;

pub(super) use crate::dynamic_tools::COMPANION_MESSAGE_TOOL;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MessageArgs {
    message: String,
}

impl App {
    #[tracing::instrument(skip_all)]
    pub(super) async fn record_companion_lifecycle(
        &mut self,
        app_server: &AppServerSession,
        notices: impl IntoIterator<Item = (ThreadId, CompanionLifecycleEvent)>,
    ) {
        for (thread_id, event) in notices {
            if let Err(error) = app_server
                .request_handle()
                .request_typed::<ThreadCompanionLifecycleResponse>(
                    ClientRequest::ThreadCompanionLifecycle {
                        request_id: RequestId::String(uuid::Uuid::new_v4().to_string()),
                        params: ThreadCompanionLifecycleParams {
                            thread_id: thread_id.to_string(),
                            event,
                        },
                    },
                )
                .await
            {
                tracing::warn!(%thread_id, %error, "failed to record companion lifecycle");
                let expected_closed_child = self.abandoned_side_threads.contains(&thread_id)
                    && matches!(&error, codex_app_server_client::TypedRequestError::Server { source, .. }
                        if source.code == -32600 && (source.message.contains("thread not found")
                            || source.message.contains("thread not loaded")
                            || source.message.contains("companion lifecycle requires")));
                if !expected_closed_child {
                    self.chat_widget.add_error_message(format!(
                        "Companion lifecycle notice could not be saved for {thread_id}: {error}"
                    ));
                }
            }
        }
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn record_companion_closed(
        &mut self,
        app_server: &AppServerSession,
        child: ThreadId,
    ) {
        let Some(state) = self.side_threads.get_mut(&child) else {
            return;
        };
        if !std::mem::take(&mut state.lifecycle_open) {
            return;
        }
        let parent = state.parent_thread_id;
        self.record_companion_lifecycle(
            app_server,
            [
                (
                    parent,
                    CompanionLifecycleEvent::Closed {
                        session_id: child.to_string(),
                    },
                ),
                (
                    child,
                    CompanionLifecycleEvent::Closed {
                        session_id: parent.to_string(),
                    },
                ),
            ],
        )
        .await;
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
        let response = self.accept_companion_message(app_server, params).await;
        self.app_event_tx.send(AppEvent::DynamicToolCallCompleted {
            request_id,
            response,
        });
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn accept_companion_message(
        &mut self,
        app_server: &AppServerSession,
        params: DynamicToolCallParams,
    ) -> DynamicToolCallResponse {
        let route = (|| {
            if self.reconnect.offline {
                return Err("TUI is reconnecting; message was not sent".to_string());
            }
            let source =
                ThreadId::from_string(&params.thread_id).map_err(|error| error.to_string())?;
            if !app_server.companion_messages_available(source) {
                return Err("This thread has no saved startup communication declaration; start a new conversation with enable_side_communication enabled".to_string());
            }
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
        async {
            let (recipient, message) = route?;
            // Admission completes before the TUI can close or replace this pair. This
            // awaits only the atomic start/steer acknowledgement, never a model reply.
            app_server
                .request_handle()
                .request_typed::<TurnStartResponse>(ClientRequest::TurnStart {
                    request_id: RequestId::String(uuid::Uuid::new_v4().to_string()),
                    params: TurnStartParams {
                        thread_id: recipient.to_string(),
                        assistant_message: Some(message),
                        assistant_message_source_thread_id: Some(params.thread_id.clone()),
                        ..TurnStartParams::default()
                    },
                })
                .await
                .map_err(|error| error.to_string())?;
            Ok::<_, String>(DynamicToolCallResponse {
                success: true,
                content_items: vec![DynamicToolCallOutputContentItem::InputText {
                    text: json!({"status": "accepted", "recipient": recipient.to_string()})
                        .to_string(),
                }],
            })
        }
        .await
        .unwrap_or_else(crate::dynamic_tools::failure_response)
    }
}

impl SideThreadState {
    pub(super) fn relationship(&self) -> CompanionRelationship {
        match (self.kind, self.mode) {
            (CompanionKind::Parallel, _) => CompanionRelationship::Parallel,
            (CompanionKind::Side, SideConversationMode::Chat) => CompanionRelationship::Chat,
            (CompanionKind::Side, SideConversationMode::Side) => CompanionRelationship::Side,
        }
    }
}
