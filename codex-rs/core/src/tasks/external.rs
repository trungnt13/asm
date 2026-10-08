//! Runs an external executor inside the native regular-turn lifecycle.

mod observations;
mod runtime;

pub(super) use runtime::interrupt;
pub(crate) use runtime::shutdown;

use std::collections::HashMap;
use std::hash::Hash;
use std::hash::Hasher;
use std::sync::Arc;

use codex_extension_api::ExternalAgentInput;
use codex_extension_api::ExternalAgentInputKind;
use codex_extension_api::ExternalAgentLaunch;
use codex_extension_api::ExternalAgentRuntime;
use codex_extension_api::ExternalObservation;
use codex_extension_api::TurnStartPhase;
use codex_protocol::ResponseItemId;
use codex_protocol::error::CodexErr;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::WarningEvent;
use codex_protocol::user_input::UserInput;
use codex_thread_store::PersistContext;
use tokio_util::sync::CancellationToken;

use crate::agent::api::AgentInput;
use crate::agent::api::AgentTarget;
use crate::agent::api::SendRequest;
use crate::agent::child_config::build_agent_resume_config;
use crate::agent::types::AgentMessage;
use crate::agent::types::MessageDeliveryMode;
use crate::hook_runtime::inspect_pending_input;
use crate::hook_runtime::record_additional_contexts;
use crate::hook_runtime::record_pending_input;
use crate::session::TurnInput;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::state::TaskKind;

use super::SessionTaskResult;

const MAX_FRAGMENT_BYTES: usize = 8_192;
const MAX_EVENT_BYTES: usize = 40_000;
const MAX_TURN_EVENTS: usize = 8_192;

#[tracing::instrument(level = "trace", skip_all)]
pub(super) async fn run(
    session: Arc<Session>,
    turn: Arc<TurnContext>,
    input: Vec<TurnInput>,
    cancellation: CancellationToken,
) -> SessionTaskResult {
    let result = run_turn(
        Arc::clone(&session),
        Arc::clone(&turn),
        input,
        cancellation.clone(),
    )
    .await;
    if result.is_err() && !cancellation.is_cancelled() {
        interrupt(&session, &turn).await?;
    }
    result
}

