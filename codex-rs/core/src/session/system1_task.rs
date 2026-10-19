use super::decision_log::DecisionApiOutcome;
use super::decision_log::DecisionAttempt;
use super::decision_log::DecisionDisposition;
use super::session::Session;
use super::step_settings::ResolvedStepSettings;
use super::turn_context::TurnContext;
use codex_api::DecisionsClient;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::System1Error;
use codex_protocol::protocol::TurnSettingsUpdateOutcome;
use std::sync::Arc;
use std::sync::Weak;
use tokio::sync::Notify;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

pub(super) const WARMUP_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(/*secs*/ 3);

pub(super) struct DecisionRequest {
    pub(super) model: String,
    pub(super) input: String,
    pub(super) instructions: String,
    pub(super) choices: Vec<(String, String)>,
}

pub(super) enum DecisionTarget {
    Warmup {
        session: Weak<Session>,
        generation: u64,
    },
    System1 {
        session: Weak<Session>,
        turn: Weak<TurnContext>,
        expected: Arc<ResolvedStepSettings>,
        task_done: Arc<Notify>,
        generation: u64,
    },
}

pub(super) struct System1DecisionGuard {
    pub(super) freshness: CancellationToken,
    pub(super) deadline: Instant,
}

#[tracing::instrument(skip_all, fields(thread.id = tracing::field::Empty, turn.id = tracing::field::Empty, kind = tracing::field::Empty))]
pub(super) async fn run_decision(
    client: Arc<DecisionsClient>,
    request: DecisionRequest,
    cancellation: CancellationToken,
    guard: System1DecisionGuard,
    target: DecisionTarget,
    mut attempt: Option<DecisionAttempt>,
) {
    let span = tracing::Span::current();
    let (session, generation) = match &target {
        DecisionTarget::Warmup {
            session,
            generation,
        } => {
            span.record("kind", "warmup");
            (session.clone(), Some(*generation))
        }
        DecisionTarget::System1 {
            session,
            turn,
            generation,
            ..
        } => {
            span.record("kind", "step");
            if let Some(turn) = turn.upgrade() {
                span.record("turn.id", &turn.sub_id);
            }
            (session.clone(), Some(*generation))
        }
    };
    if let Some(session) = session.upgrade() {
        span.record("thread.id", session.thread_id().to_string());
    }
    let started = Instant::now();
    let timeout = guard.deadline.saturating_duration_since(started);
    tracing::debug!(
        input_bytes = request.input.len(),
        instructions_bytes = request.instructions.len(),
        "System1 request started"
    );
    let (api, disposition, selected, error) = tokio::select! {
        biased;
        _ = cancellation.cancelled() => (DecisionApiOutcome::Cancelled, DecisionDisposition::Cancelled, None, None),
        _ = guard.freshness.cancelled() => (DecisionApiOutcome::Cancelled, DecisionDisposition::Superseded, None, None),
        result = tokio::time::timeout_at(guard.deadline,
            async {
                if let Some(attempt) = &mut attempt {
                    attempt.mark_http_attempted(client.backend_name());
                }
                client.choose_effort(&request.model, &request.input, &request.instructions, &request.choices).await
            }
        ) => match result {
            Err(_) => {
                tracing::warn!(
                    backend = client.backend_name(),
                    model = %request.model,
                    elapsed_ms = started.elapsed().as_millis(),
                    remaining_deadline_at_start_ms = timeout.as_millis(),
                    "System1 request timed out"
                );
                (DecisionApiOutcome::Timeout, DecisionDisposition::TargetUnavailable, None, Some(System1Error::Timeout))
            },
            Ok(Err(error)) => (DecisionApiOutcome::Error(error), DecisionDisposition::TargetUnavailable, None, Some(super::system1::health::classify_error(error))),
            Ok(Ok(selected)) => {
                tracing::debug!(selected_effort = %selected, latency_ms = started.elapsed().as_millis(),
                    "System1 request completed");
                let effort = selected.parse::<ReasoningEffort>().ok().filter(|effort|
                    !matches!(effort, ReasoningEffort::Ultra | ReasoningEffort::Persistent)
                );
                match &target {
                    DecisionTarget::Warmup { .. } => (DecisionApiOutcome::Success, DecisionDisposition::Warmup, effort, None),
                    DecisionTarget::System1 { turn, expected, task_done, .. } => {
                        if let (Some(session), Some(turn), Some(effort)) = (session.upgrade(), turn.upgrade(), effort) {
                            let unchanged = expected.effective_reasoning_effort().as_ref() == Some(&effort);
                            let outcome = session.apply_system1_turn_settings(
                                &turn, expected, task_done, &guard, effort.clone(),
                            ).await;
                            tracing::debug!(%effort, applied = outcome == TurnSettingsUpdateOutcome::Applied,
                                latency_ms = started.elapsed().as_millis(), "System1 decision");
                            let (disposition, error) = match outcome {
                                TurnSettingsUpdateOutcome::Applied => (if unchanged { DecisionDisposition::Unchanged } else { DecisionDisposition::Applied }, None),
                                TurnSettingsUpdateOutcome::Rejected { .. } => (DecisionDisposition::Rejected, Some(System1Error::Rejected)),
                                TurnSettingsUpdateOutcome::TargetUnavailable => (DecisionDisposition::TargetUnavailable,
                                    (Instant::now() >= guard.deadline).then_some(System1Error::Timeout)),
                            };
                            (DecisionApiOutcome::Success, disposition, Some(effort), error)
                        } else {
                            (DecisionApiOutcome::Success, DecisionDisposition::TargetUnavailable, None, None)
                        }
                    }
                }
            }
        }
    };
    let error = error.filter(|_| !cancellation.is_cancelled() && !guard.freshness.is_cancelled());
    if matches!(target, DecisionTarget::System1 { .. })
        && let Some(session) = session.upgrade()
    {
        session.record_system1_decision(disposition, error, started.elapsed());
    }
    if let Some(session) = session.upgrade()
        && let Some(error) = error
    {
        session
            .publish_system1_error(
                if matches!(target, DecisionTarget::Warmup { .. }) && error == System1Error::Timeout
                {
                    super::system1::health::System1HealthUpdate::WarmupTimeout
                } else {
                    super::system1::health::System1HealthUpdate::Decision(Some(error))
                },
                generation,
                Some(super::system1::health::System1DecisionHealthGuard {
                    freshness: &guard.freshness,
                    cancellation: &cancellation,
                }),
            )
            .await;
    }
    if let Some(attempt) = attempt {
        attempt.finish(api, disposition, selected);
    }
}
