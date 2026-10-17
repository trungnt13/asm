//! Thread-scoped adapter for the official CLI; native ASM retains tool authority.
mod bridge;
mod events;
mod state;
use events::record_assistant_frame;
use state::Checkpoint;
use state::ClaudeSession;
use state::PermissionSnapshot;
use state::RuntimeLease;
pub(crate) use state::RuntimeSlot;
use state::SystemPolicy;

use std::io::Write;
use std::sync::Arc;

use anyhow::Context;
use anyhow::bail;
use codex_async_utils::OrCancelExt;
use codex_protocol::error::CodexErr;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ContentItemProvenance;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TokenUsage;
use codex_protocol::user_input::UserInput;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::hook_runtime::run_pending_session_start_hooks;
use crate::session::TokenUsageContext;
use crate::session::TurnInput;
use crate::session::session::Session;
use crate::session::turn::run_hooks_and_record_inputs;
use crate::session::turn_context::TurnContext;
use crate::state::TaskKind;
use crate::tasks::SessionTaskResult;
use bridge::ClaudeToolBridge;

pub(crate) fn is_claude_model(model: &str) -> bool {
    model.starts_with("claude-code/")
}

#[tracing::instrument(skip_all)]
pub(crate) async fn shutdown(session: &Session) {
    let runtime = match session.claude_runtime.take_for_shutdown().await {
        Ok(runtime) => runtime,
        Err(error) => {
            tracing::warn!(%error, "Claude runtime lease shutdown failed");
            return;
        }
    };
    if let Some(mut runtime) = runtime {
        if let Err(error) = runtime.bridge.clear_turn().await {
            tracing::warn!(%error, "Claude tool drain failed");
        }
        if let Err(error) = runtime.runtime.shutdown().await {
            tracing::warn!(%error, "Claude runtime shutdown failed");
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
                tracing::warn!(%error, "could not retain Claude diagnostics");
            }
        }
        if let Err(error) = runtime.bridge.shutdown().await {
            tracing::warn!(%error, "Claude bridge shutdown failed");
        }
    }
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run_turn(
    session: Arc<Session>,
    turn: Arc<TurnContext>,
    mut input: Vec<TurnInput>,
    cancellation: CancellationToken,
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
    session.request_mcp_runtime_reprojection();
    if preparation.is_err() {
        return Err(CodexErr::TurnAborted);
    }
    let result = run_conversation(&session, &turn, &mut input, &cancellation).await;
    if result.is_err() {
        shutdown(&session).await;
    }
    match result {
        Ok(text) => Ok(text),
        Err(_) if cancellation.is_cancelled() => Err(CodexErr::TurnAborted),
        Err(error) => Err(CodexErr::InvalidRequest(format!(
            "Claude runtime: {error:#}"
        ))),
    }
}

