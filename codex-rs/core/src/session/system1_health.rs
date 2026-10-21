use super::System1Session;
use super::System1State;
use crate::config::Config;
use crate::session::session::Session;
use codex_api::DecisionsError;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::System1Error;
use codex_protocol::protocol::System1FailureWarning;
use codex_protocol::protocol::System1UpdatedEvent;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy)]
pub(in crate::session) enum System1HealthUpdate {
    Decision(Option<System1Error>),
    WarmupTimeout,
}

pub(in crate::session) struct System1DecisionHealthGuard<'a> {
    pub(in crate::session) freshness: &'a CancellationToken,
    pub(in crate::session) cancellation: &'a CancellationToken,
}

pub(in crate::session) fn classify_error(error: DecisionsError) -> System1Error {
    match error {
        DecisionsError::Credentials => System1Error::Credentials,
        DecisionsError::InvalidRequest => System1Error::InvalidRequest,
        DecisionsError::Transport => System1Error::Transport,
        DecisionsError::TransportDetails(code) => System1Error::TransportDetails { code },
        DecisionsError::Http(status) => System1Error::Http { status },
        DecisionsError::ResponseTooLarge => System1Error::ResponseTooLarge,
        DecisionsError::InvalidResponse => System1Error::InvalidResponse,
    }
}

// Request/credential faults need explicit recovery rather than repeated identical requests.
pub(super) fn permanent_error(error: System1Error) -> bool {
    match error {
        System1Error::Credentials | System1Error::InvalidRequest | System1Error::Rejected => true,
        System1Error::Http { status } => {
            (400..500).contains(&status) && !matches!(status, 408 | 429)
        }
        System1Error::Transport
        | System1Error::TransportDetails { .. }
        | System1Error::Timeout
        | System1Error::ResponseTooLarge
        | System1Error::InvalidResponse => false,
    }
}

impl Session {
    pub(in crate::session) fn initialize_system1_health(
        &self,
        config: &Config,
        source: &super::super::session::SessionConfiguration,
    ) {
        let state = self
            .services
            .thread_extension_data
            .get_or_init(System1Session::default);
        let Some(mode) = state.mode.get() else {
            return;
        };
        {
            let mut mode = mode
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            mode.error = config.system1_initial_error;
            mode.failure_episode = config.system1_initial_failure_episode;
            state
                .recovery
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .blocked = mode.error.is_some_and(permanent_error);
        }
        if self.system1_warmup_eligible(config, source)
            && let Err(error) = state.client(config)
        {
            self.update_system1_error(
                System1HealthUpdate::Decision(Some(classify_error(error))),
                /*generation*/ None,
                /*guard*/ None,
            );
        }
    }

    fn update_system1_error(
        &self,
        update: System1HealthUpdate,
        generation: Option<u64>,
        guard: Option<System1DecisionHealthGuard<'_>>,
    ) -> Option<(System1State, Option<u64>)> {
        let error = match update {
            System1HealthUpdate::Decision(error) => error,
            System1HealthUpdate::WarmupTimeout => Some(System1Error::Timeout),
        };
        let state = self
            .services
            .thread_extension_data
            .get::<System1Session>()?;
        let mode = state.mode.get()?;
        let mut mode = mode
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.shutdown.is_cancelled()
            || !mode.enabled
            || generation
                .is_some_and(|expected| state.generation.load(Ordering::Acquire) != expected)
        {
            return None;
        }
        let mut recovery = state
            .recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Acceptance resets under this same lock; stale failures cannot follow that reset.
        if guard.is_some_and(|guard| {
            guard.freshness.is_cancelled() || guard.cancellation.is_cancelled()
        }) {
            return None;
        }
        let mut cooldown_seconds = None;
        if let System1HealthUpdate::Decision(Some(error)) = update {
            recovery.failures = recovery.failures.saturating_add(/*rhs*/ 1);
            recovery.blocked = permanent_error(error);
            if !recovery.blocked {
                let seconds = (1_u64
                    << recovery.failures.saturating_sub(/*rhs*/ 1).min(/*other*/ 5))
                .min(/*other*/ 30);
                recovery.next_probe = Some(Instant::now() + Duration::from_secs(seconds));
                cooldown_seconds = Some(seconds);
            }
        }
        if mode.error == error {
            return None;
        }
        if mode.error.is_none() && error.is_some() {
            mode.failure_episode = mode.failure_episode.saturating_add(/*rhs*/ 1);
        }
        mode.error = error;
        mode.revision = mode.revision.saturating_add(/*rhs*/ 1);
        Some((*mode, cooldown_seconds))
    }

    #[tracing::instrument(skip_all)]
    pub(in crate::session) async fn publish_system1_error(
        &self,
        update: System1HealthUpdate,
        generation: Option<u64>,
        guard: Option<System1DecisionHealthGuard<'_>>,
    ) {
        let (error, warmup_timeout_seconds) = match update {
            System1HealthUpdate::Decision(error) => (error, None),
            System1HealthUpdate::WarmupTimeout => (
                Some(System1Error::Timeout),
                Some(super::super::system1_task::WARMUP_TIMEOUT.as_secs()),
            ),
        };
        let reasoning_effort = if error.is_some() {
            let active = self
                .active_turn
                .lock()
                .await
                .as_ref()
                .and_then(|turn| turn.task.as_ref())
                .map(|task| task.turn_context.next_step_settings.load_full());
            if let Some(active) = active {
                active.effective_reasoning_effort()
            } else {
                self.state
                    .lock()
                    .await
                    .session_configuration
                    .step_settings
                    .collaboration_mode
                    .reasoning_effort()
            }
        } else {
            None
        };
        let Some((updated, cooldown_seconds)) =
            self.update_system1_error(update, generation, guard)
        else {
            return;
        };
        tracing::info!(thread_id = %self.thread_id(), ?error, revision = updated.revision,
            failure_episode = updated.failure_episode, "System1 health changed");
        self.deliver_event_raw(Event {
            id: super::super::new_submission_id(),
            msg: EventMsg::System1Updated(System1UpdatedEvent {
                enabled: updated.enabled,
                system1_revision: updated.revision,
                error: updated.error,
                failure_episode: updated.failure_episode,
                warning: error.map(|_| System1FailureWarning {
                    warmup_timeout_seconds,
                    reasoning_effort,
                    cooldown_seconds,
                }),
            }),
        })
        .await;
    }
}
