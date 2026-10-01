//! Chat-surface modes for temporary sides and saved parallel conversations.
//!
//! Temporary sides restrict commands and use a follow-up placeholder.
//! Saved parallel conversations retain normal composer behavior.
//! App-level lifecycle lives in `app::side`.

use super::*;

impl ChatWidget {
    pub(crate) fn submit_user_message_as_plain_user_turn(
        &mut self,
        user_message: UserMessage,
    ) -> Option<AppCommand> {
        self.submit_user_message_with_shell_escape_policy(user_message, ShellEscapePolicy::Disallow)
    }

    pub(crate) fn set_side_conversation_active(&mut self, active: bool) {
        self.active_side_conversation = active;
        if active {
            self.active_parallel_conversation = false;
        }
        let placeholder = if active {
            self.side_placeholder_text.clone()
        } else {
            self.normal_placeholder_text.clone()
        };
        self.bottom_pane.set_placeholder_text(placeholder);
        self.bottom_pane.set_side_conversation_active(active);
        if self.blocks_direct_input && !active {
            self.bottom_pane.set_parent_owned_thread();
        }
    }

    pub(crate) fn side_conversation_active(&self) -> bool {
        self.active_side_conversation
    }

    pub(crate) fn set_parallel_conversation_active(&mut self, active: bool) {
        self.active_parallel_conversation = active;
        if active {
            self.set_side_conversation_active(/*active*/ false);
        }
    }

    pub(crate) fn parallel_conversation_active(&self) -> bool {
        self.active_parallel_conversation
    }

    pub(crate) fn set_side_conversation_context_label(&mut self, label: Option<String>) {
        self.bottom_pane.set_side_conversation_context_label(label);
    }
}
