use super::AdaptiveReasoningState;
use super::AdaptiveSession;
use crate::config::Config;
use crate::session::session::Session;
use codex_api::DecisionsError;
use codex_protocol::protocol::AdaptiveReasoningError;
use codex_protocol::protocol::AdaptiveReasoningUpdatedEvent;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use std::sync::atomic::Ordering;

pub(in crate::session) fn classify_error(error: DecisionsError) -> AdaptiveReasoningError {
    match error {
        DecisionsError::Credentials => AdaptiveReasoningError::Credentials,
        DecisionsError::InvalidRequest => AdaptiveReasoningError::InvalidRequest,
        DecisionsError::Transport => AdaptiveReasoningError::Transport,
        DecisionsError::Http(status) => AdaptiveReasoningError::Http { status },
        DecisionsError::ResponseTooLarge => AdaptiveReasoningError::ResponseTooLarge,
        DecisionsError::InvalidResponse => AdaptiveReasoningError::InvalidResponse,
    }
}

impl Session {
    pub(in crate::session) fn initialize_adaptive_reasoning_health(
        &self,
        config: &Config,
        source: &super::super::session::SessionConfiguration,
    ) {
        let state = self
            .services
            .thread_extension_data
            .get_or_init(AdaptiveSession::default);
        let Some(mode) = state.mode.get() else {
            return;
        };
        {
            let mut mode = mode
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            mode.error = config.adaptive_reasoning_initial_error;
            mode.failure_episode = config.adaptive_reasoning_initial_failure_episode;
        }
        if self.adaptive_warmup_eligible(config, source)
            && let Err(error) = state.client(config)
        {
            self.update_adaptive_reasoning_error(
                Some(classify_error(error)),
                /*generation*/ None,
            );
        }
    }

    fn update_adaptive_reasoning_error(
        &self,
        error: Option<AdaptiveReasoningError>,
        generation: Option<u64>,
    ) -> Option<AdaptiveReasoningState> {
        let state = self
            .services
            .thread_extension_data
            .get::<AdaptiveSession>()?;
        let mode = state.mode.get()?;
        let mut mode = mode
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.shutdown.is_cancelled()
            || !mode.enabled
            || generation
                .is_some_and(|expected| state.generation.load(Ordering::Acquire) != expected)
            || mode.error == error
        {
            return None;
        }
        if mode.error.is_none() && error.is_some() {
            mode.failure_episode = mode.failure_episode.saturating_add(/*rhs*/ 1);
        }
        mode.error = error;
        mode.revision = mode.revision.saturating_add(/*rhs*/ 1);
        Some(*mode)
    }

    #[tracing::instrument(skip_all)]
    pub(in crate::session) async fn publish_adaptive_reasoning_error(
        &self,
        error: Option<AdaptiveReasoningError>,
        generation: Option<u64>,
    ) {
        let Some(updated) = self.update_adaptive_reasoning_error(error, generation) else {
            return;
        };
        tracing::info!(thread_id = %self.thread_id(), ?error, revision = updated.revision,
            failure_episode = updated.failure_episode, "adaptive reasoning health changed");
        self.deliver_event_raw(Event {
            id: super::super::new_submission_id(),
            msg: EventMsg::AdaptiveReasoningUpdated(AdaptiveReasoningUpdatedEvent {
                enabled: updated.enabled,
                adaptive_reasoning_revision: updated.revision,
                error: updated.error,
                failure_episode: updated.failure_episode,
            }),
        })
        .await;
    }
}
