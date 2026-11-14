use super::ConfirmedInterrupt;
use super::ExternalSession;
use super::RuntimeProcess;
use super::events;
use crate::session::TokenUsageContext;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_protocol::openai_models::NativeRuntime;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::WarningEvent;
use serde_json::Value;

#[tracing::instrument(skip_all)]
pub(super) async fn complete_response(
    session: &Session,
    turn: &TurnContext,
    runtime: &mut ExternalSession,
    native_runtime: NativeRuntime,
    mut stream: events::MessageStream,
    outcome: codex_external_runtime::ExternalTurnOutcome,
    generation: Option<u64>,
) -> anyhow::Result<()> {
    let interrupted = outcome.interrupted;
    let failure = outcome.failure;
    let has_result = outcome.result.is_some();
    let result = outcome.result.unwrap_or(Value::Null);
    let settings = &turn.initial_settings;
    let cursor = native_runtime == NativeRuntime::CursorAgent;
    if has_result && !cursor && stream.results.is_empty() {
        stream.results.push(result.clone());
    }
    let mut budget_error = None;
    for response in &stream.results {
        let usage = runtime.measure_usage_delta(response);
        if usage.is_none() {
            session.send_event(turn, EventMsg::Warning(WarningEvent {
                message: "Claude token usage is unavailable for this response: the first result after resume or missing/reset cumulative counters cannot be counted safely.".to_owned(),
            })).await;
        }
        let response_id = response["uuid"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        session
            .record_observed_response_completed(
                turn,
                &response_id,
                usage.as_ref(),
                /*usage_metadata*/ None,
            )
            .await;
        if let Err(error) = session
            .record_token_usage_info(
                turn,
                settings,
                usage.as_ref(),
                TokenUsageContext::SpendingOnly,
            )
            .await
        {
            budget_error.get_or_insert(error);
        }
    }
    if !has_result && !cursor {
        runtime.usage_baseline = None;
    }
    if cursor {
        runtime.usage_baseline = None;
        if has_result {
            let response_id = result["uuid"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            session
                .record_observed_response_completed(
                    turn,
                    &response_id,
                    /*usage*/ None,
                    /*usage_metadata*/ None,
                )
                .await;
        }
        budget_error = session
            .record_token_usage_info(
                turn,
                settings,
                /*usage*/ None,
                TokenUsageContext::SpendingOnly,
            )
            .await
            .err();
    }
    let budget_result = budget_error.map_or(Ok(()), Err);
    let model_context_window = stream.context.as_ref().and_then(|context| {
        result["modelUsage"].get(&context.model)?["contextWindow"]
            .as_i64()
            .filter(|window| *window > 0 && context.usage.input_tokens <= *window)
    });
    if !cursor && (stream.context.is_none() || model_context_window.is_none()) {
        session.send_event(turn, EventMsg::Warning(WarningEvent {
                message: "Claude context fill is unavailable: the latest main request usage or its model context window was not reported.".to_owned(),
            })).await;
    }
    session
        .set_reported_context_usage(
            stream
                .context
                .map(|context| context.usage)
                .unwrap_or_default(),
            model_context_window,
        )
        .await;
    session
        .send_token_count_event(turn, stream.output_tps)
        .await;
    session.flush_rollout().await?;
    save_reconciled_checkpoint(runtime, generation, !interrupted && failure.is_none())?;
    if interrupted {
        if let Err(error) = budget_result {
            tracing::warn!(%error, "Native interrupted-turn accounting failed");
        }
        return Err(ConfirmedInterrupt.into());
    }
    if let Some(failure) = failure {
        if let Err(error) = budget_result {
            tracing::warn!(%error, "Native failed-turn accounting failed");
        }
        return Err(failure.into());
    }
    budget_result?;
    Ok(())
}

pub(super) fn save_reconciled_checkpoint(
    runtime: &mut ExternalSession,
    generation: Option<u64>,
    successful: bool,
) -> anyhow::Result<()> {
    runtime.foreground_idle = true;
    let checkpoint = &mut runtime.checkpoint;
    let linkage = &runtime.linkage;
    let path = &runtime.checkpoint_path;
    match &runtime.runtime {
        RuntimeProcess::Claude(process) => process.commit_projection(
            generation.unwrap_or_else(|| process.completed_generation()),
            |clean, fork_point| {
                checkpoint.in_flight = !clean;
                checkpoint.fork_point = if successful { fork_point } else { None };
                linkage.save(checkpoint, path)
            },
        ),
        RuntimeProcess::Cursor(_) => {
            checkpoint.in_flight = false;
            checkpoint.fork_point = None;
            linkage.save(checkpoint, path)
        }
    }
}
