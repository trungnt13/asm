//! Retains launch ownership across cancellation of a native turn.

use std::sync::Arc;
use std::time::Duration;

use codex_extension_api::ExternalAgentLaunch;
use codex_extension_api::ExternalAgentLaunchError;
use codex_extension_api::ExternalAgentLaunchMode;
use codex_extension_api::ExternalAgentRuntime;
use codex_protocol::error::CodexErr;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::ErrorEvent;
use codex_protocol::protocol::EventMsg;
use tokio::sync::Mutex;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::thread_manager::AgentTreeShutdownFailure;

use super::invalid;

const OPEN_TIMEOUT: Duration = Duration::from_secs(35);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
const STOP_UNCONFIRMED: &str = "External runtime stop could not be confirmed; execution may continue. The runtime is quarantined and requires human recovery.";

#[derive(Default)]
struct ExternalRuntimeSlot(Mutex<RuntimeState>);

#[derive(Default)]
enum RuntimeState {
    #[default]
    Empty,
    Stopped,
    Opening {
        _task: JoinHandle<()>,
        receiver:
            oneshot::Receiver<Result<Arc<dyn ExternalAgentRuntime>, ExternalAgentLaunchError>>,
        acknowledgement: Option<oneshot::Sender<()>>,
        launch: ExternalAgentLaunch,
        backend_id: String,
        unsettled: bool,
    },
    Open(OpenedRuntime),
    Unknown,
}

struct OpenedRuntime {
    runtime: Arc<dyn ExternalAgentRuntime>,
    launch: ExternalAgentLaunch,
    backend_id: String,
    unsettled: bool,
}

#[tracing::instrument(level = "trace", skip_all)]
#[expect(
    clippy::await_holding_invalid_type,
    reason = "retain one child runtime owner across bounded launch and cancellation handoffs"
)]
pub(super) async fn open(
    session: &Session,
    turn: &TurnContext,
    launch: ExternalAgentLaunch,
    cancellation: &CancellationToken,
) -> Result<Arc<dyn ExternalAgentRuntime>, CodexErr> {
    let slot = session
        .services
        .thread_extension_data
        .get_or_init(ExternalRuntimeSlot::default);
    let mut state = slot.0.lock().await;
    let descriptor = turn
        .config
        .external_agent
        .as_ref()
        .ok_or_else(|| invalid("external descriptor is missing"))?;
    if matches!(*state, RuntimeState::Empty | RuntimeState::Stopped) {
        let backend = session
            .services
            .extensions
            .external_agent_backend(&descriptor.backend_id)
            .ok_or_else(|| invalid("external executor backend is not registered"))?;
        let mut request = launch.clone();
        if matches!(*state, RuntimeState::Stopped) {
            request.mode = ExternalAgentLaunchMode::Resume;
        }
        let runtime_launch = request.clone();
        let tree = session.services.local_agent_runtime.clone();
        let guard = tree
            .admit_start()?
            .into_teardown_guard("external_launch", Some(session.thread_id));
        let (sender, receiver) = oneshot::channel();
        let (acknowledgement, admitted) = oneshot::channel();
        let task = tokio::spawn(async move {
            let runtime = match backend.open(request).await {
                Ok(runtime) => runtime,
                Err(error) => {
                    if matches!(&error, ExternalAgentLaunchError::Unsettled(_)) {
                        guard.record_shutdown_failure("open_runtime", "external_launch_failed");
                    }
                    let _ = sender.send(Err(error));
                    guard.complete();
                    return;
                }
            };
            let abandoned = if tree.shutdown.is_cancelled() {
                let _ = sender.send(Err(ExternalAgentLaunchError::Unsettled(
                    "external launch finished after tree shutdown".to_string(),
                )));
                true
            } else if sender.send(Ok(Arc::clone(&runtime))).is_err() {
                true
            } else {
                admitted.await.is_err()
            };
            if abandoned
                && !matches!(
                    tokio::time::timeout(STOP_TIMEOUT, runtime.shutdown()).await,
                    Ok(Ok(()))
                )
            {
                guard.record_shutdown_failure("stop_runtime", "external_shutdown_failed");
            }
            guard.complete();
        });
        *state = RuntimeState::Opening {
            _task: task,
            receiver,
            acknowledgement: Some(acknowledgement),
            launch: runtime_launch,
            backend_id: descriptor.backend_id.clone(),
            unsettled: false,
        };
    }
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Err(CodexErr::TurnAborted),
        result = resolve_open(&mut state) => {
            if let Err(error) = result {
                if !matches!(*state, RuntimeState::Empty | RuntimeState::Stopped) {
                    record_failure(session);
                }
                return Err(invalid(error));
            }
        }
    }
    match &*state {
        RuntimeState::Open(existing)
            if !existing.unsettled
                && existing.backend_id == descriptor.backend_id
                && existing.launch.cwd == launch.cwd
                && existing.launch.model == launch.model
                && existing.launch.runtime_session_id == launch.runtime_session_id
                && existing.launch.instructions == launch.instructions
                && existing.launch.env == launch.env =>
        {
            Ok(Arc::clone(&existing.runtime))
        }
        RuntimeState::Open(_)
        | RuntimeState::Empty
        | RuntimeState::Stopped
        | RuntimeState::Opening { .. }
        | RuntimeState::Unknown => {
            drop(state);
            shutdown(session).await;
            Err(invalid(
                "external runtime settings changed or execution is unsettled; reopen the child",
            ))
        }
    }
}

