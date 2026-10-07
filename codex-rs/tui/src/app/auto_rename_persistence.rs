//! Persist generated-title eligibility in client-local, server-scoped records.

use super::App;
use super::thread_events::ThreadBufferedEvent;
use crate::side_conversations::app_server_scope;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::ThreadId;
use std::io::Read;
use std::path::PathBuf;

const MAX_TITLE_BYTES: u64 = 512;

impl App {
    pub(super) fn automatic_title_record_path(&self, thread_id: ThreadId) -> PathBuf {
        let scope = app_server_scope(&self.app_server_target);
        self.config
            .codex_home
            .join("asm-automatic-titles")
            .join(scope)
            .join(format!("{thread_id}.txt"))
            .into_path_buf()
    }

    fn read_automatic_title_record(&self, thread_id: ThreadId) -> std::io::Result<Option<String>> {
        let file = match std::fs::File::open(self.automatic_title_record_path(thread_id)) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut title = String::new();
        file.take(MAX_TITLE_BYTES + 1).read_to_string(&mut title)?;
        if title.len() as u64 > MAX_TITLE_BYTES
            || title.chars().count() > 128
            || title.trim().is_empty()
            || title.trim() != title
            || title.chars().any(char::is_control)
        {
            return Err(std::io::Error::other("invalid automatic-title record"));
        }
        Ok(Some(title))
    }

    pub(super) fn automatic_title_record_matches(
        &mut self,
        thread_id: ThreadId,
        name: Option<&str>,
    ) -> bool {
        match self.read_automatic_title_record(thread_id) {
            Ok(title) => name.is_some() && title.as_deref() == name,
            Err(error) => {
                self.chat_widget.add_error_message(format!(
                    "Automatic naming is protected: could not read its saved title: {error}"
                ));
                false
            }
        }
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn restore_automatic_thread_title(
        &mut self,
        thread_id: ThreadId,
        name: Option<&str>,
    ) {
        if self.automatic_thread_titles.contains_key(&thread_id)
            || !self.local_settings.auto_rename.enabled
            || !self.local_settings.auto_rename.auto_update
        {
            return;
        }
        if self.automatic_title_record_matches(thread_id, name)
            && let Some(name) = name
        {
            self.track_automatic_thread_title(thread_id, name.to_string());
            self.seed_completed_automatic_title_turns(thread_id).await;
        }
    }

    #[tracing::instrument(skip_all)]
    async fn seed_completed_automatic_title_turns(&mut self, thread_id: ThreadId) {
        if let Some(channel) = self.thread_event_channels.get(&thread_id) {
            let store = channel.store.lock().await;
            if let Some(state) = self.automatic_thread_titles.get_mut(&thread_id) {
                state.completed_turns.extend(
                    store
                        .turns
                        .iter()
                        .filter(|turn| turn.status == TurnStatus::Completed)
                        .map(|turn| turn.id.clone()),
                );
                state
                    .completed_turns
                    .extend(store.buffer.iter().filter_map(|event| {
                        let ThreadBufferedEvent::Notification(notification) = event else {
                            return None;
                        };
                        let ServerNotification::TurnCompleted(turn) = notification.as_ref() else {
                            return None;
                        };
                        (turn.turn.status == TurnStatus::Completed).then(|| turn.turn.id.clone())
                    }));
            }
        }
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn persist_automatic_thread_title(
        &mut self,
        thread_id: ThreadId,
        title: &str,
    ) {
        if let Err(error) =
            codex_utils_path::write_atomically(&self.automatic_title_record_path(thread_id), title)
        {
            self.stop_automatic_thread_titles(thread_id);
            self.chat_widget.add_error_message(format!(
                "Session renamed, but automatic refresh could not be saved: {error}"
            ));
            return;
        }
        if self.local_settings.auto_rename.enabled && self.local_settings.auto_rename.auto_update {
            let new_ownership = self
                .automatic_thread_titles
                .get(&thread_id)
                .is_none_or(|state| state.owned_title.is_none());
            self.track_automatic_thread_title(thread_id, title.to_string());
            if new_ownership {
                self.seed_completed_automatic_title_turns(thread_id).await;
            }
        }
    }

    pub(super) fn forget_automatic_thread_title(&self, thread_id: ThreadId) -> std::io::Result<()> {
        match std::fs::remove_file(self.automatic_title_record_path(thread_id)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(std::io::Error::new(
                error.kind(),
                format!("Could not save manual-name protection: {error}"),
            )),
        }
    }
}