#[tracing::instrument(level = "trace", skip_all)]
async fn run_turn(
    session: Arc<Session>,
    turn: Arc<TurnContext>,
    mut input: Vec<TurnInput>,
    cancellation: CancellationToken,
) -> SessionTaskResult {
    session.emit_turn_started(&turn, TaskKind::Regular).await;
    session
        .emit_turn_start_lifecycle(
            &turn,
            /*token_usage_at_turn_start*/ None,
            TurnStartPhase::RegularTaskStart,
        )
        .await;
    let launch = match launch_request(&session, &turn).await {
        Ok(launch) => launch,
        Err(error) => {
            shutdown(&session).await;
            return Err(error);
        }
    };
    let runtime = runtime::open(&session, &turn, launch, &cancellation).await?;
    // Subscribe before draining so accepted input cannot be missed.
    let (mut activity_rx, _) = session
        .input_queue
        .subscribe_activity(/*turn_state*/ None)
        .await;
    let (pending, _) = session
        .input_queue
        .get_pending_input(&session.active_turn)
        .await;
    input.extend(pending);
    let mut task_count = submit_inputs(&session, &turn, runtime.as_ref(), input).await?;
    while task_count == 0 {
        let (pending, _) = session
            .input_queue
            .get_pending_input(&session.active_turn)
            .await;
        task_count = submit_inputs(&session, &turn, runtime.as_ref(), pending).await?;
        if task_count == 0
            && session
                .input_queue
                .close_external_turn_input(&session.active_turn, &turn.sub_id)
                .await
        {
            return Ok(None);
        }
        if cancellation.is_cancelled() {
            return Err(CodexErr::TurnAborted);
        }
    }
    let mut seen = HashMap::new();
    let mut event_count = 0;
    let mut observed = observations::ObservedItems::new(&session).await;
    'events: loop {
        let event = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(CodexErr::TurnAborted),
            result = activity_rx.changed() => {
                result.map_err(|_| invalid("external input activity closed"))?;
                let (input, _) = session.input_queue.get_pending_input(&session.active_turn).await;
                submit_inputs(&session, &turn, runtime.as_ref(), input).await?;
                continue;
            }
            event = runtime.next_event() => event.map_err(invalid)?,
        };
        event_count += 1;
        if event_count > MAX_TURN_EVENTS * 2 {
            return Err(invalid(
                "external turn exceeded its received observation limit",
            ));
        }
        let event =
            event.ok_or_else(|| invalid("external runtime closed before completing its turn"))?;
        if event.turn_id != turn.sub_id
            && !(event.turn_id.is_empty()
                && matches!(
                    event.kind,
                    ExternalObservation::Notice(_)
                        | ExternalObservation::Failed(_)
                        | ExternalObservation::Closed
                ))
        {
            continue;
        }
        if event.id.is_empty() || event.id.len() > 512 {
            return Err(invalid("external observation ID is empty or too long"));
        }
        let serialized =
            serde_json::to_string(&event).map_err(|_| invalid("invalid external observation"))?;
        if serialized.len() > MAX_EVENT_BYTES {
            return Err(invalid("external observation exceeds its transport limit"));
        }
        let mut fingerprint = std::hash::DefaultHasher::new();
        serialized.hash(&mut fingerprint);
        let fingerprint = fingerprint.finish();
        if let Some(previous) = seen.get(&event.id) {
            if previous != &fingerprint {
                return Err(invalid(
                    "external observation ID was reused with different content",
                ));
            }
            continue;
        }
        if seen.len() >= MAX_TURN_EVENTS {
            return Err(invalid("external turn exceeded its observation limit"));
        }
        seen.insert(event.id, fingerprint);
        match event.kind {
            ExternalObservation::ReadyToFinish { text } => {
                validate_fragment(&text)?;
                loop {
                    if cancellation.is_cancelled() {
                        return Err(CodexErr::TurnAborted);
                    }
                    session
                        .input_queue
                        .defer_mailbox_delivery_to_next_turn(&session.active_turn, &turn.sub_id)
                        .await;
                    let (input, _) = session
                        .input_queue
                        .get_pending_input(&session.active_turn)
                        .await;
                    if submit_inputs(&session, &turn, runtime.as_ref(), input).await? > 0 {
                        continue 'events;
                    }
                    if session
                        .input_queue
                        .close_external_turn_input(&session.active_turn, &turn.sub_id)
                        .await
                    {
                        break;
                    }
                    // Explicit work raced with the drain; retain this answer until it is admitted.
                    tokio::task::yield_now().await;
                }
                observed
                    .final_message(&session, &turn, text.clone())
                    .await?;
                return Ok(Some(text));
            }
            ExternalObservation::Usage(usage) => {
                session.update_token_usage_info(&turn, Some(&usage)).await?;
            }
            ExternalObservation::Notice(message) => {
                validate_fragment(&message)?;
                // Consent and permission requests must reach the orchestrator even
                // when the user is not viewing this child's transcript.
                if let Some(parent) = turn.session_source.parent_thread_id() {
                    let resume_config = build_agent_resume_config(&turn).map_err(invalid)?;
                    if let Err(error) = session
                        .services
                        .agent_control
                        .send(SendRequest {
                            caller: session.thread_id,
                            target: AgentTarget::Id(parent),
                            resume_config,
                            input: AgentInput::Message {
                                message: AgentMessage::Plaintext(message.clone()),
                                mode: MessageDeliveryMode::QueueOnly,
                            },
                            start_options: Default::default(),
                        })
                        .await
                    {
                        tracing::warn!(%error, "external runtime notice could not reach its parent");
                    }
                }
                session
                    .send_event(&turn, EventMsg::Warning(WarningEvent { message }))
                    .await;
            }
            ExternalObservation::Failed(error) => {
                validate_fragment(&error)?;
                return Err(invalid(error));
            }
            ExternalObservation::Closed => {
                return Err(invalid(
                    "external runtime closed before completing its turn",
                ));
            }
            observation @ (ExternalObservation::AssistantMessage { .. }
            | ExternalObservation::ToolStarted { .. }
            | ExternalObservation::ToolFinished { .. }) => {
                observed.publish(&session, &turn, observation).await?
            }
        }
    }
}

