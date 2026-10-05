//! Close local pairing selections without deleting saved conversations.

use super::*;
use crate::side_conversations::SideConversationStore;

impl App {
    pub(super) fn close_side_selection(&self, parent: ThreadId, side: ThreadId) -> Result<()> {
        SideConversationStore::new(&self.config.codex_home, &self.app_server_target)
            .close_selection(parent, side)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "side_persistence_tests.rs"]
mod tests;
