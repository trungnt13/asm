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

    pub(crate) fn set_adaptive_reasoning_enabled(&mut self, enabled: bool, revision: u64) {
        if revision <= self.status_state.adaptive_reasoning_revision {
            return;
        }
        self.status_state.adaptive_reasoning_revision = revision;
        let previous = self.status_state.adaptive_reasoning_enabled;
        self.status_state.adaptive_reasoning_enabled = enabled;
        if !previous && enabled {
            self.status_state.suppressed_adaptive_turn_id = None;
        }
        self.refresh_status_surfaces();
    }

    pub(crate) fn announce_adaptive_reasoning_enabled(&mut self, enabled: bool, revision: u64) {
        self.set_adaptive_reasoning_enabled(enabled, revision);
        if revision <= self.status_state.announced_adaptive_reasoning_revision {
            return;
        }
        self.status_state.announced_adaptive_reasoning_revision = revision;
        let state = if enabled { "on" } else { "off" };
        self.add_info_message(
            format!("Adaptive reasoning turned {state}."),
            /*hint*/ None,
        );
    }

    pub(super) fn clear_captured_reasoning_for_manual_settings(&mut self) {
        self.status_state.captured_step_reasoning = None;
        self.status_state.suppressed_adaptive_turn_id = self.turn_lifecycle.last_turn_id.clone();
    }
}
