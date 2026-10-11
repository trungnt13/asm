use super::App;
use crate::app_event::AdaptiveReasoningAction;
use crate::app_server_session::AppServerSession;
use codex_app_server_protocol::ThreadSettingsUpdateParams;
use codex_protocol::ThreadId;

impl App {
    #[tracing::instrument(skip_all)]
    pub(super) async fn change_adaptive_reasoning(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
        action: AdaptiveReasoningAction,
    ) {
        if self.chat_widget.thread_id() != Some(thread_id) {
            return;
        }
        let enabled = match action {
            AdaptiveReasoningAction::Toggle => !self.chat_widget.adaptive_reasoning_enabled(),
            AdaptiveReasoningAction::Enable => true,
            AdaptiveReasoningAction::Disable => false,
        };
        let result = app_server
            .adaptive_reasoning_update(ThreadSettingsUpdateParams {
                thread_id: thread_id.to_string(),
                adaptive_reasoning_enabled: Some(enabled),
                ..Default::default()
            })
            .await;
        match result {
            Ok(Some(
                response @ codex_app_server_protocol::ThreadSettingsUpdateResponse {
                    adaptive_reasoning_revision: Some(revision),
                    ..
                },
            )) => {
                let error = response.adaptive_reasoning_error;
                let failure_episode = response
                    .adaptive_reasoning_failure_episode
                    .unwrap_or_default();
                self.apply_adaptive_reasoning_to_cached_session(
                    thread_id,
                    enabled,
                    revision,
                    error,
                    failure_episode,
                )
                .await;
                if self.chat_widget.thread_id() == Some(thread_id) {
                    self.chat_widget.set_adaptive_reasoning_state(
                        enabled,
                        revision,
                        error,
                        failure_episode,
                    );
                }
            }
            Ok(Some(_)) | Ok(None) => self
                .chat_widget
                .add_error_message("Adaptive reasoning is unavailable on this server.".to_string()),
            Err(error) => self
                .chat_widget
                .add_error_message(format!("Could not change adaptive reasoning: {error}")),
        }
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn apply_adaptive_reasoning_to_cached_session(
        &mut self,
        thread_id: ThreadId,
        enabled: bool,
        revision: u64,
        error: Option<codex_protocol::protocol::AdaptiveReasoningError>,
        failure_episode: u64,
    ) {
        if let Some(blank) = self.agents_overview.blank_sessions.get_mut(&thread_id)
            && revision > blank.session.adaptive_reasoning_revision
        {
            blank.session.adaptive_reasoning_enabled = enabled;
            blank.session.adaptive_reasoning_revision = revision;
            blank.session.adaptive_reasoning_error = error;
            blank.session.adaptive_reasoning_failure_episode = failure_episode;
        }
        if self.primary_thread_id == Some(thread_id)
            && let Some(session) = self.primary_session_configured.as_mut()
            && revision > session.adaptive_reasoning_revision
        {
            session.adaptive_reasoning_enabled = enabled;
            session.adaptive_reasoning_revision = revision;
            session.adaptive_reasoning_error = error;
            session.adaptive_reasoning_failure_episode = failure_episode;
        }
        if let Some(channel) = self.thread_event_channels.get(&thread_id) {
            let mut store = channel.store.lock().await;
            if let Some(session) = store.session.as_mut()
                && revision > session.adaptive_reasoning_revision
            {
                session.adaptive_reasoning_enabled = enabled;
                session.adaptive_reasoning_revision = revision;
                session.adaptive_reasoning_error = error;
                session.adaptive_reasoning_failure_episode = failure_episode;
            }
        }
    }
}
