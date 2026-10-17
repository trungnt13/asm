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
            if let Some(connection) = response.decision_connection {
                let profile = connection
                    .profile
                    .as_deref()
                    .unwrap_or("legacy configuration");
                let client = if connection.client_initialized {
                    "client initialized"
                } else {
                    "client not initialized"
                };
                message.push_str(&format!(
                    "\nDecision connection: session-fixed; {client}\nDecision backend/protocol: {}\nDecision model: {}\nDecision profile: {profile}",
                    connection.backend, connection.model,
                ));
            } else {
                message.push_str("\nDecision connection: not resolved or not reported by server.");
            }
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
            message.push_str("\nDecision connection and stats unavailable on this server.");
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
        warning: Option<codex_protocol::protocol::System1FailureWarning>,
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
        self.status_state.system1_warning = warning;
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
        let announced = self.status_state.announced_system1_failures.get(&thread_id);
        if announced.is_some_and(|(previous_episode, previous_error)| {
            episode < *previous_episode
                || (episode == *previous_episode && error == *previous_error)
        }) {
            return;
        }
        self.status_state
            .announced_system1_failures
            .insert(thread_id, (episode, error));
        let code = match error {
            codex_protocol::protocol::System1Error::Credentials => "CREDENTIALS".to_string(),
            codex_protocol::protocol::System1Error::InvalidRequest => "INVALID_REQUEST".to_string(),
            codex_protocol::protocol::System1Error::Transport => "TRANSPORT".to_string(),
            codex_protocol::protocol::System1Error::Timeout => "TIMEOUT".to_string(),
            codex_protocol::protocol::System1Error::Http { status } => format!("HTTP{status}"),
            codex_protocol::protocol::System1Error::ResponseTooLarge => {
                "RESPONSE_TOO_LARGE".to_string()
            }
            codex_protocol::protocol::System1Error::InvalidResponse => {
                "INVALID_RESPONSE".to_string()
            }
            codex_protocol::protocol::System1Error::Rejected => "REJECTED".to_string(),
        };
        let message = if let Some(warning) = &self.status_state.system1_warning {
            let effort = warning
                .reasoning_effort
                .as_ref()
                .map_or("model default", ReasoningEffortConfig::as_str);
            let recovery = warning.cooldown_seconds.map_or_else(
                || "recovery blocked".to_string(),
                |seconds| format!("cool down {seconds}s"),
            );
            format!("System 1 error {code}, fallback to {effort}, {recovery}")
        } else {
            format!("System 1 error {code}, fallback and cooldown unavailable")
        };
        self.add_to_history(history_cell::new_usage_warning_event(message));
        self.request_redraw();
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
