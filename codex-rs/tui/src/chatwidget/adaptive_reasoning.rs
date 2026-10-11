use super::*;
use crate::app_event::AdaptiveReasoningAction;

impl ChatWidget {
    pub(super) fn request_adaptive_reasoning(&mut self, action: AdaptiveReasoningAction) {
        if let Some(thread_id) = self.thread_id {
            self.app_event_tx
                .send(AppEvent::SetAdaptiveReasoning { thread_id, action });
        } else {
            self.add_error_message("No active session for adaptive reasoning.".to_string());
        }
    }

    pub(crate) fn adaptive_reasoning_enabled(&self) -> bool {
        self.status_state.adaptive_reasoning_enabled
    }

    pub(crate) fn set_adaptive_reasoning_state(
        &mut self,
        enabled: bool,
        revision: u64,
        error: Option<codex_protocol::protocol::AdaptiveReasoningError>,
        failure_episode: u64,
    ) {
        if revision < self.status_state.adaptive_reasoning_revision {
            return;
        }
        if revision > self.status_state.adaptive_reasoning_revision {
            self.status_state.adaptive_reasoning_revision = revision;
            if enabled != self.status_state.adaptive_reasoning_enabled {
                self.status_state.adaptive_reasoning_enabled = enabled;
                if enabled {
                    self.status_state.suppressed_adaptive_turn_id = None;
                }
                let state = if enabled { "on" } else { "off" };
                self.add_info_message(
                    format!("Adaptive reasoning turned {state}."),
                    /*hint*/ None,
                );
            }
        }
        self.status_state.adaptive_reasoning_error = error;
        self.status_state.adaptive_reasoning_failure_episode = failure_episode;
        self.announce_adaptive_reasoning_error();
        self.refresh_status_surfaces();
    }

    pub(super) fn announce_adaptive_reasoning_error(&mut self) {
        if !self.adaptive_reasoning_enabled() {
            return;
        }
        let (Some(thread_id), Some(error)) =
            (self.thread_id, self.status_state.adaptive_reasoning_error)
        else {
            return;
        };
        let episode = self.status_state.adaptive_reasoning_failure_episode;
        let announced = self
            .status_state
            .announced_adaptive_failures
            .entry(thread_id)
            .or_default();
        if episode <= *announced {
            return;
        }
        *announced = episode;
        self.add_info_message(
            format!("Adaptive reasoning error: {error}. Keeping the current effort; auto remains enabled."),
            /*hint*/ None,
        );
    }

    pub(crate) fn inherit_adaptive_reasoning_warnings(&mut self, previous: &mut Self) {
        self.status_state.announced_adaptive_failures =
            std::mem::take(&mut previous.status_state.announced_adaptive_failures);
    }

    pub(crate) fn capture_reasoning_state(&self) -> crate::session_state::ThreadReasoningState {
        crate::session_state::ThreadReasoningState {
            captured_step: self.status_state.captured_step_reasoning.clone(),
            suppressed_turn_id: self.status_state.suppressed_adaptive_turn_id.clone(),
        }
    }

    pub(crate) fn restore_reasoning_state(
        &mut self,
        state: crate::session_state::ThreadReasoningState,
        latest_turn_id: Option<String>,
    ) {
        let Some(latest_turn_id) = latest_turn_id else {
            return;
        };
        // This live identity is newer than replayed history when turn-start was evicted.
        self.turn_lifecycle.last_turn_id = Some(latest_turn_id);
        self.status_state.suppressed_adaptive_turn_id = state.suppressed_turn_id;
        if let Some(notification) = state.captured_step {
            self.handle_server_notification(
                codex_app_server_protocol::ServerNotification::ReasoningEffortUpdated(notification),
                /*replay_kind*/ None,
            );
        }
    }

    pub(super) fn clear_captured_reasoning_for_manual_settings(&mut self) {
        self.status_state.captured_step_reasoning = None;
        self.status_state.suppressed_adaptive_turn_id = self.turn_lifecycle.last_turn_id.clone();
    }
}
