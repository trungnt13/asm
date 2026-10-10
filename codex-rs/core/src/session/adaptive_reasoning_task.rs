use super::step_settings::ResolvedStepSettings;
use codex_api::DecisionsClient;
use codex_api::DecisionsError;
use std::sync::Arc;
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
    pub(super) result: oneshot::Receiver<Result<String, DecisionsError>>,
}

pub(super) struct AdaptiveDecisionGuard {
    pub(super) freshness: CancellationToken,
    pub(super) deadline: Instant,
}

pub(super) struct PendingAdaptiveDecision {
    pub(super) job: DecisionJob,
    pub(super) expected: Arc<ResolvedStepSettings>,
    pub(super) task_done: Arc<Notify>,
    pub(super) cancellation: CancellationToken,
    pub(super) guard: AdaptiveDecisionGuard,
    pub(super) started: Instant,
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
        guard: &AdaptiveDecisionGuard,
    ) -> Self {
        let (sender, result) = oneshot::channel();
        let handle = AbortOnDropHandle::new(tokio::spawn(run_decision(
            client,
            request,
            cancellation,
            guard.freshness.clone(),
            guard.deadline,
            sender,
        )));
        Self { handle, result }
    }
}

#[tracing::instrument(skip_all)]
async fn run_decision(
    client: Arc<DecisionsClient>,
    request: DecisionRequest,
    cancellation: CancellationToken,
    freshness: CancellationToken,
    deadline: Instant,
    sender: oneshot::Sender<Result<String, DecisionsError>>,
) {
    let started = Instant::now();
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => {},
        _ = freshness.cancelled() => {},
        result = tokio::time::timeout_at(deadline, client.choose_effort(
            &request.model, &request.input, &request.instructions, &request.choices,
        )) => {
            match result {
                Ok(result) => {
                    tracing::debug!(success = result.is_ok(), selected_effort = ?result.as_ref().ok(),
                        latency_ms = started.elapsed().as_millis(),
                        "adaptive reasoning request completed");
                    let _ = sender.send(result);
                },
                Err(_) => tracing::debug!("adaptive reasoning request deadline"),
            }
        }
    }
}
