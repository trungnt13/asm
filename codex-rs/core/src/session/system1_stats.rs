use super::System1Session;
use crate::session::decision_log::DecisionDisposition;
use crate::session::session::Session;
use codex_config::System1Backend;
use codex_protocol::System1DecisionConnection;
use codex_protocol::System1Stats;
use codex_protocol::protocol::System1Error;
use std::time::Duration;

impl Session {
    pub(crate) fn system1_decision_connection(&self) -> Option<System1DecisionConnection> {
        let state = self
            .services
            .thread_extension_data
            .get::<System1Session>()?;
        let profile = state.profile.get()?;
        Some(System1DecisionConnection {
            backend: match profile.settings.backend {
                System1Backend::Openai => "openai",
                System1Backend::Typesafe => "typesafe",
            }
            .to_string(),
            model: profile.settings.model.clone(),
            profile: profile.name.clone(),
            client_initialized: state.client.get().is_some(),
        })
    }

    pub(crate) fn system1_stats(&self) -> System1Stats {
        self.services
            .thread_extension_data
            .get::<System1Session>()
            .map(|state| {
                state
                    .stats
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone()
            })
            .unwrap_or_default()
    }

    pub(in crate::session) fn record_system1_decision(
        &self,
        disposition: DecisionDisposition,
        error: Option<System1Error>,
        elapsed: Duration,
    ) {
        let Some(state) = self.services.thread_extension_data.get::<System1Session>() else {
            return;
        };
        let mut stats = state
            .stats
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
        stats.completed_attempts = stats.completed_attempts.saturating_add(/*rhs*/ 1);
        stats.total_elapsed_ms = stats.total_elapsed_ms.saturating_add(elapsed_ms);
        stats.last_elapsed_ms = Some(elapsed_ms);
        match disposition {
            DecisionDisposition::Applied | DecisionDisposition::Unchanged => {
                stats.accepted = stats.accepted.saturating_add(/*rhs*/ 1);
                if matches!(disposition, DecisionDisposition::Applied) {
                    stats.effort_changes = stats.effort_changes.saturating_add(/*rhs*/ 1);
                }
            }
            DecisionDisposition::Rejected => {
                stats.errors = stats.errors.saturating_add(/*rhs*/ 1);
            }
            DecisionDisposition::Cancelled
            | DecisionDisposition::Superseded
            | DecisionDisposition::TargetUnavailable => {
                if error.is_some() {
                    stats.errors = stats.errors.saturating_add(/*rhs*/ 1);
                } else {
                    stats.ignored = stats.ignored.saturating_add(/*rhs*/ 1);
                }
            }
            DecisionDisposition::Warmup => unreachable!("warmup is excluded from runtime stats"),
        }
    }
}
