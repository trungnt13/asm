//! Thread-scoped lifecycle for the official CLI's native conversation and tools.
mod activity;
mod bridge;
mod completion;
mod continuation;
use completion::complete_response;
use completion::save_reconciled_checkpoint;
mod cursor;
mod deletion;
mod delivery;
mod events;
mod fork;
mod input;
mod messages;
mod state;
use bridge::ExternalMessageBridge;
use cursor::record_cursor_frame;
pub(crate) use deletion::delete_thread_data;
pub(crate) use deletion::ensure_no_runtime_owner;
use events::record_assistant_frame;
use fork::TEMPORARY_FORK_BOUNDARY;
use fork::TEMPORARY_FORK_DISALLOWED_TOOLS;
pub(crate) use fork::prepare_temporary_fork;
pub(crate) use input::context_item;
use messages::input_messages;
use messages::input_prefix;
use messages::prompt_content;
use messages::validate_pending_prompt;
use state::Checkpoint;
use state::ExternalSession;
use state::Linkage;
use state::RuntimeLease;
use state::RuntimeProcess;
pub(crate) use state::RuntimeSlot;
use state::SessionStart;

use serde_json::Value;
use std::io::Write;
use std::sync::Arc;

use anyhow::Context;
use anyhow::bail;
use codex_async_utils::OrCancelExt;
use codex_history::RolloutItem;
use codex_protocol::error::CodexErr;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::NativeRuntime;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::MultiAgentVersion;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_protocol::protocol::WarningEvent;
use tokio_util::sync::CancellationToken;

use crate::hook_runtime::record_pending_input;
use crate::session::TurnInput;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::state::TaskKind;
use crate::tasks::SessionTaskResult;

pub(crate) fn is_native_model(model: &str) -> bool {
    codex_protocol::openai_models::is_native_runtime_model(model)
}

#[tracing::instrument(skip_all)]
pub(crate) async fn abort(session: &Session) {
    let preserve = match RuntimeLease::take(&session.external_runtime) {
        Ok(lease) => lease
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.foreground_idle),
        Err(error) => {
            tracing::warn!(%error, "Native abort could not inspect runtime");
            false
        }
    };
    // A clean checkpoint is written only after the native idle boundary and rollout flush.
    // Forced cancellation leaves it dirty and must still dispose of the process.
    if !preserve {
        shutdown(session).await;
    }
}

