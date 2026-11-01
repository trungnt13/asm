use std::collections::HashMap;
use std::path::Path;
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

mod claude;
mod cursor;
mod process;
mod provider;

use self::process::LoginProcess;
use self::provider::LoginProvider;
use crate::error_code::invalid_request;
use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::ConnectionRequestId;
use crate::outgoing_message::OutgoingMessageSender;

#[derive(Default)]
pub(crate) struct NativeLogin {
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

impl NativeLogin {
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
        let requested_provider = text.and_then(LoginProvider::from_command);
        let start = requested_provider.is_some();
        let authorization_code = text.is_some_and(claude::is_authorization_code);
        let pending_entry = self.sessions.lock().await.get(&thread_id).cloned();
        let guard_path = thread
            .config()
            .await
            .codex_home
            .join("claude-runtime")
            .join(format!("{thread_id}.login-pending"));
        let durable_pending = match tokio::fs::symlink_metadata(&guard_path).await {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(_) => return Err(login_guard_error()),
        };
        let pending = pending_entry.is_some() || durable_pending;
        let cancel = text.is_some_and(|text| text.eq_ignore_ascii_case("cancel"));
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
        let provider = LoginProvider::from_model(&selected_model);
        let code = match provider {
            Some(LoginProvider::Cursor) => {
                pending && text.is_some_and(|text| LoginProvider::Cursor.accepts_reply(text))
            }
            Some(LoginProvider::Claude) | None => authorization_code,
        };
        if !pending
            && !start
            && !(provider.is_some() && (start || code || thread.native_login_required()))
            && !(config.model.starts_with("claude-code/") && authorization_code)
        {
            return Ok(false);
        }
        let Some(provider) = provider else {
            if pending && !cancel {
                return Err(invalid_request(
                    "No active native login. Send `claude login` or `cursor login` with the matching native runtime to restart, or `cancel`. Nothing from this exchange is sent to a model.",
                ));
            }
            if let Some(entry) = pending_entry {
                let mut session = entry.lock().await;
                if session.owner != request_id.connection_id && session.process.is_some() {
                    return Err(invalid_request(
                        "Native login belongs to another connection.",
                    ));
                }
                clear_login_guard(&guard_path).await?;
                session.process = None;
                self.sessions.lock().await.remove(&thread_id);
                return Err(invalid_request(
                    "Native login cancelled because the selected runtime changed. Send a new message to continue.",
                ));
            }
            if durable_pending {
                clear_login_guard(&guard_path).await?;
                return Err(invalid_request(
                    "Native login cancelled. Send a new message to continue with the selected runtime.",
                ));
            }
            return Err(invalid_request(
                "Native login requires a Claude or Cursor native-runtime chat.",
            ));
        };
        if requested_provider.is_some_and(|requested| requested != provider) {
            return Err(invalid_request(
                "Login command must match the selected native runtime: `cursor login` or `claude login`. Nothing from this exchange was sent to a model.",
            ));
        }
        if matches!(
            config.session_source,
            codex_protocol::protocol::SessionSource::SubAgent(_)
        ) {
            return Err(invalid_request("Sign in from a main native-runtime chat."));
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
                "Native login belongs to another connection.",
            ));
        }
        persist_login_guard(&guard_path).await?;
        if !start
            && !cancel
            && (session.model != config.model
                || session.selected_model != selected_model
                || session.cwd != config.cwd().as_path())
        {
            session.process = None;
            return Err(invalid_request(
                "Native login configuration changed. Send `claude login` or `cursor login` to restart.",
            ));
        }
        if !cancel && let Some(message) = provider.environment_error() {
            session.process = None;
            return Err(invalid_request(message));
        }
        if !thread.stop_native_for_login().await {
            return Err(invalid_request(
                "Wait for native work to finish before login.",
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
                    invalid_request("Native login model selection could not be applied.")
                })?;
            session.model = selected_model.clone();
        }
        if cancel {
            clear_login_guard(&guard_path).await?;
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
        let message = if cancel {
            session.process = None;
            if !thread.native_login_required() {
                self.sessions.lock().await.remove(&thread_id);
            }
            "Native login cancelled. Send `claude login` or `cursor login` to sign in again."
                .to_owned()
        } else if start || (!pending && !code) {
            session.process = None;
            session.owner = owner;
            session.model = selected_model.clone();
            session.selected_model = selected_model;
            session.cwd = config.cwd().as_path().to_path_buf();
            session.generation = uuid::Uuid::new_v4();
            let result = tokio::select! {
                result = LoginProcess::start(config.cwd().as_path(), provider) => result,
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
                    provider.instructions(&url)
                }
                Err(()) => {
                    "Native subscription login could not start. Send `claude login` or `cursor login` to try again."
                        .to_owned()
                }
            }
        } else if session.process.is_none() {
            "No active native login. Send `claude login` or `cursor login` to obtain a new sign-in link. Nothing from this exchange is sent to a model.".to_owned()
        } else if !code {
            "Login is pending. Send only `done` for Cursor or the one-time Claude authorization code, `claude login` or `cursor login` to restart, or `cancel`. Nothing from this login exchange is sent to a model.".to_owned()
        } else if let Some(process) = session.process.take().filter(|process| !process.expired()) {
            let result = tokio::select! {
                result = process.submit(text.unwrap_or_default()) => result,
                _ = disconnect.cancelled() => Err(()),
            };
            match result {
                Ok(()) => match clear_login_guard(&guard_path).await {
                    Ok(()) => {
                        thread.finish_native_login();
                        self.sessions.lock().await.remove(&thread_id);
                        "Native account sign-in completed. The subscription runtime remains selected. Send a new message to continue this chat; the failed prompt was not replayed.".to_owned()
                    }
                    Err(_) => login_guard_error().message,
                },
                Err(()) => "Native subscription login did not complete. Send `claude login` or `cursor login` to restart; do not resend this code.".to_owned(),
            }
        } else {
            "No active native login. Send `claude login` or `cursor login` to obtain a new sign-in link. This code was not sent to a model.".to_owned()
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

fn login_guard_error() -> JSONRPCErrorError {
    invalid_request(
        "Native login safety state is unavailable. Nothing from this exchange was sent to a model. Retry after local storage is available.",
    )
}

#[tracing::instrument(skip_all)]
async fn persist_login_guard(path: &Path) -> Result<(), JSONRPCErrorError> {
    let result = async {
        let directory = path.parent().expect("login marker has a parent");
        tokio::fs::create_dir_all(directory).await?;
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create(true).truncate(false);
        #[cfg(unix)]
        options.mode(/*mode*/ 0o600);
        options.open(path).await?.sync_all().await?;
        #[cfg(unix)]
        tokio::fs::File::open(directory).await?.sync_all().await?;
        std::io::Result::Ok(())
    }
    .await;
    result.map_err(|_| login_guard_error())
}

#[tracing::instrument(skip_all)]
async fn clear_login_guard(path: &Path) -> Result<(), JSONRPCErrorError> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(login_guard_error()),
    }
}
