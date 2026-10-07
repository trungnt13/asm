//! Apply one configured title on user request without enabling automatic updates.

use super::App;
use super::auto_rename::automatic_title_prompt;
use super::auto_rename::user_message_text;
use super::thread_title::parse_thread_title;
use super::thread_title::recent_conversation_messages;
use crate::app_command::AppCommand;
use crate::app_event::AutoRenameRequest;
use crate::app_event::ThreadTitleDestination;
use crate::app_server_session::AppServerSession;
use codex_config::AutoRenameContext;
use codex_protocol::ThreadId;

impl App {
    #[tracing::instrument(skip_all)]
    pub(super) async fn autorename_thread(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) {
        if self.chat_widget.thread_id() != Some(thread_id) {
            return;
        }
        if self
            .pending_thread_titles
            .contains_key(&(thread_id, ThreadTitleDestination::ExplicitAutoRename))
        {
            self.chat_widget.add_info_message(
                "A session name is already being generated.".to_string(),
                /*hint*/ None,
            );
            return;
        }
        // Live item buffers can evict the latest user message during long turns.
        let thread = match app_server
            .thread_read(thread_id, /*include_turns*/ true)
            .await
        {
            Ok(thread) => thread,
            Err(error) => {
                self.chat_widget
                    .add_error_message(format!("Could not read the current session: {error}"));
                return;
            }
        };
        let settings = &self.local_settings.auto_rename;
        let items = thread.turns.iter().flat_map(|turn| turn.items.iter());
        let source = match settings.context {
            AutoRenameContext::UserMessage => items
                .rev()
                .filter_map(user_message_text)
                .find(|text| !text.trim().is_empty()),
            AutoRenameContext::RecentConversation => {
                recent_conversation_messages(items, Some(settings))
            }
        };
        let Some(source) = source else {
            self.chat_widget
                .add_error_message("There is no conversation text to name.".to_string());
            return;
        };
        let prompt = automatic_title_prompt(settings, &source);
        // An explicit naming request supersedes background ownership and other suggestions.
        self.cancel_thread_title_generation(thread_id);
        self.generate_thread_title(
            app_server,
            thread_id,
            ThreadTitleDestination::ExplicitAutoRename,
            prompt,
            AutoRenameRequest {
                expected_name: thread.name,
                auto_update: false,
            },
        );
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn apply_autorename_result(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
        result: Result<String, String>,
        max_title_chars: usize,
        request: AutoRenameRequest,
    ) {
        if self.chat_widget.thread_id() != Some(thread_id) {
            return;
        }
        let title = match result {
            Ok(response) => match parse_thread_title(&response, max_title_chars) {
                Some(title) => title,
                None => {
                    self.chat_widget
                        .add_error_message("Could not generate a valid session name.".to_string());
                    return;
                }
            },
            Err(error) => {
                self.chat_widget
                    .add_error_message(format!("Could not generate a session name: {error}"));
                return;
            }
        };
        match app_server
            .thread_read(thread_id, /*include_turns*/ false)
            .await
        {
            Ok(thread) if thread.name != request.expected_name => {
                self.chat_widget.add_info_message(
                    "The session name changed; keeping the newer name.".to_string(),
                    /*hint*/ None,
                );
                return;
            }
            Ok(thread) if thread.name.as_ref() == Some(&title) => return,
            Ok(_) => {}
            Err(error) => {
                self.chat_widget
                    .add_error_message(format!("Could not read the current session name: {error}"));
                return;
            }
        }
        // The shared command future also includes large user-turn paths.
        if let Err(error) = Box::pin(self.try_submit_active_thread_op_via_app_server(
            app_server,
            thread_id,
            &AppCommand::set_thread_name(title),
        ))
        .await
        {
            self.chat_widget
                .add_error_message(format!("Could not rename the session: {error}"));
        }
    }
}
