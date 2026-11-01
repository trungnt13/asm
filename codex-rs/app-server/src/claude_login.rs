use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItemPayload;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::TurnSteerResponse;
use codex_app_server_protocol::UserInput;
use codex_core::CodexThread;
use codex_protocol::ThreadId;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::claude_login_process::LoginProcess;
use crate::error_code::invalid_request;
use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::ConnectionRequestId;
use crate::outgoing_message::OutgoingMessageSender;

#[derive(Default)]
pub(crate) struct ClaudeLogin {
    sessions: Arc<Mutex<HashMap<ThreadId, Arc<Mutex<LoginSession>>>>>,
}

struct LoginSession {
    owner: ConnectionId,
    generation: uuid::Uuid,
    model: String,
    selected_model: String,
    cwd: PathBuf,
    process: Option<LoginProcess>,
}

#[derive(Clone, Copy)]
pub(crate) enum LoginReply<'a> {
    Start { model: Option<&'a str> },
    Steer,
}

impl ClaudeLogin {
    #[tracing::instrument(skip_all)]
    pub(crate) async fn intercept(
        &self,
        thread_id: ThreadId,
        thread: &CodexThread,
        input: &[UserInput],
        request_id: &ConnectionRequestId,
        reply: LoginReply<'_>,
        outgoing: &OutgoingMessageSender,
        disconnect: CancellationToken,
    ) -> Result<bool, JSONRPCErrorError> {
        let text = match input {
            [UserInput::Text { text, .. }] => Some(text.trim()),
            _ => None,
        };
        let start = text.is_some_and(|text| text.eq_ignore_ascii_case("claude login"));
        let code = text.is_some_and(is_authorization_code);
        let pending_entry = self.sessions.lock().await.get(&thread_id).cloned();
        let pending = pending_entry.is_some();
        let config = thread.config_snapshot().await;
        let requested_model = match reply {
            LoginReply::Start { model } => model,
            LoginReply::Steer => None,
        };
        let selected_model = match requested_model {
            Some(model) => model.to_owned(),
            None => match &pending_entry {
                Some(entry) => entry.lock().await.selected_model.clone(),
                None => config.model.clone(),
            },
        };
        let claude = selected_model.starts_with("claude-code/");
        if !pending
            && !(claude && (start || code || thread.claude_login_required()))
            && !(config.model.starts_with("claude-code/") && code)
        {
            return Ok(false);
        }
        if !claude {
            if let Some(entry) = pending_entry {
                let mut session = entry.lock().await;
                if session.owner != request_id.connection_id && session.process.is_some() {
                    return Err(invalid_request(
                        "Claude login belongs to another connection.",
                    ));
                }
                session.process = None;
                self.sessions.lock().await.remove(&thread_id);
                return Err(invalid_request(
                    "Claude login cancelled because the selected runtime changed. Send a new message to continue.",
                ));
            }
            return Err(invalid_request(
                "Claude login requires a Claude native-runtime chat.",
            ));
        }
        if matches!(
            config.session_source,
            codex_protocol::protocol::SessionSource::SubAgent(_)
        ) {
            return Err(invalid_request("Sign in from a main Claude chat."));
        }
        let owner = request_id.connection_id;
        let entry = self
            .sessions
            .lock()
            .await
            .entry(thread_id)
            .or_insert_with(|| {
                Arc::new(Mutex::new(LoginSession {
                    owner,
                    generation: uuid::Uuid::new_v4(),
                    model: config.model.clone(),
                    selected_model: selected_model.clone(),
                    cwd: config.cwd().as_path().to_path_buf(),
                    process: None,
                }))
            })
            .clone();
        let mut session = entry.lock().await;
        if session.owner != owner && session.process.is_some() {
            return Err(invalid_request(
                "Claude login belongs to another connection.",
            ));
        }
        if pending
            && session.process.is_none()
            && !start
            && !code
            && !thread.claude_login_required()
        {
            self.sessions.lock().await.remove(&thread_id);
            return Ok(false);
        }
        if !start
            && (session.model != config.model
                || session.selected_model != selected_model
                || session.cwd != config.cwd().as_path())
        {
            session.process = None;
            return Err(invalid_request(
                "Claude login configuration changed. Send `claude login` to restart.",
            ));
        }
        if [
            "CLAUDE_CODE_OAUTH_TOKEN",
            "CLAUDE_CODE_OAUTH_REFRESH_TOKEN",
            "CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR",
        ]
        .into_iter()
        .any(|name| std::env::var_os(name).is_some())
        {
            session.process = None;
            return Err(invalid_request(
                "Claude login is unavailable while native OAuth environment overrides are set. Remove those overrides from the backend environment and retry.",
            ));
        }
        if !thread.stop_claude_for_login().await {
            return Err(invalid_request(
                "Wait for native work to finish before Claude login.",
            ));
        }
        if selected_model != config.model {
            thread
                .update_thread_settings(codex_protocol::protocol::ThreadSettingsOverrides {
                    model: Some(selected_model.clone()),
                    ..Default::default()
                })
                .await
                .map_err(|_| {
                    invalid_request("Claude login model selection could not be applied.")
                })?;
            session.model = selected_model.clone();
        }
        let started_at_ms: i64 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or_default();
        let mut turn = Turn {
            id: uuid::Uuid::now_v7().to_string(),
            root_turn_id: None,
            token_usage: None,
            items: Vec::new(),
            items_view: Default::default(),
            status: TurnStatus::InProgress,
            error: None,
            started_at: Some(started_at_ms / 1000),
            completed_at: None,
            duration_ms: None,
        };
        match reply {
            LoginReply::Start { .. } => {
                outgoing
                    .send_response(request_id.clone(), TurnStartResponse { turn: turn.clone() })
                    .await
            }
            LoginReply::Steer => {
                outgoing
                    .send_response(
                        request_id.clone(),
                        TurnSteerResponse {
                            turn_id: turn.id.clone(),
                        },
                    )
                    .await
            }
        }
        outgoing
            .send_server_notification_to_connections(
                &[owner],
                ServerNotification::TurnStarted(TurnStartedNotification {
                    thread_id: thread_id.to_string(),
                    turn: turn.clone(),
                }),
            )
            .await;
        let cancel = text.is_some_and(|text| text.eq_ignore_ascii_case("cancel"));
        let message = if cancel {
            session.process = None;
            if !thread.claude_login_required() {
                self.sessions.lock().await.remove(&thread_id);
            }
            "Claude login cancelled. Send `claude login` to sign in again.".to_owned()
        } else if start || (!pending && !code) {
            session.process = None;
            session.owner = owner;
            session.model = selected_model.clone();
            session.selected_model = selected_model;
            session.cwd = config.cwd().as_path().to_path_buf();
            session.generation = uuid::Uuid::new_v4();
            let result = tokio::select! {
                result = LoginProcess::start(config.cwd().as_path()) => result,
                _ = disconnect.cancelled() => Err(()),
            };
            match result {
                Ok((process, url)) => {
                    session.process = Some(process);
                    let generation = session.generation;
                    let entry = Arc::clone(&entry);
                    tokio::spawn(async move {
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_secs(300)) => {}
                            _ = disconnect.cancelled() => {}
                        }
                        let mut session = entry.lock().await;
                        if session.generation == generation {
                            session.process = None;
                        }
                    });
                    format!(
                        "Sign in to your Claude subscription: [Open Claude sign-in]({url}).\nCopy the one-time authorization code from the browser and send it as your next message. Do not send an API key or access token. Send `cancel` to cancel. The failed prompt will not be replayed."
                    )
                }
                Err(()) => {
                    "Claude subscription login could not start. Send `claude login` to try again."
                        .to_owned()
                }
            }
        } else if session.process.is_none() {
            "No active Claude login. Send `claude login` to obtain a new sign-in link. Nothing from this exchange is sent to a model.".to_owned()
        } else if !code {
            "Login is pending. Send only the one-time authorization code, `claude login` to restart, or `cancel`. Nothing from this login exchange is sent to a model.".to_owned()
        } else if let Some(process) = session.process.take().filter(|process| !process.expired()) {
            let result = tokio::select! {
                result = process.submit(text.unwrap_or_default()) => result,
                _ = disconnect.cancelled() => Err(()),
            };
            match result {
                Ok(()) => {
                    thread.finish_claude_login();
                    self.sessions.lock().await.remove(&thread_id);
                    "Claude account sign-in completed. The subscription runtime remains selected. Send a new message to continue this chat; the failed prompt was not replayed.".to_owned()
                }
                Err(()) => "Claude subscription login did not complete. Send `claude login` to restart; do not resend this code.".to_owned(),
            }
        } else {
            "No active Claude login. Send `claude login` to obtain a new sign-in link. This code was not sent to a model.".to_owned()
        };
        drop(session);
        let completed_at_ms: i64 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or_default();
        let item = ThreadItemPayload::AgentMessage {
            id: uuid::Uuid::now_v7().to_string(),
            text: message,
            phase: None,
            memory_citation: None,
            delivery: None,
            questions: None,
        };
        outgoing
            .send_server_notification_to_connections(
                &[owner],
                ServerNotification::ItemStarted(ItemStartedNotification::new(
                    thread_id.to_string(),
                    turn.id.clone(),
                    item.clone(),
                    started_at_ms,
                )),
            )
            .await;
        let completed = ItemCompletedNotification::new(
            thread_id.to_string(),
            turn.id.clone(),
            item,
            Some(started_at_ms),
            completed_at_ms,
        );
        turn.items.push(completed.item.clone());
        outgoing
            .send_server_notification_to_connections(
                &[owner],
                ServerNotification::ItemCompleted(completed),
            )
            .await;
        turn.status = TurnStatus::Completed;
        turn.completed_at = Some(completed_at_ms / 1000);
        turn.duration_ms = Some(completed_at_ms.saturating_sub(started_at_ms));
        outgoing
            .send_server_notification_to_connections(
                &[owner],
                ServerNotification::TurnCompleted(TurnCompletedNotification {
                    thread_id: thread_id.to_string(),
                    turn,
                }),
            )
            .await;
        Ok(true)
    }
}

fn is_authorization_code(text: &str) -> bool {
    text.len() <= 4096
        && text.split_once('#').is_some_and(|(code, state)| {
            [code, state].into_iter().all(|part| {
                part.len() >= 16
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            })
        })
}
