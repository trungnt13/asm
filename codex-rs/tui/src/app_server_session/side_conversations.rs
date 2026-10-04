//! Apply local side-view boundaries without changing server history or model input.

use super::*;
use crate::side_conversations::SideConversation;
use crate::side_conversations::SideConversationStore;

impl AppServerSession {
    pub(crate) fn with_side_conversations(
        mut self,
        codex_home: &std::path::Path,
        target: &crate::AppServerTarget,
    ) -> Self {
        self.side_conversations = Some(SideConversationStore::new(codex_home, target));
        self
    }

    pub(super) fn side_conversation(&self, thread_id: ThreadId) -> Option<SideConversation> {
        match self.side_conversations.as_ref()?.side(thread_id) {
            Ok(side) => side,
            Err(error) => {
                // Pairing is optional UI state, never a prerequisite for reading server history.
                // Restoring the pair separately surfaces the metadata error in the chat.
                tracing::warn!(%thread_id, %error, "could not read parallel navigation metadata; showing ordinary thread history");
                None
            }
        }
    }

    #[tracing::instrument(skip_all)]
    pub(crate) async fn save_side_conversation(
        &mut self,
        parent: ThreadId,
        side: ThreadId,
    ) -> Result<()> {
        let last_inherited_turn = if self
            .history_pagination
            .get(&side)
            .is_some_and(|state| state.history_mode == ThreadHistoryMode::Legacy)
        {
            self.thread_read(side, /*include_turns*/ true)
                .await?
                .turns
                .last()
                .map(|turn| turn.id.clone())
        } else {
            self.thread_turns_page(side, /*cursor*/ None, /*limit*/ 1)
                .await?
                .data
                .first()
                .map(|turn| turn.id.clone())
        };
        let store = self.side_conversations.as_ref().ok_or_else(|| {
            color_eyre::eyre::eyre!("side conversation storage is not configured")
        })?;
        store.save(&SideConversation {
            parent,
            side,
            last_inherited_turn: last_inherited_turn.clone(),
        })?;
        // The new side has no visible history to page yet. Keep its format and boundary for
        // prompt editing; reverting an empty visible window must not reopen inherited history.
        let state = self.history_pagination.entry(side).or_default();
        let history_mode = state.history_mode;
        *state = history::ThreadHistoryPagination::default();
        state.history_mode = history_mode;
        state.side_boundary = last_inherited_turn;
        Ok(())
    }
}

#[cfg(test)]
#[path = "side_conversations_tests.rs"]
mod tests;
