use super::session::Session;
use super::step_settings::ResolvedStepSettings;
use super::turn_context::TurnContext;
use codex_api::DecisionsClient;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::TurnSettingsUpdateOutcome;
use std::sync::Arc;
use std::sync::Weak;
use tokio::sync::Notify;
use tokio::sync::oneshot;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;

pub(super) struct DecisionRequest {
    pub(super) model: String,
    pub(super) input: String,
    pub(super) instructions: String,
    pub(super) choices: Vec<String>,
}

pub(super) struct DecisionJob {
    pub(super) handle: AbortOnDropHandle<()>,
}

pub(super) enum DecisionTarget {
    Discard,
    Adaptive {
        session: Weak<Session>,
        turn: Weak<TurnContext>,
        expected: Arc<ResolvedStepSettings>,
        task_done: Arc<Notify>,
    },
}

pub(super) struct AdaptiveDecisionGuard {
    pub(super) freshness: CancellationToken,
    pub(super) deadline: Instant,
}

pub(super) struct PendingAdaptiveDecision {
    pub(super) job: DecisionJob,
    pub(super) cancellation: CancellationToken,
}

impl Drop for PendingAdaptiveDecision {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

impl DecisionJob {
    pub(super) fn start(
        client: Arc<DecisionsClient>,
        request: DecisionRequest,
        cancellation: CancellationToken,
        guard: AdaptiveDecisionGuard,
        target: DecisionTarget,
    ) -> (Self, oneshot::Receiver<()>) {
        let (completion, completed) = oneshot::channel();
        let handle = AbortOnDropHandle::new(tokio::spawn(run_decision(
            client,
            request,
            cancellation,
            guard,
            target,
            completion,
        )));
        (Self { handle }, completed)
    }
}

#[tracing::instrument(skip_all)]
async fn run_decision(
    client: Arc<DecisionsClient>,
    request: DecisionRequest,
    cancellation: CancellationToken,
    guard: AdaptiveDecisionGuard,
    target: DecisionTarget,
    completion: oneshot::Sender<()>,
) {
    let started = Instant::now();
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => {},
        _ = guard.freshness.cancelled() => {},
        result = tokio::time::timeout_at(guard.deadline, async {
            let result = client.choose_effort(
                &request.model, &request.input, &request.instructions, &request.choices,
            ).await;
            tracing::debug!(success = result.is_ok(), selected_effort = ?result.as_ref().ok(),
                latency_ms = started.elapsed().as_millis(),
                "adaptive reasoning request completed");
            let selected = match result {
                Ok(selected) => selected,
                Err(error) => {
                    tracing::debug!(%error, "adaptive reasoning fallback");
                    return;
                }
            };
            let (session, turn, expected, task_done) = match target {
                DecisionTarget::Discard => return,
                DecisionTarget::Adaptive { session, turn, expected, task_done } => {
                    let (Some(session), Some(turn)) = (session.upgrade(), turn.upgrade()) else {
                        return;
                    };
                    (session, turn, expected, task_done)
                }
            };
            let Ok(effort) = selected.parse::<ReasoningEffort>() else {
                return;
            };
            if matches!(effort, ReasoningEffort::Ultra | ReasoningEffort::Persistent)
                || expected.effective_reasoning_effort().as_ref() == Some(&effort)
            {
                return;
            }
            let outcome = session.apply_adaptive_turn_settings(
                &turn, &expected, &task_done, &guard, effort.clone(),
            ).await;
            tracing::debug!(%effort, applied = outcome == TurnSettingsUpdateOutcome::Applied,
                latency_ms = started.elapsed().as_millis(), "adaptive reasoning decision");
        }) => {
            if result.is_err() {
                tracing::debug!("adaptive reasoning request deadline");
            }
        }
    }
    let _ = completion.send(());
}
