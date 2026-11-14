use super::RuntimeLease;
use super::RuntimeProcess;
use super::delivery;
use super::events;
use crate::session::TurnInput;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::state::TaskKind;
use crate::tasks::SessionTask;
use crate::tasks::SessionTaskResult;
use anyhow::Context;
use codex_external_runtime::ClaudeContinuation;
use codex_protocol::error::CodexErr;
use codex_protocol::openai_models::NativeRuntime;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub(super) fn listen(session: &Arc<Session>, mut receiver: mpsc::Receiver<ClaudeContinuation>) {
    let weak = Arc::downgrade(session);
    let listener = tokio::spawn(async move {
        while let Some(continuation) = receiver.recv().await {
            let Some(session) = weak.upgrade() else {
                break;
            };
            let Ok(admission) = session
                .external_runtime
                .continuation_admission
                .acquire()
                .await
            else {
                break;
            };
            let Some(_admission) = session.services.extensions.admit_turn_start() else {
                break;
            };
            let reservation = loop {
                let finished = session.external_runtime.turn_finished.notified();
                tokio::pin!(finished);
                finished.as_mut().enable();
                if continuation.superseded.is_cancelled() {
                    break None;
                }
                {
                    let mut active = session.active_turn.lock().await;
                    if active.is_none() {
                        let turn = active.get_or_insert_with(crate::state::ActiveTurn::default);
                        break Some(Arc::clone(&turn.turn_state));
                    }
                }
                tokio::select! {
                    _ = &mut finished => {},
                    _ = continuation.superseded.cancelled() => {},
                }
            };
            let Some(reservation) = reservation else {
                continue;
            };
            let turn = session
                .new_turn_with_default_settings(
                    uuid::Uuid::new_v4().to_string(),
                    Default::default(),
                )
                .await;
            turn.turn_metadata_state
                .set_turn_trigger("native_background".to_owned());
            session
                .start_task_with_reservation(
                    turn,
                    Vec::new(),
                    NativeContinuationTask(Mutex::new(Some(continuation))),
                    Some(Arc::clone(&reservation)),
                )
                .await;
            session.clear_reserved_idle_turn(&reservation).await;
            drop(admission);
        }
    });
    *session
        .external_runtime
        .continuation_listener
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(listener);
}

struct NativeContinuationTask(Mutex<Option<ClaudeContinuation>>);

impl SessionTask for NativeContinuationTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Regular
    }
    fn span_name(&self) -> &'static str {
        "native_continuation"
    }

    #[tracing::instrument(skip_all)]
    async fn run(
        self: Arc<Self>,
        session: Arc<Session>,
        ctx: Arc<TurnContext>,
        _input: Vec<TurnInput>,
        cancellation_token: CancellationToken,
    ) -> SessionTaskResult {
        let continuation = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .ok_or_else(|| {
                CodexErr::InvalidRequest("native continuation already consumed".to_owned())
            })?;
        super::run_turn_start(
            session,
            ctx,
            Vec::new(),
            cancellation_token,
            Some(continuation),
        )
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn abort(&self, session: Arc<Session>, _ctx: Arc<TurnContext>) {
        super::abort(&session).await;
    }
}