#[tracing::instrument(level = "trace", skip_all)]
async fn resolve_open(state: &mut RuntimeState) -> Result<(), String> {
    let RuntimeState::Opening {
        receiver,
        acknowledgement,
        launch,
        backend_id,
        unsettled,
        ..
    } = state
    else {
        return if matches!(state, RuntimeState::Unknown) {
            Err("external launch ownership is unknown".to_string())
        } else {
            Ok(())
        };
    };
    let result = match tokio::time::timeout(OPEN_TIMEOUT, receiver).await {
        Ok(result) => result,
        Err(_) => {
            *unsettled = true;
            return Err("external launch timed out; execution ownership retained".to_string());
        }
    };
    match result {
        Ok(Ok(runtime)) => {
            let acknowledgement = acknowledgement.take();
            *state = RuntimeState::Open(OpenedRuntime {
                runtime,
                launch: launch.clone(),
                backend_id: backend_id.clone(),
                unsettled: *unsettled,
            });
            if let Some(acknowledgement) = acknowledgement {
                let _ = acknowledgement.send(());
            }
            Ok(())
        }
        Ok(Err(ExternalAgentLaunchError::Rejected(error))) => {
            *state = match launch.mode {
                ExternalAgentLaunchMode::New => RuntimeState::Empty,
                ExternalAgentLaunchMode::Resume => RuntimeState::Stopped,
            };
            Err(error)
        }
        Ok(Err(ExternalAgentLaunchError::Unsettled(error))) => {
            *state = RuntimeState::Unknown;
            Err(error)
        }
        Err(error) => {
            *state = RuntimeState::Unknown;
            Err(format!("external launch handoff failed: {error}"))
        }
    }
}

#[tracing::instrument(level = "trace", skip_all)]
pub(crate) async fn interrupt(session: &Session, turn: &TurnContext) -> Result<(), CodexErr> {
    let Some(slot) = session
        .services
        .thread_extension_data
        .get::<ExternalRuntimeSlot>()
    else {
        return Ok(());
    };
    let runtime = {
        let state = slot.0.lock().await;
        match &*state {
            RuntimeState::Open(opened) => Some(Arc::clone(&opened.runtime)),
            RuntimeState::Empty | RuntimeState::Stopped => return Ok(()),
            RuntimeState::Opening { .. } | RuntimeState::Unknown => None,
        }
    };
    if let Some(runtime) = runtime
        && matches!(
            tokio::time::timeout(STOP_TIMEOUT, runtime.interrupt(&turn.sub_id)).await,
            Ok(Ok(()))
        )
    {
        return Ok(());
    }
    shutdown(session).await;
    if !matches!(
        *slot.0.lock().await,
        RuntimeState::Empty | RuntimeState::Stopped
    ) {
        session
            .send_event(
                turn,
                EventMsg::Error(ErrorEvent {
                    message: STOP_UNCONFIRMED.to_string(),
                    codex_error_info: Some(CodexErrorInfo::Other),
                    misalignment: None,
                }),
            )
            .await;
        return Err(CodexErr::Fatal(STOP_UNCONFIRMED.to_string()));
    }
    Ok(())
}

#[tracing::instrument(level = "trace", skip_all)]
#[expect(
    clippy::await_holding_invalid_type,
    reason = "serialize bounded settlement without releasing or losing the child runtime owner"
)]
pub(crate) async fn shutdown(session: &Session) {
    let Some(slot) = session
        .services
        .thread_extension_data
        .get::<ExternalRuntimeSlot>()
    else {
        return;
    };
    let mut state = slot.0.lock().await;
    if matches!(*state, RuntimeState::Empty | RuntimeState::Stopped) {
        return;
    }
    if resolve_open(&mut state).await.is_err() {
        if !matches!(*state, RuntimeState::Empty | RuntimeState::Stopped) {
            record_failure(session);
        }
        return;
    }
    if let RuntimeState::Open(opened) = &mut *state
        && !matches!(
            tokio::time::timeout(STOP_TIMEOUT, opened.runtime.shutdown()).await,
            Ok(Ok(()))
        )
    {
        opened.unsettled = true;
        record_failure(session);
        return;
    }
    *state = RuntimeState::Stopped;
}

fn record_failure(session: &Session) {
    session
        .services
        .local_agent_runtime
        .record_shutdown_failure(AgentTreeShutdownFailure::operation_failed(
            "external_executor",
            "stop_runtime",
            Some(session.thread_id),
            "external_shutdown_failed",
        ));
}
