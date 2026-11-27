//! Thread-scoped lifecycle for the official CLI's native conversation and tools.
mod active_handoff;
mod activity;
mod bridge;
mod completion;
mod context;
mod continuation;
use completion::complete_response;
use completion::save_reconciled_checkpoint;
mod cursor;
mod deletion;
mod events;
mod fork;
mod input;
mod messages;
mod portable;
mod state;
pub(crate) use active_handoff::active_handoff_history;
use bridge::ExternalMessageBridge;
use cursor::record_cursor_frame;
pub(crate) use deletion::delete_thread_data;
pub(crate) use deletion::ensure_no_runtime_owner;
use events::record_assistant_frame;
use fork::TEMPORARY_FORK_BOUNDARY;
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
use codex_external_runtime::CursorEffortRejected;
use codex_external_runtime::CursorRunFailed;
use codex_external_runtime::ExternalTurnFailure;
use codex_history::RolloutItem;
use codex_protocol::error::CodexErr;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::NativeRuntime;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::MultiAgentVersion;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_protocol::protocol::WarningEvent;
use tokio_util::sync::CancellationToken;

use crate::hook_runtime::record_pending_input;
use crate::session::InputQueueEntry;
use crate::session::SubmittedAdditionalContext;
use crate::session::TurnInput;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::state::TaskKind;
use crate::tasks::SessionTaskResult;