#[tracing::instrument(skip_all)]
async fn run_conversation(
    session: &Arc<Session>,
    turn: &Arc<TurnContext>,
    input: &mut Vec<TurnInput>,
    cancellation: &CancellationToken,
) -> anyhow::Result<Option<String>> {
    if turn.final_output_json_schema.is_some() {
        bail!("structured output is not supported; start a native-model chat");
    }
    let mut last_message = None;
    let mut stop_hook_active = false;
    loop {
        if cancellation.is_cancelled() {
            bail!("turn interrupted");
        }
        for item in input.iter() {
            if let TurnInput::UserInput { content, .. } = item
                && content
                    .iter()
                    .any(|part| !matches!(part, UserInput::Text { .. }))
            {
                bail!(
                    "only text input is supported; images, skills, and attachments require a native model"
                );
            }
        }
        let effort = turn
            .effective_reasoning_effort()
            .map(|effort| effort.to_string());
        if effort
            .as_deref()
            .is_some_and(|value| !matches!(value, "low" | "medium" | "high" | "xhigh" | "max"))
        {
            bail!("unsupported Claude effort; use low, medium, high, xhigh, or max");
        }
        let original_history_count = session.clone_history().await.raw_items().count();
        let checkpoint_path = turn
            .config
            .codex_home
            .join("claude-runtime")
            .join(format!("{}.json", session.thread_id()));
        let previous_checkpoint = match session.claude_runtime.checkpoint()? {
            Some(checkpoint) => Some(checkpoint),
            None if checkpoint_path.exists() => Some(serde_json::from_slice::<Checkpoint>(
                &std::fs::read(&checkpoint_path)?,
            )?),
            None => None,
        };
        let previous_count = previous_checkpoint
            .as_ref()
            .map_or(0, |checkpoint| checkpoint.delivered_items);
        let expected_prefix = previous_checkpoint
            .as_ref()
            .map_or("", |checkpoint| checkpoint.delivered_prefix.as_str());
        let original_history = session.clone_history().await;
        if previous_count > original_history_count
            || previous_count > 0
                && history_prefix(&original_history, previous_count)? != expected_prefix
        {
            bail!("saved ASM context does not match the Claude checkpoint; refusing replay");
        }
        let step = session
            .capture_step_context(Arc::clone(turn), cancellation)
            .await?;
        if step.environments.turn_environments().count() != 1
            || step.environments.local_environment_cwd().is_none()
        {
            bail!("Claude runtime requires exactly one local environment");
        }
        let cwd = step
            .environments
            .local_environment_cwd()
            .context("Claude requires a local working directory")?
            .to_path_buf();
        let world = session
            .record_context_updates_and_set_reference_context_item(&step)
            .await?;
        let base = session.get_prompt_base_instructions().await.text;
        let (initial_context, snapshot) = session
            .build_initial_context_with_world_state(&step, &world)
            .await;
        let permissions = snapshot
            .into_object()
            .remove("permissions")
            .map(serde_json::from_value::<PermissionSnapshot>)
            .transpose()?;
        let mut instructions = base.clone();
        let mut stable_messages = Vec::new();
        let mut developer_parts = Vec::new();
        for item in crate::context_manager::updates::merge_world_state_updates(initial_context) {
            let ResponseItem::Message {
                role,
                content,
                internal_chat_message_metadata_passthrough: metadata,
                ..
            } = item
            else {
                continue;
            };
            if role != "developer" {
                continue;
            }
            let mut texts = Vec::new();
            let mut stable_parts = Vec::new();
            for (index, content) in content.into_iter().enumerate() {
                let ContentItem::InputText { text } = content else {
                    bail!("non-text developer policy is unsupported");
                };
                let kind = metadata
                    .as_ref()
                    .and_then(|metadata| metadata.content_item_kinds.as_ref())
                    .and_then(|kinds| kinds.get(index))
                    .map(|kind| kind.as_str().to_owned());
                let provenance = metadata
                    .as_ref()
                    .and_then(|metadata| metadata.content_item_metadata.as_ref())
                    .and_then(|parts| parts.get(index))
                    .map(|part| part.provenance.clone());
                let stable_text = if kind.as_deref() == Some("permissions.instructions")
                    && matches!(
                        provenance,
                        Some(ContentItemProvenance::Harness { hook: false })
                    ) {
                    permissions
                        .as_ref()
                        .context("missing typed permissions snapshot")?
                        .instructions
                        .clone()
                } else {
                    text.clone()
                };
                stable_parts.push((kind.clone(), provenance.clone(), stable_text));
                developer_parts.push((kind, provenance, text.clone()));
                texts.push(text);
            }
            instructions.push_str("\n\n");
            instructions.push_str(&texts.join("\n"));
            stable_messages.push(stable_parts);
        }
        let system_policy = SystemPolicy {
            stable_instructions: serde_json::to_string(&(base, stable_messages))?,
            approved_prefixes: permissions
                .map(|permissions| permissions.approved_command_prefixes)
                .unwrap_or_default(),
        };
        if let Some(checkpoint) = &previous_checkpoint {
            checkpoint.permits_system(&instructions, &system_policy)?;
        }
        if codex_utils_output_truncation::approx_token_count(&instructions) > 10_000 {
            bail!("Claude system instructions exceed the 10,000 approximate tokens");
        }
        if run_pending_session_start_hooks(session, turn).await
            || run_hooks_and_record_inputs(
                session,
                turn,
                &step.settings.model_info,
                input,
                codex_thread_store::PersistContext::TurnStart,
            )
            .await
        {
            return Ok(last_message);
        }
        let history = session.clone_history().await;
        let mut prompt = String::new();
        for item in history.raw_items().skip(previous_count) {
            match item {
                ResponseItem::Message { role, .. } if role == "assistant" => {}
                ResponseItem::Message {
                    role,
                    content,
                    internal_chat_message_metadata_passthrough: metadata,
                    ..
                } if role == "developer" => {
                    for (index, part) in content.iter().enumerate() {
                        let ContentItem::InputText { text } = part else {
                            bail!("non-text developer policy is unsupported");
                        };
                        let kind = metadata
                            .as_ref()
                            .and_then(|metadata| metadata.content_item_kinds.as_ref())
                            .and_then(|kinds| kinds.get(index))
                            .map(|kind| kind.as_str().to_owned());
                        let provenance = metadata
                            .as_ref()
                            .and_then(|metadata| metadata.content_item_metadata.as_ref())
                            .and_then(|parts| parts.get(index))
                            .map(|part| part.provenance.clone());
                        let saved_prefix = kind.as_deref()
                            == Some("permissions.approved_command_prefix_saved")
                            && matches!(
                                provenance,
                                Some(ContentItemProvenance::Harness { hook: false })
                            )
                            && previous_checkpoint
                                .as_ref()
                                .and_then(|checkpoint| checkpoint.system_policy.as_ref())
                                .is_some_and(|previous| {
                                    previous.verifies_receipt(&system_policy, text)
                                });
                        if !saved_prefix
                            && !developer_parts.contains(&(kind, provenance, text.clone()))
                        {
                            bail!(
                                "dynamic developer instructions cannot be delivered at user priority; start a new chat"
                            );
                        }
                    }
                }
                ResponseItem::Message { role, content, .. } => {
                    prompt.push_str(&format!("<{role}>\n"));
                    for content in content {
                        match content {
                            ContentItem::InputText { text }
                            | ContentItem::OutputText { text, .. } => {
                                prompt.push_str(text);
                                prompt.push('\n');
                            }
                            _ => bail!("non-text context cannot be delivered to Claude"),
                        }
                    }
                    prompt.push_str(&format!("</{role}>\n"));
                }
                ResponseItem::FunctionCall { .. }
                | ResponseItem::CustomToolCall { .. }
                | ResponseItem::CustomToolCallOutput { .. }
                | ResponseItem::FunctionCallOutput {
                    call_id: Some(_), ..
                } => {}
                ResponseItem::FunctionCallOutput { output, .. } => {
                    prompt.push_str(&serde_json::to_string(output)?);
                    prompt.push('\n');
                }
                _ => bail!("unsupported injected context; no input was sent to Claude"),
            }
        }
        if prompt.trim().is_empty() {
            return Ok(last_message);
        }
        if codex_utils_output_truncation::approx_token_count(&prompt) > 10_000 {
            bail!(
                "Claude input exceeds the 10,000 approximate tokens per context item; shorten the input or use a native model"
            );
        }
        let authority = json!({
            "tools": &*step.tool_router.model_visible_specs(),
            "permissions": turn.permission_profile(),
            "approval": turn.approval_policy(),
            "developer_instructions": turn.developer_instructions,
        });
        let mut lease = RuntimeLease::take(&session.claude_runtime)?;
        let needs_binding = lease.runtime.is_some();
        if lease.runtime.is_none() {
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
                .context("Claude session is already owned by another ASM process")?;
            let (checkpoint, claude_session) = match std::fs::read(&checkpoint_path) {
                Ok(bytes) => {
                    let mut checkpoint: Checkpoint = serde_json::from_slice(&bytes)?;
                    if checkpoint.in_flight {
                        bail!(
                            "previous turn has an uncertain outcome; automatic replay/resume is unsafe. Start a new chat"
                        );
                    }
                    if checkpoint.model != step.settings.model_info.slug
                        || checkpoint.effort != effort
                        || checkpoint.cwd != cwd
                        || checkpoint.authority != authority
                    {
                        bail!(
                            "saved Claude model, directory, or instructions changed; start a new chat"
                        );
                    }
                    checkpoint.permits_system(&instructions, &system_policy)?;
                    checkpoint.instructions = instructions.clone();
                    checkpoint.system_policy = Some(system_policy.clone());
                    let resume = codex_claude_runtime::ClaudeSession::Resume {
                        id: checkpoint.session_id.clone(),
                    };
                    (checkpoint, resume)
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    // Existing assistant history without a linkage cannot be replayed safely.
                    if history
                        .raw_items()
                        .take(original_history_count)
                        .any(|item| {
                            matches!(item,
                        ResponseItem::Message { role, .. } if role == "assistant")
                        })
                    {
                        bail!(
                            "missing Claude session linkage for existing history; start a new chat"
                        );
                    }
                    let id = session.thread_id().to_string();
                    (
                        Checkpoint {
                            model: step.settings.model_info.slug.clone(),
                            effort: effort.clone(),
                            cwd: cwd.clone(),
                            session_id: id.clone(),
                            instructions: instructions.clone(),
                            authority: authority.clone(),
                            system_policy: Some(system_policy.clone()),
                            in_flight: false,
                            delivered_items: 0,
                            delivered_prefix: String::new(),
                        },
                        codex_claude_runtime::ClaudeSession::New { id },
                    )
                }
                Err(error) => return Err(error.into()),
            };
            let bridge = ClaudeToolBridge::start().await?;
            bridge
                .set_turn(Arc::clone(session), Arc::clone(&step), cancellation.clone())
                .await?;
            let runtime =
                ClaudeSession::spawn_runtime(&bridge, &checkpoint, claude_session).await?;
            lease.runtime = Some(ClaudeSession {
                runtime,
                bridge,
                checkpoint,
                checkpoint_path,
                _lock: lock,
                diagnostic_capture: turn.config.diagnostic_log_capture,
            });
        }
        let runtime = lease
            .runtime
            .as_mut()
            .context("Claude session was not initialized")?;
        runtime
            .checkpoint
            .permits_system(&instructions, &system_policy)?;
        if runtime.checkpoint.authority != authority {
            bail!("Claude tool or permission configuration changed; start a new chat");
        }
        if runtime.checkpoint.in_flight {
            bail!(
                "previous Claude turn was interrupted or failed; start a new chat to avoid unsafe replay"
            );
        }
        let refresh_system = runtime.checkpoint.instructions != instructions;
        if refresh_system {
            runtime.runtime.shutdown().await?;
        }
        if needs_binding {
            runtime
                .bridge
                .set_turn(Arc::clone(session), Arc::clone(&step), cancellation.clone())
                .await?;
        }
        if refresh_system {
            // Only a clean result and drained bridge can reach this boundary. Resume the same
            // Claude history with new system policy, never replay a prompt or tool call.
            let previous_instructions =
                std::mem::replace(&mut runtime.checkpoint.instructions, instructions.clone());
            let resumed = ClaudeSession::spawn_runtime(
                &runtime.bridge,
                &runtime.checkpoint,
                codex_claude_runtime::ClaudeSession::Resume {
                    id: runtime.checkpoint.session_id.clone(),
                },
            )
            .await;
            match resumed {
                Ok(resumed) => runtime.runtime = resumed,
                Err(error) => {
                    runtime.checkpoint.instructions = previous_instructions;
                    return Err(error);
                }
            }
        }
        runtime.checkpoint.system_policy = Some(system_policy);
        runtime.checkpoint.delivered_items = history.raw_items().count();
        runtime.checkpoint.delivered_prefix =
            history_prefix(&history, runtime.checkpoint.delivered_items)?;
        runtime.checkpoint.in_flight = true;
        runtime.save_checkpoint()?;
        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(/*buffer*/ 32);
        let mut stream = events::MessageStream::default();
        let result = {
            let run = runtime
                .runtime
                .run_turn(&prompt, events_tx, cancellation.clone());
            tokio::pin!(run);
            loop {
                tokio::select! {
                    result = &mut run => break result,
                    Some(frame) = events_rx.recv() => {
                        if let Some(text) = record_assistant_frame(session, turn, &frame, &mut stream).await? {
                            last_message = Some(text);
                        }
                    }
                }
            }
        };
        while let Ok(frame) = events_rx.try_recv() {
            if let Some(text) = record_assistant_frame(session, turn, &frame, &mut stream).await? {
                last_message = Some(text);
            }
        }
        let cleanup = runtime.bridge.clear_turn().await;
        let result = match result {
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
        let usage = &result["usage"];
        let input_tokens = usage["input_tokens"].as_i64().unwrap_or_default();
        let cached = usage["cache_read_input_tokens"]
            .as_i64()
            .unwrap_or_default();
        let written = usage["cache_creation_input_tokens"]
            .as_i64()
            .unwrap_or_default();
        let output = usage["output_tokens"].as_i64().unwrap_or_default();
        let usage = TokenUsage {
            input_tokens: input_tokens + cached + written,
            cached_input_tokens: cached,
            cache_write_input_tokens: written,
            output_tokens: output,
            reasoning_output_tokens: usage["output_tokens_details"]["thinking_tokens"]
                .as_i64()
                .unwrap_or_default(),
            total_tokens: input_tokens + cached + written + output,
            codex_rollout_budget_units: None,
        };
        let response_id = result["uuid"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        session
            .record_observed_response_completed(
                turn,
                &response_id,
                Some(&usage),
                /*usage_metadata*/ None,
            )
            .await;
        let budget_result = session
            .record_token_usage_info(
                turn,
                &step.settings,
                Some(&usage),
                TokenUsageContext::Sampling,
            )
            .await;
        session
            .send_token_count_event(turn, /*output_tokens_per_second*/ None)
            .await;
        budget_result?;
        session.flush_rollout().await?;
        runtime.checkpoint.in_flight = false;
        runtime.save_checkpoint()?;
        drop(lease);
        let stop = crate::hook_runtime::run_turn_stop_hooks(
            session,
            &step,
            stop_hook_active,
            last_message.clone(),
        )
        .await;
        if stop.should_stop {
            return Ok(last_message);
        }
        if stop.should_block {
            let message =
                codex_protocol::items::build_hook_prompt_message(&stop.continuation_fragments)
                    .context("Stop hook blocked completion without continuation instructions")?;
            *input = vec![TurnInput::ResponseItem(message.into())];
            stop_hook_active = true;
            continue;
        }
        session
            .input_queue
            .accept_mailbox_delivery_for_current_turn(&session.active_turn, &turn.sub_id)
            .await;
        let (pending, _) = session
            .input_queue
            .get_pending_input(&session.active_turn)
            .await;
        *input = pending;
        // Re-enter once even without queued input: tool hooks may have appended context.
        // An empty prompt at the next boundary ends the turn without another CLI request.
    }
}

fn history_prefix(
    history: &crate::context_manager::ContextManager,
    count: usize,
) -> anyhow::Result<String> {
    use sha1::Digest;
    let items = history.raw_items().take(count).collect::<Vec<_>>();
    Ok(format!(
        "{:x}",
        sha1::Sha1::digest(serde_json::to_vec(&items)?)
    ))
}