#[tracing::instrument(skip_all)]
pub(crate) async fn shutdown(session: &Session) {
    let listener = session
        .external_runtime
        .continuation_listener
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    if let Some(listener) = listener {
        listener.abort();
        let _ = listener.await;
    }

    let runtime = match session.external_runtime.take_for_shutdown().await {
        Ok(runtime) => runtime,
        Err(error) => {
            tracing::warn!(%error, "Native runtime lease shutdown failed");
            return;
        }
    };
    if let Some(mut runtime) = runtime {
        if let Some(bridge) = &runtime.bridge
            && let Err(error) = bridge.clear_turn().await
        {
            tracing::warn!(%error, "Native messaging drain failed");
        }
        if let Err(error) = runtime.runtime.shutdown().await {
            tracing::warn!(%error, "Native runtime shutdown failed");
        }
        if runtime.diagnostic_capture {
            let path = runtime
                .checkpoint_path
                .with_extension(format!("{}.diagnostic.json", uuid::Uuid::new_v4()));
            let snapshot = runtime.runtime.diagnostic_snapshot();
            let write = (|| -> anyhow::Result<()> {
                let mut options = std::fs::OpenOptions::new();
                options.create_new(true).write(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                options
                    .open(path)?
                    .write_all(&serde_json::to_vec(&snapshot)?)?;
                Ok(())
            })();
            if let Err(error) = write {
                tracing::warn!(%error, "could not retain native runtime diagnostics");
            }
        }
        if let Some(bridge) = &mut runtime.bridge
            && let Err(error) = bridge.shutdown().await
        {
            tracing::warn!(%error, "Native messaging bridge shutdown failed");
        }
    }
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run_turn(
    session: Arc<Session>,
    turn: Arc<TurnContext>,
    input: Vec<TurnInput>,
    cancellation: CancellationToken,
) -> SessionTaskResult {
    run_turn_start(
        session,
        turn,
        input,
        cancellation,
        /*continuation*/ None,
    )
    .await
}

#[tracing::instrument(skip_all)]
async fn run_turn_start(
    session: Arc<Session>,
    turn: Arc<TurnContext>,
    mut input: Vec<TurnInput>,
    cancellation: CancellationToken,
    continuation: Option<codex_external_runtime::ClaudeContinuation>,
) -> SessionTaskResult {
    session.emit_turn_started(&turn, TaskKind::Regular).await;
    let preparation = session
        .emit_turn_start_lifecycle(
            &turn,
            /*token_usage_at_turn_start*/ None,
            codex_extension_api::TurnStartPhase::RegularTaskStart,
        )
        .or_cancel(&cancellation)
        .await;
    if preparation.is_err() {
        return Err(CodexErr::TurnAborted);
    }
    let result = match continuation {
        Some(continuation) => continuation::run(&session, &turn, continuation, &cancellation).await,
        None => run_conversation(&session, &turn, &mut input, &cancellation).await,
    };
    let authentication_required = result
        .as_ref()
        .is_err_and(anyhow::Error::is::<codex_external_runtime::ExternalTurnFailure>);
    if authentication_required {
        session
            .external_runtime
            .authentication_required
            .store(/*val*/ true, std::sync::atomic::Ordering::Release);
    }
    if result
        .as_ref()
        .is_err_and(|error| !error.is::<ConfirmedInterrupt>())
    {
        shutdown(&session).await;
    }
    match result {
        Ok(text) => Ok(Some(crate::tasks::RegularTaskOutput {
            last_agent_message: text,
            mcp_startup_requirements: Default::default(),
        })),
        Err(error) if error.is::<ConfirmedInterrupt>() || cancellation.is_cancelled() => {
            Err(CodexErr::TurnAborted)
        }
        Err(_) if authentication_required => {
            let message = match NativeRuntime::for_model(&turn.model_info().slug) {
                Some((NativeRuntime::ClaudeCode, _)) => {
                    "Claude authentication failed. Send `claude login` in this chat to sign in to your Claude subscription; the failed message will not be replayed."
                }
                Some((NativeRuntime::CursorAgent, _)) => {
                    "Cursor authentication failed. Send `cursor login` in this chat to sign in to your Cursor account; the failed message will not be replayed."
                }
                None => "Native authentication failed; the failed message will not be replayed.",
            };
            Err(CodexErr::InvalidRequest(message.to_owned()))
        }
        Err(error) => {
            let runtime = NativeRuntime::for_model(&turn.model_info().slug)
                .map_or("Native", |(runtime, _)| runtime.display_name());
            Err(CodexErr::InvalidRequest(format!(
                "{runtime} runtime: {error:#}"
            )))
        }
    }
}

#[derive(Debug)]
struct ConfirmedInterrupt;

impl std::fmt::Display for ConfirmedInterrupt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Native turn interrupted at a confirmed idle boundary")
    }
}

impl std::error::Error for ConfirmedInterrupt {}

/// Main chats and their forks keep their starting window so config edits cannot break resume.
fn bound_auto_compact_window(
    thread_spawn_child: bool,
    configured: Option<i64>,
    checkpoint: &Checkpoint,
) -> Option<i64> {
    if thread_spawn_child {
        configured
    } else {
        checkpoint.auto_compact_window
    }
}