pub(crate) const CURSOR_COLD_RESUME: &str = "Cursor SDK chats cannot resume after the runtime closes. Start a new chat; saved history remains readable.";

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
        if let Err(error) = runtime.bridge.clear_turn().await {
            tracing::warn!(%error, "Native tool bridge drain failed");
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
        if let Err(error) = runtime.bridge.shutdown().await {
            tracing::warn!(%error, "Native tool bridge shutdown failed");
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
    let failure = result
        .as_ref()
        .err()
        .and_then(|error| error.downcast_ref::<ExternalTurnFailure>())
        .copied();
    let native_runtime =
        NativeRuntime::for_model(&turn.model_info().slug).map(|(runtime, _)| runtime);
    let cursor = native_runtime == Some(NativeRuntime::CursorAgent);
    if !cursor && failure == Some(ExternalTurnFailure::Authentication) {
        session
            .external_runtime
            .authentication_required
            .store(/*val*/ true, std::sync::atomic::Ordering::Release);
    }
    if let Err(error) = &result {
        // A confirmed Claude interrupt or usage limit leaves a reusable idle process. A
        // temporary side fork needs that process because it cannot be started again.
        let preserve = if cursor {
            error.is::<CursorEffortRejected>() || error.is::<CursorPromptRejected>()
        } else {
            error.is::<ConfirmedInterrupt>() || failure == Some(ExternalTurnFailure::UsageLimit)
        };
        if cursor {
            let mut baseline = session
                .external_runtime
                .additional_context_baseline
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            baseline.rendered = baseline.cursor_submitted.clone();
        }
        if !preserve {
            shutdown(&session).await;
        }
    }
    match result {
        Ok(text) => Ok(Some(crate::tasks::RegularTaskOutput {
            last_agent_message: text,
            mcp_startup_requirements: Default::default(),
        })),
        Err(error) if error.is::<ConfirmedInterrupt>() || cancellation.is_cancelled() => {
            Err(CodexErr::TurnAborted)
        }
        Err(_) if !cursor && failure == Some(ExternalTurnFailure::UsageLimit) => Err(CodexErr::InvalidRequest(
            "Claude usage limit reached. This chat stays usable: send a new message after the limit resets; nothing will be replayed."
                .to_owned(),
        )),
        Err(_) if failure == Some(ExternalTurnFailure::Authentication) => {
            let message = match native_runtime {
                Some(NativeRuntime::ClaudeCode) => {
                    "Claude authentication failed. Send `claude login` in this chat to sign in to your Claude subscription; the failed message will not be replayed."
                }
                Some(NativeRuntime::CursorAgent) => {
                    "Cursor SDK authentication failed. Set a valid CURSOR_API_KEY in the ASM backend environment and start a new chat; the failed message will not be replayed."
                }
                None => "Native authentication failed; the failed message will not be replayed.",
            };
            Err(CodexErr::InvalidRequest(message.to_owned()))
        }
        Err(error) => {
            let runtime = native_runtime.map_or("Native", NativeRuntime::display_name);
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

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
struct CursorPromptRejected(anyhow::Error);

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
    if turn.final_output_json_schema.is_some() {
        bail!("structured output is not supported; start a native-model chat");
    }
    // Resume reads this native metadata; no ASM context is generated or sent to Claude.
    let context_item = turn.context_item_with_mode(Some(turn.collaboration_mode()));
    if !session
        .persist_rollout_items(&[RolloutItem::TurnContext(context_item)])
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
    let page_context = if native_runtime == NativeRuntime::ClaudeCode {
        let lease = RuntimeLease::take(&session.external_runtime)?;
        lease.runtime.as_ref().map_or(
            turn.config.native_runtimes.claude_code.page_context,
            |runtime| runtime.page_context,
        )
    } else {
        codex_config::PageContextPolicy::Inherit
    };
    session.start_system1_turn(turn);
    let mut first_prompt = true;
    let mut decision_effort: Option<ReasoningEffort> = None;
    let mut decision_eligible = false;
    let mut last_message = None;
    loop {
        if cancellation.is_cancelled() {
            bail!("turn interrupted");
        }
        let needs_pending_input = input.is_empty();
        if let Some(context) = turn.extension_data.remove::<SubmittedAdditionalContext>() {
            let mut items = context::submitted_additional_context(session, turn, &context);
            items.append(input);
            *input = items;
        }
        // Automatic child starts place their assignment in the queue, not task input.
        if needs_pending_input {
            session
                .input_queue
                .accept_mailbox_delivery_for_current_turn(&session.active_turn, &turn.sub_id)
                .await;
            let (pending, _) = session
                .input_queue
                .get_pending_input(&session.active_turn)
                .await;
            input.extend(materialize_native_inputs(session, turn, pending));
        }
        if page_context != codex_config::PageContextPolicy::Inherit {
            let history = session.clone_history().await;
            input::filter_page_context(input, page_context, history.raw_items());
        }
        let original_history = session.clone_history().await;
        let messages = input_messages(original_history.raw_items())?;
        let mut lease = RuntimeLease::take(&session.external_runtime)?;
        let needs_binding = lease.runtime.is_some();
        let input_rejection = |error| {
            if needs_binding && native_runtime == NativeRuntime::CursorAgent {
                // No prompt or tool authority has been sent to the warm runtime yet.
                CursorPromptRejected(error).into()
            } else {
                error
            }
        };
        let prepared_context = if needs_binding {
            context::capture(
                session,
                Arc::clone(turn),
                input,
                /*restored_role*/ None,
                cancellation,
            )
            .await?
        } else {
            let mut initial_context = None;
            let restored_role = if thread_spawn_child
                && turn.multi_agent_version == MultiAgentVersion::V1
                && turn.config.role_developer_instructions.is_none()
            {
                // V1 reload restores role identity but not its config; preserve saved runtime settings.
                let mut role_config = (*turn.config).clone();
                crate::agent::role::apply_role_to_config(
                    &mut role_config,
                    turn.session_source.get_agent_role().as_deref(),
                )
                .await
                .map_err(anyhow::Error::msg)?;
                role_config.role_developer_instructions
            } else {
                None
            };
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
            let (checkpoint, session_start, linkage) = match (
                lease.start_fork()?,
                std::fs::read(&checkpoint_path),
            ) {
                // A temporary fork never saves linkage; the native cutoff pins main as admitted.
                (Some(seed), _) => {
                    let mut checkpoint = seed.parent;
                    let id = uuid::Uuid::new_v4().to_string();
                    let parent = std::mem::replace(&mut checkpoint.session_id, id.clone());
                    checkpoint.fork_point = None;
                    initial_context = Some(Value::String(TEMPORARY_FORK_BOUNDARY.to_owned()));
                    let at = seed.fork_point;
                    let fork = SessionStart::Fork { parent, at, id };
                    (checkpoint, fork, Linkage::Temporary)
                }
                (None, Ok(_)) if native_runtime == NativeRuntime::CursorAgent => {
                    bail!(CURSOR_COLD_RESUME);
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
                    let inherited = portable::inherited_prefix(original_history.annotated_items())?;
                    let local = original_history.raw_items().skip(inherited.len());
                    let previous_input = local.clone().filter(|item| {
                        !matches!(item, ResponseItem::Message {
                            role, content, internal_chat_message_metadata_passthrough: Some(metadata), ..
                        } if metadata.content_item_kinds.as_ref().is_some_and(|kinds| {
                            kinds.len() == 1 && kinds[0].as_str() == input::SHELL_CONTEXT_KIND
                        }) || (matches!(role.as_str(), "user" | "developer")
                            && !content.is_empty()
                            && content.iter().all(|part| matches!(part, codex_protocol::models::ContentItem::InputText { .. }))
                            && metadata.content_item_metadata.as_ref().is_some_and(|parts| {
                                parts.len() == content.len() && parts.iter().enumerate().all(|(index, part)| matches!(
                                    part.provenance,
                                    codex_protocol::models::ContentItemProvenance::Harness { hook: false }
                                        | codex_protocol::models::ContentItemProvenance::DeveloperInstructions { .. }
                                        | codex_protocol::models::ContentItemProvenance::Skills
                                        | codex_protocol::models::ContentItemProvenance::AgentsMd
                                ) || role == "developer"
                                    && matches!(&part.provenance,
                                        codex_protocol::models::ContentItemProvenance::Tool {
                                            namespace: Some(codex_protocol::models::ContentItemNamespace::Collaboration) })
                                    && metadata.content_item_kinds.as_ref().and_then(|kinds| kinds.get(index))
                                        .is_some_and(|kind| matches!(kind.as_str(),
                                            "multi_agent.mode_instructions" | codex_prompts::MultiAgentRoleInstructions::KIND)))
                            })))
                    });
                    if !input_messages(previous_input)?.is_empty()
                        || local.clone().any(|item| matches!(item, ResponseItem::Message { role, .. } if role == "assistant"))
                    {
                        if native_runtime == NativeRuntime::CursorAgent {
                            bail!(CURSOR_COLD_RESUME);
                        }
                        bail!("missing native session linkage for existing history; start a new chat");
                    }
                    let (input_count, reference) = portable::reference_message(inherited)?;
                    initial_context = reference;
                    let id = if native_runtime == NativeRuntime::CursorAgent || inherited.is_empty()
                    {
                        session.thread_id().to_string()
                    } else {
                        uuid::Uuid::new_v4().to_string()
                    };
                    let linkage =
                        if native_runtime == NativeRuntime::ClaudeCode && turn.config.ephemeral {
                            Linkage::Temporary
                        } else {
                            Linkage::Saved
                        };
                    (
                        Checkpoint {
                            format_version: 3,
                            model: settings.model_info.slug.clone(),
                            effort: effort.clone(),
                            auto_compact_window,
                            cwd: cwd.clone(),
                            session_id: id.clone(),
                            messaging: true,
                            in_flight: false,
                            input_count,
                            input_prefix: input_prefix(&messages[..input_count])?,
                            fork_point: None,
                        },
                        SessionStart::New { id },
                        linkage,
                    )
                }
                (None, Err(error)) => return Err(error.into()),
            };
            checkpoint.validate(
                &settings.model_info.slug,
                bound_auto_compact_window(thread_spawn_child, auto_compact_window, &checkpoint),
                &cwd,
                /*messaging*/ true,
                &messages,
            )?;
            if input.is_empty()
                && messages[checkpoint.input_count..]
                    .iter()
                    .all(|message| message.as_str().is_some_and(|text| text.trim().is_empty()))
            {
                return Ok(last_message);
            }
            let prepared = context::capture(
                session,
                Arc::clone(turn),
                input,
                restored_role.as_deref(),
                cancellation,
            )
            .await?;
            let max_wait = turn.config.multi_agent_v2.max_wait_timeout_ms;
            let bridge = ExternalMessageBridge::start(
                std::time::Duration::from_millis(u64::try_from(max_wait).unwrap_or_default()),
                native_runtime,
            )
            .await?;
            bridge
                .set_turn(
                    Arc::clone(session),
                    Arc::clone(&prepared.step),
                    cancellation.clone(),
                )
                .await?;
            let message = match native_runtime {
                NativeRuntime::ClaudeCode => {
                    "Claude Code supplies inference and conversation state. ASM owns tools, instructions, skills, sandbox and approvals."
                }
                NativeRuntime::CursorAgent => {
                    "Cursor SDK provides inference and conversation state using CURSOR_API_KEY from the ASM backend environment. ASM owns tools, skills, sandbox and approvals. Base instructions are supplied as labeled context, not as Cursor system instructions. Context fill and output speed are unavailable."
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
            let usage_baseline = (native_runtime == NativeRuntime::ClaudeCode
                && matches!(&session_start, SessionStart::New { .. }))
            .then(Default::default);
            let mut runtime = ExternalSession::spawn_runtime(
                &bridge,
                session.get_prompt_base_instructions().await.text,
                if thread_spawn_child {
                    codex_external_runtime::ClaudeSessionKind::Child
                } else {
                    codex_external_runtime::ClaudeSessionKind::Chat
                },
                Arc::clone(&session.external_runtime.queued_input_count),
                &checkpoint,
                session_start,
                &linkage,
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
                initial_context,
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
            prepared
        };
        input::normalize_selections(input);
        if input::prepare_input(input).map_err(input_rejection)? > 0 {
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
        drop(original_history);
        drop(messages);
        let runtime = lease
            .runtime
            .as_mut()
            .context("native session was not initialized")?;
        let prepared_step = Arc::clone(&prepared_context.step);
        {
            let runtime = &mut *runtime;
            let input = input.as_slice();
            let effort = &effort;
            let decision_effort = &mut decision_effort;
            let decision_eligible = &mut decision_eligible;
            let cwd = &cwd;
            let model = &settings.model_info.slug;
            let admit = move |context_items: &[ResponseItem], injections: &[ResponseItem]| {
                let generated = input_messages(context_items.iter().chain(injections));
                async move {
                    let generated = generated?;
                    let mut messages = input_messages(session.clone_history().await.raw_items())?;
                    runtime.checkpoint.validate_live(
                        model,
                        bound_auto_compact_window(
                            thread_spawn_child,
                            auto_compact_window,
                            &runtime.checkpoint,
                        ),
                        cwd,
                        /*messaging*/ true,
                        &messages,
                    )?;
                    messages.extend(generated);
                    validate_pending_prompt(
                        session,
                        &messages[runtime.checkpoint.input_count..],
                        input,
                        runtime.initial_context.as_ref(),
                        |prompt| match &runtime.runtime {
                            RuntimeProcess::Claude(_) => {
                                codex_external_runtime::validate_claude_prompt(prompt)
                            }
                            RuntimeProcess::Cursor(process) => process.validate_prompt(prompt),
                        },
                    )
                    .map_err(input_rejection)?;
                    drop(messages);
                    if first_prompt {
                        let supported = match &runtime.runtime {
                            RuntimeProcess::Claude(process)
                                if !process.has_background_work()
                                    && !process.has_unprojected_activity() =>
                            {
                                &["low", "medium", "high", "xhigh", "max"][..]
                            }
                            RuntimeProcess::Claude(_) | RuntimeProcess::Cursor(_) => &[],
                        };
                        *decision_eligible = session.native_system1_eligible(turn, supported);
                        if *decision_eligible
                            && session.system1_enabled(turn.config.system1.enabled)
                        {
                            for item in input.iter().filter(|item| {
                                matches!(
                                    item,
                                    TurnInput::UserInput { .. }
                                        | TurnInput::InterAgentCommunication(_)
                                )
                            }) {
                                session.record_system1_input(turn, item);
                            }
                            *decision_effort = session
                                .adapt_reasoning(
                                    turn,
                                    codex_config::System1Trigger::TurnStart,
                                    /*pending_input*/ &[],
                                    cancellation,
                                    Some(supported),
                                )
                                .await;
                        }
                    }
                    if cancellation.is_cancelled() {
                        bail!("turn interrupted");
                    }
                    let requested = decision_effort
                        .as_ref()
                        .map(ToString::to_string)
                        .or_else(|| effort.clone());
                    if runtime.checkpoint.effort != requested
                        || (first_prompt && decision_effort.is_some())
                    {
                        // An unsent handoff has no checkpoint to resume yet.
                        let linkage = runtime
                            .initial_context
                            .as_ref()
                            .map_or(&runtime.linkage, |_| &Linkage::Temporary);
                        let changed = match &mut runtime.runtime {
                            RuntimeProcess::Claude(process) => {
                                runtime.checkpoint.in_flight = true;
                                linkage.save(&runtime.checkpoint, &runtime.checkpoint_path)?;
                                process.set_effort(requested.as_deref()).await?;
                                runtime
                                    .checkpoint
                                    .effort
                                    .as_deref()
                                    .is_some_and(|previous| Some(previous) != requested.as_deref())
                            }
                            RuntimeProcess::Cursor(process) => {
                                process.set_effort(requested.as_deref()).await?;
                                false
                            }
                        };
                        if changed && decision_effort.is_some() {
                            session.record_native_system1_effort_change();
                        }
                        tracing::debug!(runtime_name, effort = ?requested, "Native effort confirmed");
                        runtime.checkpoint.effort = requested;
                        linkage.save(&runtime.checkpoint, &runtime.checkpoint_path)?;
                    }
                    Ok::<(), anyhow::Error>(())
                }
            };
            Box::pin(context::record(session, prepared_context, admit)).await?;
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
            .await?;
        }
        let history = session.clone_history().await;
        let messages = input_messages(history.raw_items())?;
        drop(history);
        let mut pending_messages = messages[runtime.checkpoint.input_count..].to_vec();
        let mut prompt = prompt_content(&pending_messages)?;
        if matches!(&prompt, codex_external_runtime::Prompt::Text(text) if text.trim().is_empty()) {
            return Ok(last_message);
        }
        if let Some(boundary) = &runtime.initial_context {
            pending_messages.insert(/*index*/ 0, boundary.clone());
            prompt = prompt_content(&pending_messages)?;
        }
        drop(pending_messages);
        if needs_binding {
            runtime
                .bridge
                .set_turn(Arc::clone(session), prepared_step, cancellation.clone())
                .await?;
        }
        if first_prompt {
            let applied = match &runtime.runtime {
                RuntimeProcess::Claude(_) => runtime.checkpoint.effort.as_deref(),
                RuntimeProcess::Cursor(process) => process.current_effort(),
            }
            .and_then(|effort| effort.parse().ok());
            session
                .report_system1_effort(turn, applied, decision_eligible)
                .await;
            first_prompt = false;
        }
        runtime.checkpoint.input_count = messages.len();
        runtime.checkpoint.input_prefix = input_prefix(&messages)?;
        drop(messages);
        runtime.foreground_idle = false;
        runtime.checkpoint.in_flight = true;
        runtime.checkpoint.fork_point = None;
        runtime
            .linkage
            .save(&runtime.checkpoint, &runtime.checkpoint_path)?;
        runtime.initial_context = None;
        if native_runtime == NativeRuntime::CursorAgent {
            let mut baseline = session
                .external_runtime
                .additional_context_baseline
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            baseline.cursor_submitted = baseline.rendered.clone();
        }
        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(/*buffer*/ 32);
        let mut stream = events::MessageStream::default();
        let handoff_capture = active_handoff::HandoffCapture::default();
        if native_runtime == NativeRuntime::ClaudeCode {
            turn.extension_data.insert(handoff_capture.clone());
        }
        stream.activity.messaging = true;
        stream.activity.server = native_runtime.id();
        // Only Claude accepts queued input during inference.
        let (steering, mut mid_turn_delivery) = match &runtime.runtime {
            RuntimeProcess::Claude(process) => (true, process.subscribe_mid_turn_delivery()),
            RuntimeProcess::Cursor(_) => (false, tokio::sync::watch::channel(false).1),
        };
        let turn_state = session
            .input_queue
            .turn_state_for_sub_id(&session.active_turn, &turn.sub_id)
            .await;
        let (mut activity, mut pending_activity) = session
            .input_queue
            .subscribe_activity(turn_state.as_deref())
            .await;
        let (steering_tx, steering_rx) = tokio::sync::mpsc::channel(1);
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
                            stream.submitted_steering_count += 1;
                            handoff_capture.send_replace(None);
                        }
                    }
                    changed = mid_turn_delivery.changed(), if steering => {
                        if changed.is_err() { bail!("Claude steering capability channel closed"); }
                    }
                    Some(frame) = events_rx.recv() => {
                        if let Some(text) = record_frame(native_runtime, session, turn, &frame, &mut stream).await? {
                            last_message = Some(text);
                        }
                    }
                }
            }
        };
        while let Ok(frame) = events_rx.try_recv() {
            if let Some(text) =
                record_frame(native_runtime, session, turn, &frame, &mut stream).await?
            {
                last_message = Some(text);
            }
        }
        stream.activity.close(session, turn).await?;
        if native_runtime == NativeRuntime::CursorAgent
            && let Some(text) = cursor::complete_cursor_message(session, turn, &mut stream).await?
        {
            last_message = Some(text);
        }
        let cleanup = runtime.bridge.clear_turn().await;
        let outcome = match result {
            Ok(result) => {
                cleanup?;
                result
            }
            Err(error) => {
                if let Err(cleanup_error) = cleanup {
                    tracing::warn!(%cleanup_error, "Native bridge cleanup also failed");
                }
                if let Some(failure) = error.downcast_ref::<CursorRunFailed>() {
                    let result = failure.result();
                    if let Err(usage_error) = completion::record_cursor_usage(
                        session,
                        turn,
                        result["uuid"].as_str(),
                        cursor::reported_usage(result),
                    )
                    .await
                    {
                        tracing::warn!(%usage_error, "Cursor failed-run accounting also failed");
                    }
                    session
                        .send_token_count_event(turn, /*output_tokens_per_second*/ None)
                        .await;
                    if let Err(flush_error) = session.flush_rollout().await {
                        tracing::warn!(%flush_error, "Cursor failed-run rollout flush also failed");
                    }
                }
                return Err(error);
            }
        };
        if outcome.submitted_steering_count != stream.submitted_steering_count {
            bail!(
                "Claude did not submit all recorded steering messages; explicit recovery is required"
            );
        }
        complete_response(
            session,
            turn,
            runtime,
            native_runtime,
            stream,
            outcome,
            /*generation*/ None,
        )
        .await?;
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
        *input = materialize_native_inputs(session, turn, pending);
    }
}

fn materialize_native_inputs(
    session: &Session,
    turn: &TurnContext,
    entries: Vec<InputQueueEntry>,
) -> Vec<TurnInput> {
    let mut inputs = Vec::with_capacity(entries.len());
    for entry in entries {
        if let Some(context) = entry.additional_context {
            inputs.extend(context::submitted_additional_context(
                session, turn, &context,
            ));
        }
        inputs.push(entry.input);
    }
    inputs
}

async fn record_frame(
    native_runtime: NativeRuntime,
    session: &Session,
    turn: &TurnContext,
    frame: &Value,
    stream: &mut events::MessageStream,
) -> anyhow::Result<Option<String>> {
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
