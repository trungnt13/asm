use super::App;
use crate::app_event::System1Action;
use crate::app_server_session::AppServerSession;
use codex_app_server_protocol::ThreadSettingsUpdateParams;
use codex_protocol::ThreadId;

impl App {
    #[tracing::instrument(skip_all)]
    pub(super) async fn show_system1_status(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) {
        if self.chat_widget.thread_id() != Some(thread_id) {
            return;
        }
        let response = app_server.system1_stats_read(thread_id).await.ok();
        self.chat_widget.show_system1_status(response);
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn change_system1(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
        action: System1Action,
    ) {
        if self.chat_widget.thread_id() != Some(thread_id) {
            return;
        }
        let enabled = match action {
            System1Action::Toggle => !self.chat_widget.system1_enabled(),
            System1Action::Enable => true,
            System1Action::Disable => false,
        };
        let result = app_server
            .system1_update(ThreadSettingsUpdateParams {
                thread_id: thread_id.to_string(),
                system1_enabled: Some(enabled),
                ..Default::default()
            })
            .await;
        match result {
            Ok(Some(
                response @ codex_app_server_protocol::ThreadSettingsUpdateResponse {
                    system1_revision: Some(revision),
                    ..
                },
            )) => {
                let error = response.system1_error;
                let failure_episode = response.system1_failure_episode.unwrap_or_default();
                self.apply_system1_to_cached_session(
                    thread_id,
                    enabled,
                    revision,
                    error,
                    failure_episode,
                )
                .await;
                if self.chat_widget.thread_id() == Some(thread_id) {
                    self.chat_widget
                        .set_system1_state(enabled, revision, error, failure_episode);
                }
            }
            Ok(Some(_)) | Ok(None) => self
                .chat_widget
                .add_error_message("System1 is unavailable on this server.".to_string()),
            Err(error) => self
                .chat_widget
                .add_error_message(format!("Could not change System1: {error}")),
        }
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn apply_system1_to_cached_session(
        &mut self,
        thread_id: ThreadId,
        enabled: bool,
        revision: u64,
        error: Option<codex_protocol::protocol::System1Error>,
        failure_episode: u64,
    ) {
        if let Some(blank) = self.agents_overview.blank_sessions.get_mut(&thread_id)
            && revision > blank.session.system1_revision
        {
            blank.session.system1_enabled = enabled;
            blank.session.system1_revision = revision;
            blank.session.system1_error = error;
            blank.session.system1_failure_episode = failure_episode;
        }
        if self.primary_thread_id == Some(thread_id)
            && let Some(session) = self.primary_session_configured.as_mut()
            && revision > session.system1_revision
        {
            session.system1_enabled = enabled;
            session.system1_revision = revision;
            session.system1_error = error;
            session.system1_failure_episode = failure_episode;
        }
        if let Some(channel) = self.thread_event_channels.get(&thread_id) {
            let mut store = channel.store.lock().await;
            if enabled
                && store.session.as_ref().is_some_and(|session| {
                    revision > session.system1_revision && !session.system1_enabled
                })
            {
                store.reasoning_state.suppressed_turn_id = None;
            }
            if let Some(session) = store.session.as_mut()
                && revision > session.system1_revision
            {
                session.system1_enabled = enabled;
                session.system1_revision = revision;
                session.system1_error = error;
                session.system1_failure_episode = failure_episode;
            }
        }
    }
}