#[tracing::instrument(skip_all)]
async fn run_conversation(
    session: &Arc<Session>,
    turn: &Arc<TurnContext>,
    input: &mut Vec<TurnInput>,
    cancellation: &CancellationToken,
) -> anyhow::Result<Option<String>> {
    let settings = &turn.initial_settings;
    let (native_runtime, _) = NativeRuntime::for_model(&settings.model_info.slug)
        .context("not a native runtime model")?;
    let runtime_name = native_runtime.display_name();
    if session.tool_policy.require_managed_sandbox {
        bail!(
            "{runtime_name} native tools cannot satisfy the required ASM-managed sandbox; use an ASM-managed model"
        );
    }
    if turn.final_output_json_schema.is_some() {
        bail!("structured output is not supported; start a native-model chat");
    }
    // Resume reads this native metadata; no ASM context is generated or sent to Claude.
    if !session
        .persist_rollout_items(&[RolloutItem::TurnContext(turn.to_turn_context_item())])
        .await
    {
        bail!("could not persist native turn settings");
    }
    session.flush_rollout().await?;
    let effort = match native_runtime {
        NativeRuntime::ClaudeCode => settings.effective_reasoning_effort(),
        NativeRuntime::CursorAgent => settings.reasoning_effort().cloned(),
    }
    .map(|effort| effort.to_string());
    if native_runtime == NativeRuntime::ClaudeCode
        && effort
            .as_deref()
            .is_some_and(|value| !matches!(value, "low" | "medium" | "high" | "xhigh" | "max"))
    {
        bail!("unsupported Claude effort; use low, medium, high, xhigh, or max");
    }
    if turn.initial_environments.turn_environments().count() != 1 {
        bail!("native runtime requires exactly one local environment");
    }
    let cwd = turn
        .initial_environments
        .local_environment_cwd()
        .context("native runtime requires a local working directory")?
        .to_path_buf();
    let child_agent = turn.parent_thread_id.is_some()
        && turn.session_source.is_non_root_agent()
        && turn.multi_agent_version == MultiAgentVersion::V2;
    // Only root sessions read the scalar: children inherit it, and metadata is never a native setting.
    // Cursor exposes no compaction window, so the Claude window never applies to it.
    let thread_spawn_child = matches!(
        turn.session_source,
        SessionSource::SubAgent(SubAgentSource::ThreadSpawn { .. })
    );
    let auto_compact_window = if native_runtime == NativeRuntime::ClaudeCode && thread_spawn_child {
        turn.config.role_model_context_window.or_else(|| {
            (turn.multi_agent_version == MultiAgentVersion::V2)
                .then(|| {
                    turn.config
                        .subagent_model_context_windows
                        .get(&settings.model_info.slug)
                        .copied()
                })
                .flatten()
        })
    } else if native_runtime == NativeRuntime::ClaudeCode
        && !turn.session_source.is_non_root_agent()
    {
        turn.config.model_context_window
    } else {
        None
    };
    // Native children get ASM messaging tools through a session MCP server. Cursor team policy
    // can block it without telling ASM; mail still arrives between Cursor turns.
    let messaging = child_agent;
    let page_context = if native_runtime == NativeRuntime::ClaudeCode {
        let lease = RuntimeLease::take(&session.external_runtime)?;
        lease.runtime.as_ref().map_or(
            turn.config.native_runtimes.claude_code.page_context,
            |runtime| runtime.page_context,
        )
    } else {
        codex_config::PageContextPolicy::Inherit
    };
    let mut last_message = None;
    loop {
        if cancellation.is_cancelled() {
            bail!("turn interrupted");
        }
        // Automatic child starts place their assignment in the queue, not task input.
        if input.is_empty() {
            session
                .input_queue
                .accept_mailbox_delivery_for_current_turn(&session.active_turn, &turn.sub_id)
                .await;
            let (pending, _) = session
                .input_queue
                .get_pending_input(&session.active_turn)
                .await;
            *input = pending;
        }
        if page_context != codex_config::PageContextPolicy::Inherit {
            let history = session.clone_history().await;
            input::filter_page_context(input, page_context, history.raw_items());
        }
        if input::prepare_input(input)? > 0 {
            session
                .send_event(
                    turn,
                    EventMsg::Warning(WarningEvent {
                        message: "Encrypted ASM input was ignored; the user prompt is preserved."
                            .to_owned(),
                    }),
                )
                .await;
        }
        let original_history = session.clone_history().await;
        let messages = input_messages(original_history.raw_items())?;
        let mut lease = RuntimeLease::take(&session.external_runtime)?;
        let needs_binding = lease.runtime.is_some();
        let mut fork_boundary = None;
        if lease.runtime.is_none() {
            let mut role_instructions = if thread_spawn_child {
                turn.config.role_developer_instructions.clone()
            } else {
                None
            };
            if thread_spawn_child
                && turn.multi_agent_version == MultiAgentVersion::V1
                && role_instructions.is_none()
            {
                // V1 reload restores role identity but not its config; preserve saved runtime settings.
                let mut role_config = (*turn.config).clone();
                crate::agent::role::apply_role_to_config(
                    &mut role_config,
                    turn.session_source.get_agent_role().as_deref(),
                )
                .await
                .map_err(anyhow::Error::msg)?;
                role_instructions = role_config.role_developer_instructions;
            }
            let directory = turn.config.codex_home.join("claude-runtime");
            std::fs::create_dir_all(&directory)?;
            let checkpoint_path = directory
                .join(format!("{}.json", session.thread_id()))
                .to_path_buf();
            let lock = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(directory.join(format!("{}.lock", session.thread_id())))?;
            lock.try_lock()
                .context("native session is already owned by another ASM process")?;
            let (mut checkpoint, session_start, linkage) = match (
                lease.start_fork()?,
                std::fs::read(&checkpoint_path),
            ) {
                // A temporary fork never saves linkage; the native cutoff pins main as admitted.
                (Some(seed), _) => {
                    let mut checkpoint = seed.parent;
                    let id = uuid::Uuid::new_v4().to_string();
                    let parent = std::mem::replace(&mut checkpoint.session_id, id.clone());
                    checkpoint.fork_point = None;
                    let at = seed.fork_point;
                    let fork = SessionStart::Fork { parent, at, id };
                    (checkpoint, fork, Linkage::Temporary)
                }
                (None, Ok(bytes)) => {
                    let checkpoint: Checkpoint = serde_json::from_slice(&bytes)
                        .context("incompatible native session linkage; start a new chat")?;
                    let resume = SessionStart::Resume {
                        id: checkpoint.session_id.clone(),
                    };
                    (checkpoint, resume, Linkage::Saved)
                }
                (None, Err(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                    // Shell-only history can precede the first native conversation.
                    // Other native input still requires its saved linkage to prevent replay.
                    let previous_input = original_history.raw_items().filter(|item| {
                        !matches!(item, ResponseItem::Message {
                            internal_chat_message_metadata_passthrough: Some(metadata), ..
                        } if metadata.content_item_kinds.as_ref().is_some_and(|kinds| {
                            kinds.len() == 1 && kinds[0].as_str() == input::SHELL_CONTEXT_KIND
                        }))
                    });
                    if !input_messages(previous_input)?.is_empty()
                        || original_history.raw_items().any(|item| matches!(item, ResponseItem::Message { role, .. } if role == "assistant"))
                    {
                        bail!("missing native session linkage for existing history; start a new chat");
                    }
                    let id = session.thread_id().to_string();
                    (
                        Checkpoint {
                            format_version: 2,
                            model: settings.model_info.slug.clone(),
                            effort: effort.clone(),
                            auto_compact_window,
                            cwd: cwd.clone(),
                            session_id: id.clone(),
                            messaging,
                            in_flight: false,
                            input_count: 0,
                            input_prefix: input_prefix(&[])?,
                            fork_point: None,
                        },
                        SessionStart::New { id },
                        Linkage::Saved,
                    )
                }
                (None, Err(error)) => return Err(error.into()),
            };
            checkpoint.validate(
                &settings.model_info.slug,
                bound_auto_compact_window(thread_spawn_child, auto_compact_window, &checkpoint),
                &cwd,
                messaging,
                &messages,
            )?;
            if input.is_empty()
                && messages[checkpoint.input_count..]
                    .iter()
                    .all(|message| message.as_str().is_some_and(|text| text.trim().is_empty()))
            {
                return Ok(last_message);
            }
            let bridge = if messaging {
                let step = session
                    .capture_speculative_step_context(Arc::clone(turn), cancellation)
                    .await?;
                let max_wait = turn.config.multi_agent_v2.max_wait_timeout_ms;
                let bridge = ExternalMessageBridge::start(std::time::Duration::from_millis(
                    u64::try_from(max_wait).unwrap_or_default(),
                ))
                .await?;
                bridge
                    .set_turn(Arc::clone(session), step, cancellation.clone())
                    .await?;
                Some(bridge)
            } else {
                None
            };
            let message = match native_runtime {
                NativeRuntime::ClaudeCode => {
                    "Claude Code owns instructions, tools, hooks and permissions, and bypasses its permission prompts; native deny rules and managed policy still apply. ASM sandbox and approvals do not apply."
                }
                NativeRuntime::CursorAgent if child_agent => {
                    "Cursor Agent owns instructions, tools and permissions. Cursor runs with --force; native denies and team auto-run controls still apply. ASM sandbox and approvals do not apply; remaining Cursor approval requests are denied. Token usage and context fill are unavailable. ASM communication uses a local CLI helper through native shell tools; messages arrive between turns."
                }
                NativeRuntime::CursorAgent => {
                    "Cursor Agent owns instructions, tools and permissions. Cursor runs with --force; native denies and team auto-run controls still apply. ASM sandbox and approvals do not apply; remaining Cursor approval requests are denied. Token usage and context fill are unavailable."
                }
            };
            session
                .send_event(
                    turn,
                    EventMsg::Warning(WarningEvent {
                        message: message.to_owned(),
                    }),
                )
                .await;
            let usage_baseline = match &session_start {
                SessionStart::New { .. } => Some(Default::default()),
                SessionStart::Resume { .. } | SessionStart::Fork { .. } => None,
            };
            let mut disallowed_tools = Vec::new();
            // Spawned children run headless: nobody can answer their native questions.
            if thread_spawn_child {
                disallowed_tools.push("AskUserQuestion".to_owned());
            }
            // A side conversation answers questions; it must not start or steer native agents.
            if matches!(linkage, Linkage::Temporary) {
                disallowed_tools.extend(TEMPORARY_FORK_DISALLOWED_TOOLS.map(str::to_owned));
                fork_boundary = Some(TEMPORARY_FORK_BOUNDARY);
            }
            let mut runtime = ExternalSession::spawn_runtime(
                bridge.as_ref(),
                disallowed_tools,
                &turn.config.native_runtimes.claude_code,
                role_instructions,
                if thread_spawn_child {
                    codex_external_runtime::ClaudeSessionKind::ManagedChild
                } else if !turn.session_source.is_non_root_agent()
                    && !matches!(linkage, Linkage::Temporary)
                {
                    codex_external_runtime::ClaudeSessionKind::ManagedRoot
                } else {
                    codex_external_runtime::ClaudeSessionKind::NativeDefault
                },
                &mut checkpoint,
                &checkpoint_path,
                session_start,
            )
            .await?;
            if let RuntimeProcess::Claude(process) = &mut runtime {
                if matches!(linkage, Linkage::Saved) {
                    let path = checkpoint_path.clone();
                    process.set_checkpoint_invalidator(move || {
                        let mut checkpoint: Checkpoint =
                            serde_json::from_slice(&std::fs::read(&path)?)?;
                        if !checkpoint.in_flight {
                            checkpoint.in_flight = true;
                            checkpoint.fork_point = None;
                            checkpoint.save(&path)?;
                        }
                        Ok(())
                    });
                }
                if let Some(continuations) = process.take_continuations() {
                    continuation::listen(session, continuations);
                }
            }
            lease.runtime = Some(ExternalSession {
                foreground_idle: true,
                runtime,
                page_context,
                bridge,
                checkpoint,
                checkpoint_path,
                linkage,
                _lock: lock,
                diagnostic_capture: turn.config.diagnostic_log_capture,
                usage_baseline,
            });
        }
        let runtime = lease
            .runtime
            .as_mut()
            .context("native session was not initialized")?;
        runtime.checkpoint.validate_live(
            &settings.model_info.slug,
            bound_auto_compact_window(thread_spawn_child, auto_compact_window, &runtime.checkpoint),
            &cwd,
            messaging,
            &messages,
        )?;
        validate_pending_prompt(
            session,
            &messages[runtime.checkpoint.input_count..],
            input,
            fork_boundary,
            |prompt| match &runtime.runtime {
                RuntimeProcess::Claude(_) => codex_external_runtime::validate_claude_prompt(prompt),
                RuntimeProcess::Cursor(process) => process.validate_prompt(prompt),
            },
        )?;
        if runtime.checkpoint.effort != effort {
            match &mut runtime.runtime {
                RuntimeProcess::Claude(process) => {
                    runtime.checkpoint.in_flight = true;
                    runtime
                        .linkage
                        .save(&runtime.checkpoint, &runtime.checkpoint_path)?;
                    process.set_effort(effort.as_deref()).await?;
                }
                RuntimeProcess::Cursor(process) => process.set_effort(effort.as_deref()).await?,
            }
            runtime.checkpoint.effort = effort.clone();
            runtime
                .linkage
                .save(&runtime.checkpoint, &runtime.checkpoint_path)?;
        }
        for item in input.drain(..) {
            // Reject native setup or effort changes before recording input for later delivery.
            record_pending_input(
                session,
                turn,
                &settings.model_info,
                item,
                Vec::new(),
                codex_thread_store::PersistContext::TurnStart,
            )
            .await;
        }
        let history = session.clone_history().await;
        let messages = input_messages(history.raw_items())?;
        let mut pending_messages = messages[runtime.checkpoint.input_count..].to_vec();
        let mut prompt = prompt_content(&pending_messages)?;
        if matches!(&prompt, codex_external_runtime::Prompt::Text(text) if text.trim().is_empty()) {
            return Ok(last_message);
        }
        if let Some(boundary) = fork_boundary {
            pending_messages.insert(/*index*/ 0, Value::String(boundary.to_owned()));
            prompt = prompt_content(&pending_messages)?;
        }
        if needs_binding && let Some(bridge) = &runtime.bridge {
            let step = session
                .capture_speculative_step_context(Arc::clone(turn), cancellation)
                .await?;
            bridge
                .set_turn(Arc::clone(session), step, cancellation.clone())
                .await?;
        }
        runtime.checkpoint.input_count = messages.len();
        runtime.checkpoint.input_prefix = input_prefix(&messages)?;
        runtime.foreground_idle = false;
        runtime.checkpoint.in_flight = true;
        runtime.checkpoint.fork_point = None;
        runtime
            .linkage
            .save(&runtime.checkpoint, &runtime.checkpoint_path)?;
        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(/*buffer*/ 32);
        let mut stream = events::MessageStream::default();
        stream.activity.messaging = messaging;
        stream.activity.server = native_runtime.id();
        // Only Claude accepts native queued input; Cursor receives mail through tool results.
        let (steering, mut mid_turn_delivery) = match &runtime.runtime {
            RuntimeProcess::Claude(process) => (messaging, process.subscribe_mid_turn_delivery()),
            RuntimeProcess::Cursor(_) => (false, tokio::sync::watch::channel(false).1),
        };
        let mut mail_delivery = runtime
            .bridge
            .as_mut()
            .map(|bridge| &mut bridge.mail_delivery);
        let turn_state = session
            .input_queue
            .turn_state_for_sub_id(&session.active_turn, &turn.sub_id)
            .await;
        let (mut activity, mut pending_activity) = session
            .input_queue
            .subscribe_activity(turn_state.as_deref())
            .await;
        let (steering_tx, steering_rx) = tokio::sync::mpsc::channel(1);
        let mut submitted_steering_count = 0;
        let mut mail_receipts =
            delivery::CursorMailReceipts::new(runtime.checkpoint.session_id.clone());
        let result = {
            let mut run = match &mut runtime.runtime {
                RuntimeProcess::Claude(process) => Box::pin(process.run_turn(
                    &prompt,
                    events_tx,
                    cancellation.clone(),
                    steering.then_some(steering_rx),
                ))
                    as std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send + '_>>,
                RuntimeProcess::Cursor(process) => {
                    Box::pin(process.run_turn(&prompt, events_tx, cancellation.clone()))
                }
            };
            loop {
                tokio::select! {
                    biased;
                    result = &mut run => break result,
                    _ = async {
                        if pending_activity.is_some() { return; }
                        let _ = activity.changed().await;
                    }, if steering && *mid_turn_delivery.borrow() && steering_tx.capacity() > 0 && !cancellation.is_cancelled() => {
                        pending_activity = None;
                        if continuation::submit_steering(session, turn, &mut runtime.checkpoint,
                            &runtime.linkage, &runtime.checkpoint_path, page_context, continuation::SteeringSubmission::Foreground(&steering_tx)).await? {
                            submitted_steering_count += 1;
                        }
                    }
                    Some(reply) = async {
                        match mail_delivery.as_mut() {
                            Some(requests) => requests.recv().await,
                            None => std::future::pending().await,
                        }
                    }, if native_runtime == NativeRuntime::CursorAgent && !cancellation.is_cancelled() => {
                        if reply.is_closed() { continue; }
                        if let Some(turn_state) = &turn_state {
                            let mut pending = session.input_queue.take_current_turn_mail(turn_state).await;
                            if input::prepare_input(&mut pending)? > 0 {
                                session.send_event(turn, EventMsg::Warning(WarningEvent { message: "Encrypted ASM input was ignored; the user prompt is preserved.".to_owned() })).await;
                            }
                            for item in pending {
                                record_pending_input(session, turn, &settings.model_info, item, Vec::new(),
                                    codex_thread_store::PersistContext::TurnStart).await;
                            }
                        }
                        let messages = input_messages(session.clone_history().await.raw_items())?;
                        let mut delivered = messages[runtime.checkpoint.input_count..].iter()
                            .map(|message| message.as_str().map(str::to_owned)
                                .context("Cursor mailbox only supports text input"))
                            .collect::<anyhow::Result<Vec<_>>>()?;
                        runtime.checkpoint.input_count = messages.len();
                        runtime.checkpoint.input_prefix = input_prefix(&messages)?;
                        session.flush_rollout().await?;
                        runtime.linkage.save(&runtime.checkpoint, &runtime.checkpoint_path)?;
                        // The internal oneshot confirms only bridge receipt, not native delivery.
                        mail_receipts.stamp(&mut delivered);
                        if reply.send(delivered).is_err() {
                            bail!("Cursor mailbox result was not accepted; explicit recovery is required");
                        }
                    }
                    changed = mid_turn_delivery.changed(), if steering => {
                        if changed.is_err() { bail!("Claude steering capability channel closed"); }
                    }
                    Some(frame) = events_rx.recv() => {
                        if let Some(text) = record_frame(native_runtime, session, turn, &frame, &mut stream, &mut mail_receipts).await? {
                            last_message = Some(text);
                        }
                    }
                }
            }
        };
        while let Ok(frame) = events_rx.try_recv() {
            if let Some(text) = record_frame(
                native_runtime,
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
        let cleanup = match &runtime.bridge {
            Some(bridge) => bridge.clear_turn().await,
            None => Ok(()),
        };
        let outcome = match result {
            Ok(result) => {
                cleanup?;
                result
            }
            Err(error) => {
                if let Err(cleanup_error) = cleanup {
                    tracing::warn!(%cleanup_error, "Claude cleanup also failed");
                }
                return Err(error);
            }
        };
        mail_receipts.ensure_delivered()?;
        if outcome.submitted_steering_count != submitted_steering_count {
            bail!(
                "Claude did not submit all recorded steering messages; explicit recovery is required"
            );
        }
        let interrupted = outcome.interrupted;
        let completed = complete_response(
            session,
            turn,
            runtime,
            native_runtime,
            stream,
            outcome,
            /*generation*/ None,
        )
        .await;
        if interrupted && native_runtime == NativeRuntime::CursorAgent {
            lease.runtime = None;
        }
        completed?;
        drop(lease);
        session
            .input_queue
            .accept_mailbox_delivery_for_current_turn(&session.active_turn, &turn.sub_id)
            .await;
        let (pending, _) = session
            .input_queue
            .get_pending_input(&session.active_turn)
            .await;
        if pending.is_empty() {
            return Ok(last_message);
        }
        *input = pending;
    }
}

async fn record_frame(
    native_runtime: NativeRuntime,
    session: &Session,
    turn: &TurnContext,
    frame: &Value,
    stream: &mut events::MessageStream,
    mail_receipts: &mut delivery::CursorMailReceipts,
) -> anyhow::Result<Option<String>> {
    mail_receipts.observe(frame);
    if native_runtime == NativeRuntime::ClaudeCode
        && frame["type"] == "result"
        && frame.get("parent_tool_use_id").is_none_or(Value::is_null)
    {
        stream.results.push(frame.clone());
    }
    match native_runtime {
        NativeRuntime::ClaudeCode => record_assistant_frame(session, turn, frame, stream).await,
        NativeRuntime::CursorAgent => record_cursor_frame(session, turn, frame, stream).await,
    }
}

#[tracing::instrument(skip_all)]
pub(crate) async fn stop_for_login(session: &Session) -> bool {
    if session.external_runtime.is_busy() {
        return false;
    }
    shutdown(session).await;
    true
}
