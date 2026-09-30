//! Restore local side navigation without attaching or starting work in the other thread.

use super::*;
use crate::side_conversations::SideConversationStore;

impl App {
    #[tracing::instrument(skip_all)]
    pub(super) async fn remember_side_conversation(
        &mut self,
        app_server: &mut AppServerSession,
        parent: ThreadId,
        side: ThreadId,
    ) -> Result<()> {
        app_server.save_side_conversation(parent, side).await
    }

    pub(super) fn close_side_selection(&self, parent: ThreadId, side: ThreadId) -> Result<()> {
        SideConversationStore::new(&self.config.codex_home, &self.app_server_target)
            .close_selection(parent, side)?;
        Ok(())
    }

    pub(super) fn restore_side_conversation(&mut self, thread_id: ThreadId) {
        let store = SideConversationStore::new(&self.config.codex_home, &self.app_server_target);
        let pair = match store.pair(thread_id) {
            Ok(pair) => pair,
            Err(error) => {
                self.chat_widget
                    .add_error_message(format!("Could not restore side navigation: {error}"));
                return;
            }
        };
        let Some(pair) = pair else { return };
        self.side_threads.clear();
        self.side_threads
            .insert(pair.side, SideThreadState::new(pair.parent));
        self.primary_thread_id = Some(pair.parent);
        if thread_id == pair.side {
            // A parent is attached only after explicit navigation. Never resume its goal merely
            // because its saved side was opened, and never use the side's settings for the parent.
            self.primary_session_configured = None;
        }
        self.upsert_agent_picker_thread(
            pair.parent,
            /*agent_nickname*/ None,
            /*agent_role*/ None,
            /*is_closed*/ false,
        );
        self.upsert_agent_picker_thread(
            pair.side, /*agent_nickname*/ None, /*agent_role*/ None,
            /*is_closed*/ false,
        );
    }
}

#[cfg(test)]
#[path = "side_persistence_tests.rs"]
mod tests;