#[tracing::instrument(skip_all)]
pub(super) async fn run(
    session: &Arc<Session>,
    turn: &Arc<TurnContext>,
    mut continuation: ClaudeContinuation,
    cancellation: &CancellationToken,
) -> anyhow::Result<Option<String>> {
    let mut lease = RuntimeLease::take(&session.external_runtime)?;
    let runtime = lease
        .runtime
        .as_mut()
        .context("native continuation lost its runtime")?;
    runtime.foreground_idle = false;
    runtime.checkpoint.in_flight = true;
    runtime.checkpoint.fork_point = None;
    runtime
        .linkage
        .save(&runtime.checkpoint, &runtime.checkpoint_path)?;
    if let Some(bridge) = &runtime.bridge {
        let step = session
            .capture_speculative_step_context(Arc::clone(turn), cancellation)
            .await?;
        bridge
            .set_turn(Arc::clone(session), step, cancellation.clone())
            .await?;
    }
    continuation
        .admitted
        .send(())
        .map_err(|_| anyhow::anyhow!("native continuation owner stopped"))?;
    let mut stream = events::MessageStream::default();
    stream.activity.server = NativeRuntime::ClaudeCode.id();
    stream.activity.messaging = runtime.checkpoint.messaging;
    let mut mail_receipts =
        delivery::CursorMailReceipts::new(runtime.checkpoint.session_id.clone());
    let mut last_message = None;
    let mut cancelled = false;
    let mut submitted = 0;
    let mut capabilities = match &runtime.runtime {
        RuntimeProcess::Claude(process) => process.subscribe_mid_turn_delivery(),
        RuntimeProcess::Cursor(_) => anyhow::bail!("Claude continuation attached to Cursor"),
    };
    let turn_state = session
        .input_queue
        .turn_state_for_sub_id(&session.active_turn, &turn.sub_id)
        .await;
    let (mut activity, mut pending_activity) = session
        .input_queue
        .subscribe_activity(turn_state.as_deref())
        .await;

    let result = loop {
        tokio::select! {
            biased;
            _ = cancellation.cancelled(), if !cancelled => {
                cancelled = true;
                continuation.cancellation.cancel();
            },
            result = &mut continuation.outcome => break result.context("native continuation owner stopped")?,
            _ = async {
                if pending_activity.is_some() { return; }
                let _ = activity.changed().await;
            }, if continuation.steering.as_ref().is_some_and(codex_external_runtime::ClaudeSteering::is_ready)
                && *capabilities.borrow() && !cancelled => {
                pending_activity = None;
                if let Some(steering) = continuation.steering.as_ref()
                    && submit_steering(session, turn, &mut runtime.checkpoint, &runtime.linkage,
                    &runtime.checkpoint_path, runtime.page_context, SteeringSubmission::Continuation(steering)).await? {
                    submitted += 1;
                }
            },
            changed = capabilities.changed(), if continuation.steering.is_some() => {
                if changed.is_err() { continuation.steering = None; }
            },

            frame = continuation.events.recv() => {
                if let Some(frame) = frame
                    && let Some(text) = super::record_frame(NativeRuntime::ClaudeCode, session, turn,
                        &frame, &mut stream, &mut mail_receipts).await? { last_message = Some(text); }
            }
        }
    };
    while let Ok(frame) = continuation.events.try_recv() {
        if let Some(text) = super::record_frame(
            NativeRuntime::ClaudeCode,
            session,
            turn,
            &frame,
            &mut stream,
            &mut mail_receipts,
        )
        .await?
        {
            last_message = Some(text);
        }
    }
    stream.activity.close(session, turn).await?;
    if let Some(bridge) = &runtime.bridge {
        bridge.clear_turn().await?;
    }
    let outcome = result?;
    anyhow::ensure!(
        outcome.submitted_steering_count == submitted,
        "Claude did not submit all recorded continuation steering messages; explicit recovery is required"
    );
    if outcome.result.is_some() || outcome.interrupted || outcome.failure.is_some() {
        super::complete_response(
            session,
            turn,
            runtime,
            NativeRuntime::ClaudeCode,
            stream,
            outcome,
            Some(continuation.generation),
        )
        .await?;
    } else {
        session.flush_rollout().await?;
        super::save_reconciled_checkpoint(
            runtime,
            Some(continuation.generation),
            /*successful*/ true,
        )?;
    }
    drop(lease);
    if cancellation.is_cancelled() {
        return Err(super::ConfirmedInterrupt.into());
    }
    let (mut pending, _) = session
        .input_queue
        .get_pending_input(&session.active_turn)
        .await;
    if !pending.is_empty() {
        return super::run_conversation(session, turn, &mut pending, cancellation).await;
    }
    Ok(last_message)
}

