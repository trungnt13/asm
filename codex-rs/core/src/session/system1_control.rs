use super::ORDINARY_EFFORTS;
use super::System1Session;
use super::System1State;
use super::allowed_efforts;
use super::ordinary_agent_session;
use crate::session::session::Session;
use codex_features::Feature;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::System1UpdatedEvent;
use std::sync::Mutex;
use std::sync::atomic::Ordering;

impl Session {
    pub(crate) fn system1_enabled(&self, initial_enabled: bool) -> bool {
        self.system1_state(initial_enabled, /*initial_revision*/ 0)
            .enabled
    }

    pub(crate) fn system1_state(
        &self,
        initial_enabled: bool,
        initial_revision: u64,
    ) -> System1State {
        let state = self
            .services
            .thread_extension_data
            .get_or_init(System1Session::default);
        let mode = state.mode.get_or_init(|| {
            tracing::info!(thread_id = %self.thread_id(), enabled = initial_enabled,
                revision = initial_revision, reason = "initial", "System1 mode changed");
            state.ever_enabled.store(initial_enabled, Ordering::Release);
            Mutex::new(System1State {
                enabled: initial_enabled,
                revision: initial_revision,
                error: None,
                failure_episode: 0,
            })
        });
        *mode
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(in crate::session) fn system1_paused(&self) -> bool {
        self.services
            .thread_extension_data
            .get::<System1Session>()
            .is_some_and(|state| {
                state.shutdown.is_cancelled()
                    || state.mode.get().is_some_and(|mode| {
                        !mode
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .enabled
                    })
            })
    }

    #[tracing::instrument(skip_all)]
    pub(in crate::session) async fn pause_system1(&self) {
        self.publish_system1_enabled(/*enabled*/ false, "manual_override")
            .await;
    }

    #[tracing::instrument(skip_all)]
    pub(crate) async fn set_system1_enabled(&self, enabled: bool) -> CodexResult<()> {
        let _settings_guard = super::super::thread_settings::acquire_persistence_lock(self).await;
        self.set_system1_enabled_locked(enabled).await
    }

    #[tracing::instrument(skip_all)]
    async fn set_system1_enabled_locked(&self, enabled: bool) -> CodexResult<()> {
        if enabled {
            let (config, source, thread_source, model, effort) = {
                let state = self.state.lock().await;
                let configuration = &state.session_configuration;
                if crate::claude_runtime::is_native_model(
                    configuration.step_settings.collaboration_mode.model(),
                ) {
                    return Err(CodexErr::InvalidRequest(
                        "Native runtimes do not support adaptive reasoning".to_string(),
                    ));
                }
                if state.shutting_down {
                    return Err(CodexErr::InvalidRequest(
                        "thread is shutting down".to_string(),
                    ));
                }
                (
                    configuration.original_config_do_not_use.clone(),
                    configuration.session_source.clone(),
                    configuration.thread_source.clone(),
                    configuration
                        .step_settings
                        .collaboration_mode
                        .model()
                        .to_string(),
                    configuration
                        .step_settings
                        .collaboration_mode
                        .reasoning_effort(),
                )
            };
            if !ordinary_agent_session(&source, thread_source.as_ref()) {
                return Err(CodexErr::InvalidRequest(
                    "System1 is unavailable for internal worker sessions".to_string(),
                ));
            }
            if !config.features.enabled(Feature::StepModelSwitching)
                || !config.features.enabled(Feature::ReasoningEffortOverride)
            {
                return Err(CodexErr::InvalidRequest(
                    "System1 requires step_model_switching and reasoning_effort_override"
                        .to_string(),
                ));
            }
            let active = self
                .active_turn
                .lock()
                .await
                .as_ref()
                .and_then(|turn| turn.task.as_ref())
                .map(|task| task.turn_context.next_step_settings.load_full());
            if effort
                .as_ref()
                .is_some_and(|effort| !ORDINARY_EFFORTS.contains(effort))
                || active
                    .as_ref()
                    .and_then(|settings| settings.effective_reasoning_effort())
                    .is_some_and(|effort| !ORDINARY_EFFORTS.contains(&effort))
            {
                return Err(CodexErr::InvalidRequest(
                    "System1 cannot replace Ultra or Persistent effort".to_string(),
                ));
            }
            if active.as_ref().is_some_and(|settings| {
                !self
                    .services
                    .model_client
                    .reasoning_effort_override_enabled(&settings.model_info)
                    || allowed_efforts(&config.system1, &settings.model_info).len() < 2
            }) {
                return Err(CodexErr::InvalidRequest(
                    "the active model does not support System1".to_string(),
                ));
            }
            let model_info = self
                .services
                .models_manager
                .get_model_info(&model, &config.to_models_manager_config())
                .await;
            if !self
                .services
                .model_client
                .reasoning_effort_override_enabled(&model_info)
                || allowed_efforts(&config.system1, &model_info).len() < 2
                || config.system1.update_on.is_empty()
            {
                return Err(CodexErr::InvalidRequest("System1 requires a supported OpenAI model, at least two allowed efforts, and a configured update trigger".to_string()));
            }
        }
        self.publish_system1_enabled(enabled, "user_toggle").await;
        Ok(())
    }

    #[tracing::instrument(skip_all)]
    async fn publish_system1_enabled(&self, enabled: bool, reason: &'static str) {
        let state = self
            .services
            .thread_extension_data
            .get_or_init(System1Session::default);
        if enabled {
            state.ever_enabled.store(/*val*/ true, Ordering::Release);
        }
        let mode = state.mode.get_or_init(|| {
            Mutex::new(System1State {
                enabled: false,
                revision: 0,
                error: None,
                failure_episode: 0,
            })
        });
        let updated = {
            let mut mode = mode
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if mode.enabled == enabled {
                None
            } else {
                mode.enabled = enabled;
                *state
                    .recovery
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    super::System1Recovery::default();
                state
                    .decision_due_after_enable
                    .store(enabled, Ordering::Release);
                state.generation.fetch_add(/*val*/ 1, Ordering::AcqRel);
                mode.revision = mode.revision.saturating_add(/*rhs*/ 1);
                Some(*mode)
            }
        };
        if let Some(updated) = updated {
            self.invalidate_system1();
            tracing::info!(thread_id = %self.thread_id(), enabled, revision = updated.revision, reason, "System1 mode changed");
            self.send_event_raw_without_materializing_rollout(Event {
                id: super::super::new_submission_id(),
                msg: EventMsg::System1Updated(System1UpdatedEvent {
                    enabled,
                    system1_revision: updated.revision,
                    error: updated.error,
                    failure_episode: updated.failure_episode,
                    warning: None,
                }),
            })
            .await;
        }
    }
}
