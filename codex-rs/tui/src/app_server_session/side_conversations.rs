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

    pub(super) fn side_conversation(
        &self,
        thread_id: ThreadId,
    ) -> Result<Option<SideConversation>> {
        self.side_conversations
            .as_ref()
            .map(|store| store.side(thread_id))
            .transpose()
            .map(Option::flatten)
            .map_err(Into::into)
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
            last_inherited_turn,
        })?;
        // The fork response's pagination still refers to inherited history. Rehydrate on the next
        // resume; the newly created side starts with an empty visible transcript.
        self.history_pagination.remove(&side);
        Ok(())
    }
}
