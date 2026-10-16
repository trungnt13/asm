use super::*;
use crate::app_event::System1Action;

impl ChatWidget {
    pub(super) fn request_system1(&mut self, action: System1Action) {
        if let Some(thread_id) = self.thread_id {
            self.app_event_tx
                .send(AppEvent::SetSystem1 { thread_id, action });
        } else {
            self.add_error_message("No active session for System1.".to_string());
        }
    }

    pub(crate) fn show_system1_status(
        &mut self,
        response: Option<codex_app_server_protocol::ThreadSystem1StatsReadResponse>,
    ) {
        let (enabled, revision, error, failure_episode, selected_effort) =
            if let Some(response) = &response {
                (
                    response.enabled,
                    response.revision,
                    response.error,
                    response.failure_episode,
                    response.reasoning_effort.clone(),
                )
            } else {
                (
                    self.system1_enabled(),
                    self.status_state.system1_revision,
                    self.status_state.system1_error,
                    self.status_state.system1_failure_episode,
                    self.effective_reasoning_effort()
                        .or_else(|| self.config.model_reasoning_effort.clone()),
                )
            };
        let mode = if enabled { "on" } else { "off" };
        let selected = selected_effort
            .as_ref()
            .map_or("model default", ReasoningEffortConfig::as_str);
        let captured = self
            .status_state
            .captured_step_reasoning
            .as_ref()
            .map(|step| {
                step.reasoning_effort
                    .as_ref()
                    .map_or("none", ReasoningEffortConfig::as_str)
            })
            .unwrap_or("not captured");
        let health = error
            .map(|error| {
                if enabled {
                    error.to_string()
                } else {
                    format!("{error} (retained while off)")
                }
            })
            .unwrap_or_else(|| "no reported error".to_string());
        let source = if response.is_some() {
            "loaded runtime"
        } else {
            "cached TUI state (server query failed)"
        };
        let mut message = format!(
            "System1 status ({source})\nMode: {mode}\nSelected effort: {selected}\nLast captured step effort: {captured}\nHealth: {health}\nRevision: {revision}; failure episode: {failure_episode}"
        );
        if let Some(response) = response {
            let stats = response.stats;
            let last = stats
                .last_elapsed_ms
                .map_or_else(|| "not available".to_string(), |ms| format!("{ms} ms"));
            message.push_str(&format!(
                "\nStats: current loaded runtime; reset on unload/restart; warmup excluded\nCompleted decisions: {}; accepted (including unchanged): {}; effort changes: {}\nErrors: {}; ignored/cancelled: {}; setup failures: {}\nDecision wall time: total {} ms; last {last}\nTiming does not measure net overhead or savings.",
                stats.completed_attempts, stats.accepted, stats.effort_changes, stats.errors,
                stats.ignored, stats.setup_failures, stats.total_elapsed_ms,
            ));
        } else {
            message.push_str("\nStats unavailable on this server.");
        }
        self.add_info_message(message, /*hint*/ None);
    }

    pub(crate) fn system1_enabled(&self) -> bool {
        self.status_state.system1_enabled
    }

    pub(crate) fn set_system1_state(
        &mut self,
        enabled: bool,
        revision: u64,
        error: Option<codex_protocol::protocol::System1Error>,
        failure_episode: u64,
    ) {
        if revision < self.status_state.system1_revision {
            return;
        }
        if revision > self.status_state.system1_revision {
            self.status_state.system1_revision = revision;
            if enabled != self.status_state.system1_enabled {
                self.status_state.system1_enabled = enabled;
                if enabled {
                    self.status_state.suppressed_system1_turn_id = None;
                }
                let state = if enabled { "on" } else { "off" };
                self.add_info_message(format!("System1 turned {state}."), /*hint*/ None);
            }
        }
        self.status_state.system1_error = error;
        self.status_state.system1_failure_episode = failure_episode;
        self.announce_system1_error();
        self.refresh_status_surfaces();
    }

    pub(super) fn announce_system1_error(&mut self) {
        if !self.system1_enabled() {
            return;
        }
        let (Some(thread_id), Some(error)) = (self.thread_id, self.status_state.system1_error)
        else {
            return;
        };
        let episode = self.status_state.system1_failure_episode;
        let announced = self
            .status_state
            .announced_system1_failures
            .entry(thread_id)
            .or_default();
        if episode <= *announced {
            return;
        }
        *announced = episode;
        self.add_info_message(
            format!("System1 error: {error}. Keeping the current effort; auto remains enabled."),
            /*hint*/ None,
        );
    }

    pub(crate) fn inherit_system1_warnings(&mut self, previous: &mut Self) {
        self.status_state.announced_system1_failures =
            std::mem::take(&mut previous.status_state.announced_system1_failures);
    }

    pub(crate) fn capture_reasoning_state(&self) -> crate::session_state::ThreadReasoningState {
        crate::session_state::ThreadReasoningState {
            captured_step: self.status_state.captured_step_reasoning.clone(),
            suppressed_turn_id: self.status_state.suppressed_system1_turn_id.clone(),
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
        self.status_state.suppressed_system1_turn_id = state.suppressed_turn_id;
        if let Some(notification) = state.captured_step {
            self.handle_server_notification(
                codex_app_server_protocol::ServerNotification::ReasoningEffortUpdated(notification),
                /*replay_kind*/ None,
            );
        }
    }

    pub(super) fn clear_captured_reasoning_for_manual_settings(&mut self) {
        self.status_state.captured_step_reasoning = None;
        self.status_state.suppressed_system1_turn_id = self.turn_lifecycle.last_turn_id.clone();
    }
}