pub(super) enum SteeringSubmission<'a> {
    Foreground(&'a mpsc::Sender<codex_external_runtime::Prompt>),
    Continuation(&'a codex_external_runtime::ClaudeSteering),
}

#[tracing::instrument(skip_all)]
pub(super) async fn submit_steering(
    session: &Arc<Session>,
    turn: &Arc<TurnContext>,
    checkpoint: &mut super::Checkpoint,
    linkage: &super::Linkage,
    path: &std::path::Path,
    page_context: codex_config::PageContextPolicy,
    steering: SteeringSubmission<'_>,
) -> anyhow::Result<bool> {
    let settings = &turn.initial_settings;
    let reserved = match &steering {
        SteeringSubmission::Continuation(steering) => {
            let Some(reserved) = steering.reserve() else {
                return Ok(false);
            };
            Some(reserved)
        }
        SteeringSubmission::Foreground(_) => None,
    };
    session
        .input_queue
        .accept_mailbox_delivery_for_current_turn(&session.active_turn, &turn.sub_id)
        .await;
    let (mut pending, _) = session
        .input_queue
        .get_pending_input(&session.active_turn)
        .await;
    if page_context != codex_config::PageContextPolicy::Inherit {
        let history = session.clone_history().await;
        super::input::filter_page_context(&mut pending, page_context, history.raw_items());
    }
    match super::input::prepare_input(&mut pending) {
        Ok(0) => {}
        Ok(_) => {
            session
                .send_event(
                    turn,
                    codex_protocol::protocol::EventMsg::Warning(
                        codex_protocol::protocol::WarningEvent {
                            message:
                                "Encrypted ASM input was ignored; the user prompt is preserved."
                                    .to_owned(),
                        },
                    ),
                )
                .await
        }
        Err(error) => {
            session
                .send_event(
                    turn,
                    codex_protocol::protocol::EventMsg::Warning(
                        codex_protocol::protocol::WarningEvent {
                            message: format!("Claude queued input was rejected: {error}"),
                        },
                    ),
                )
                .await;
            return Ok(false);
        }
    }
    if pending.is_empty() {
        return Ok(false);
    }
    let history = session.clone_history().await;
    let messages = super::input_messages(history.raw_items())?;
    drop(history);
    if let Err(error) = super::validate_pending_prompt(
        session,
        &messages[checkpoint.input_count..],
        &pending,
        /*boundary*/ None,
        codex_external_runtime::validate_claude_prompt,
    ) {
        session
            .send_event(
                turn,
                codex_protocol::protocol::EventMsg::Warning(
                    codex_protocol::protocol::WarningEvent {
                        message: format!("Claude queued input was rejected: {error}"),
                    },
                ),
            )
            .await;
        return Ok(false);
    }
    drop(messages);
    for item in pending {
        super::record_pending_input(
            session,
            turn,
            &settings.model_info,
            item,
            Vec::new(),
            codex_thread_store::PersistContext::TurnStart,
        )
        .await;
    }
    let messages = super::input_messages(session.clone_history().await.raw_items())?;
    let prompt = super::prompt_content(&messages[checkpoint.input_count..])?;
    checkpoint.input_count = messages.len();
    checkpoint.input_prefix = super::input_prefix(&messages)?;
    drop(messages);
    session.flush_rollout().await?;
    linkage.save(checkpoint, path)?;
    match reserved {
        Some(reserved) => {
            reserved.send(prompt).map_err(|_| {
                anyhow::anyhow!("Claude continuation stopped before reserved steering delivery")
            })?;
        }
        None => match steering {
            SteeringSubmission::Foreground(steering) => steering
                .try_send(prompt)
                .context("Claude steering channel was not ready; explicit recovery is required")?,
            SteeringSubmission::Continuation(_) => {
                unreachable!("continuation delivery is reserved")
            }
        },
    }
    Ok(true)
}