#[tracing::instrument(level = "trace", skip_all)]
async fn launch_request(
    session: &Session,
    turn: &TurnContext,
) -> Result<ExternalAgentLaunch, CodexErr> {
    crate::agent::external::validate_config(&turn.config).map_err(invalid)?;
    if turn.final_output_json_schema.is_some() || turn.cyber_access_program.is_some() {
        return Err(invalid(
            "external executors do not support output schemas or cyber access programs",
        ));
    }
    let mut environments = turn.initial_environments.turn_environments();
    let environment = environments
        .next()
        .ok_or_else(|| invalid("external executor needs one ready local environment"))?;
    if environments.next().is_some()
        || environment.environment.is_remote()
        || !matches!(
            environment.permission_profile(),
            PermissionProfile::Disabled
        )
        || environment.config().network_policy.is_some()
        || environment.config().exec_policy.is_some()
        || environment.config().mcp_policy.is_some()
        || environment.shell_environment_policy().use_profile
        || turn.network.is_some()
        || session.services.network_proxy.load_full().is_some()
    {
        return Err(invalid(
            "external executor requires one local full-access environment without managed networking",
        ));
    }
    let mut instructions = session
        .services
        .agents_md_manager
        .get_loaded()
        .await
        .map(|agents| agents.text())
        .unwrap_or_default();
    if let Some(developer) = &turn.developer_instructions {
        instructions.push_str("\n\nDeveloper instructions:\n");
        instructions.push_str(developer);
    }
    if instructions.len() > 65_536 {
        return Err(invalid(
            "external instruction bundle exceeds its 64 KiB limit",
        ));
    }
    let mut pages = Vec::new();
    let mut remaining = instructions.as_str();
    while !remaining.is_empty() {
        let mut end = remaining.len().min(8_000);
        while !remaining.is_char_boundary(end) {
            end -= 1;
        }
        pages.push(remaining[..end].to_owned());
        remaining = &remaining[end..];
    }
    let descriptor = turn
        .config
        .external_agent
        .as_ref()
        .ok_or_else(|| invalid("external descriptor is missing"))?;
    Ok(ExternalAgentLaunch {
        thread_id: session.thread_id,
        runtime_session_id: descriptor.runtime_session_id,
        cwd: environment
            .cwd()
            .to_abs_path()
            .map_err(|_| invalid("external working directory is not local"))?,
        state_dir: turn
            .config
            .codex_home
            .join("external-agents")
            .join(session.thread_id.to_string())
            .into_path_buf(),
        model: descriptor.model.clone(),
        env: crate::exec_env::create_env(
            environment.shell_environment_policy(),
            Some(session.thread_id),
        ),
        instructions: pages,
        mode: turn.config.external_agent_launch_mode,
    })
}

#[tracing::instrument(level = "trace", skip_all)]
async fn submit_inputs(
    session: &Arc<Session>,
    turn: &Arc<TurnContext>,
    runtime: &dyn ExternalAgentRuntime,
    input: Vec<TurnInput>,
) -> Result<usize, CodexErr> {
    let mut submitted = 0;
    for item in input {
        let (text, sender, kind) = match &item {
            TurnInput::UserInput { content, .. } => {
                let mut text = String::new();
                for input in content {
                    let UserInput::Text { text: part, .. } = input else {
                        return Err(invalid("external executor accepts only text input"));
                    };
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(part);
                }
                (text, "user".to_string(), ExternalAgentInputKind::Task)
            }
            TurnInput::InterAgentCommunication(mail) => {
                if mail.encrypted_content.is_some() {
                    return Err(invalid(
                        "external executor cannot consume encrypted messages",
                    ));
                }
                let kind = if mail.trigger_turn {
                    ExternalAgentInputKind::Task
                } else {
                    ExternalAgentInputKind::Message
                };
                (mail.content.clone(), mail.author.to_string(), kind)
            }
            TurnInput::ResponseItem(_) | TurnInput::FunctionCallOutput(_) => {
                return Err(invalid(
                    "external executor cannot consume native response items",
                ));
            }
        };
        validate_fragment(&text)?;
        let outcome = inspect_pending_input(session, turn, &item).await;
        if outcome.should_stop {
            record_additional_contexts(session, turn, outcome.additional_contexts).await;
            continue;
        }
        let contexts = outcome.additional_contexts;
        let mut text = text;
        for context in &contexts {
            text.push_str("\n\n");
            text.push_str(context);
        }
        validate_fragment(&text)?;
        record_pending_input(
            session,
            turn,
            &turn.capture_current_model_info(),
            item,
            contexts,
            PersistContext::TurnStart,
        )
        .await;
        let triggers_task = matches!(kind, ExternalAgentInputKind::Task);
        runtime
            .submit(ExternalAgentInput {
                turn_id: turn.sub_id.clone(),
                id: ResponseItemId::new("external_input").to_string(),
                sender,
                text,
                kind,
            })
            .await
            .map_err(invalid)?;
        if triggers_task {
            submitted += 1;
        }
    }
    Ok(submitted)
}

fn validate_fragment(text: &str) -> Result<(), CodexErr> {
    if text.len() > MAX_FRAGMENT_BYTES {
        return Err(invalid("external fragment exceeds its 8192-byte limit"));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> CodexErr {
    CodexErr::InvalidRequest(message.into())
}
