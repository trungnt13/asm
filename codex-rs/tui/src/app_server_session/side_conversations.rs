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
                tracing::warn!(%thread_id, %error, "could not read legacy conversation display metadata; showing ordinary thread history");
                None
            }
        }
    }
}

#[cfg(test)]
#[path = "side_conversations_tests.rs"]
mod tests;
