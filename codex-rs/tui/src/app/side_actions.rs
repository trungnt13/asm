//! Preserve Side handoffs while refreshing context or saving independent forks.

use super::*;
use crate::app_event::SideConversationAction;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::UserInput;
use codex_context_fragments::AdditionalContextDeveloperFragment;
use codex_context_fragments::ContextualUserFragment;

const SAVED_FORK_INSTRUCTIONS: &str = "You are in an ordinary saved fork. Inherited history is reference context, not an active task. Earlier Side role and capability restrictions no longer apply; use normal capabilities under this session's permissions. Wait for a new user request before starting work.";

impl App {
    #[tracing::instrument(skip_all)]
    pub(super) async fn apply_side_conversation_action(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        action: SideConversationAction,
    ) -> Result<()> {
        let Some(thread_id) = self.current_displayed_thread_id() else {
            return Ok(());
        };
        let Some(state) = self.side_threads.get(&thread_id).cloned() else {
            self.chat_widget
                .add_error_message("This command requires an active Side conversation.".into());
            return Ok(());
        };
        if matches!(
            action,
            SideConversationAction::Sync | SideConversationAction::Fork { .. }
        ) && self.active_turn_id_for_thread(thread_id).await.is_some()
        {
            self.chat_widget.add_error_message(
                "Wait for Side to finish before syncing or saving a fork.".into(),
            );
            return Ok(());
        }
        if self.pending_server_profiles.contains_key(&thread_id)
            || self
                .pending_server_profiles
                .contains_key(&state.parent_thread_id)
        {
            self.chat_widget.add_error_message(
                "Wait for permissions to update before using this command.".into(),
            );
            return Ok(());
        }
        match action {
            SideConversationAction::SendLast { text } => {
                let Some(reply) = self.chat_widget.last_side_reply_markdown() else {
                    self.chat_widget
                        .add_error_message("Side has no completed reply to send.".into());
                    return Ok(());
                };
                let extra = if text.trim().is_empty() {
                    ""
                } else {
                    text.as_str()
                };
                let separator = if extra.is_empty() { "" } else { "\n\n" };
                // Bound the complete user-requested item before allocating a copy. The byte
                // cap conservatively limits it to 10K tokens without tokenizer work.
                if reply
                    .len()
                    .saturating_add(extra.len())
                    .saturating_add(separator.len())
                    > 10_000
                {
                    self.chat_widget.add_error_message("The forwarded reply and extra message exceed 10,000 bytes; use /export instead.".into());
                    return Ok(());
                }
                let message = format!("{reply}{separator}{extra}");
                let request_id = app_server.next_request_id();
                // Omitted settings preserve the recipient's configuration. The server atomically
                // starts or steers its turn, avoiding a busy/idle race and a second request.
                let result = app_server
                    .request_handle()
                    .request_typed::<TurnStartResponse>(ClientRequest::TurnStart {
                        request_id,
                        params: TurnStartParams {
                            thread_id: state.parent_thread_id.to_string(),
                            client_user_message_id: Some(uuid::Uuid::new_v4().to_string()),
                            input: vec![UserInput::Text {
                                text: message,
                                text_elements: Vec::new(),
                            }],
                            turn_trigger: Some("user".to_string()),
                            ..Default::default()
                        },
                    })
                    .await;
                match result {
                    Ok(_) => self.chat_widget.add_info_message(
                        "Sent the last reply to main.".into(),
                        /*hint*/ None,
                    ),
                    Err(error) => self
                        .chat_widget
                        .add_error_message(format!("Failed to send reply to main: {error}")),
                }
            }
            SideConversationAction::Sync => {
                let config = self.side_fork_config();
                let selected_profile = self.selected_server_profile(thread_id);
                let fork = match app_server
                    .sidebar_fork(
                        self.local_settings.clone(),
                        config,
                        state.parent_thread_id,
                        selected_profile,
                        state.storage,
                    )
                    .await
                {
                    Ok(fork) => fork,
                    Err(error) => {
                        self.chat_widget.add_error_message(format!(
                            "Failed to sync; the current conversation is unchanged: {error}"
                        ));
                        return Ok(());
                    }
                };
                app_server.observe_sidebar_fork(&fork);
                let storage = fork.storage;
                let forked = fork.thread;
                let replacement_id = forked.session.thread_id;
                if let Err(error) = fork.boundary {
                    self.discard_side_thread(app_server, replacement_id).await;
                    self.chat_widget.add_error_message(format!(
                        "Failed to prepare synced conversation: {error}"
                    ));
                    return Ok(());
                }
                if fork.inherited_profile
                    && let Some(profile) = &forked.session.active_permission_profile
                {
                    self.agents_overview
                        .selected_permission_profiles
                        .insert(replacement_id, profile.id.clone());
                }
                {
                    let channel = self.ensure_thread_channel(replacement_id);
                    channel.backend_id = Some(app_server.backend_id());
                    let mut store = channel.store.lock().await;
                    Self::install_side_thread_snapshot(&mut store, forked.session, forked.turns);
                }
                let previous_storage = state.storage;
                let mut state = state;
                state.storage = storage;
                let parent_thread_id = state.parent_thread_id;
                self.side_threads.insert(replacement_id, state);
                self.upsert_agent_picker_thread(
                    replacement_id,
                    /*agent_nickname*/ None,
                    /*agent_role*/ None,
                    /*is_closed*/ false,
                );
                self.sync_agent_picker_thread(replacement_id);
                self.sidebars
                    .entry(Some(parent_thread_id))
                    .or_default()
                    .chat_ready(replacement_id);
                let result = self
                    .select_agent_thread(tui, app_server, replacement_id)
                    .await;
                if result.is_ok() && self.current_displayed_thread_id() == Some(replacement_id) {
                    self.discard_side_thread_in_background(app_server, thread_id)
                        .await;
                    self.sync_side_thread_ui();
                    let message = match previous_storage {
                        crate::app_server_session::SideChatStorage::Temporary => {
                            "Synced with main; previous Side history was discarded."
                        }
                        crate::app_server_session::SideChatStorage::Attached => {
                            "Synced with main; the previous Side chat remains saved."
                        }
                    };
                    self.chat_widget
                        .add_info_message(message.into(), /*hint*/ None);
                } else {
                    // Keep the old view until the replacement is usable.
                    self.select_agent_thread(tui, app_server, thread_id).await?;
                    self.discard_side_thread(app_server, replacement_id).await;
                    self.sync_side_thread_ui();
                    self.chat_widget.add_error_message(format!(
                        "Failed to open synced conversation: {result:?}"
                    ));
                }
            }
            SideConversationAction::Fork { name } => {
                let name = if let Some(name) = name {
                    name
                } else {
                    let parent = match app_server
                        .thread_read(state.parent_thread_id, /*include_turns*/ false)
                        .await
                    {
                        Ok(parent) => parent,
                        Err(error) => {
                            self.chat_widget.add_error_message(format!(
                                "Could not read main's title; use /fork <name>: {error}"
                            ));
                            return Ok(());
                        }
                    };
                    let title = parent
                        .name
                        .as_deref()
                        .filter(|title| !title.trim().is_empty())
                        .unwrap_or("Untitled");
                    format!("{title} (fork)")
                };
                let mut config = self.current_thread_fork_config();
                config.ephemeral = false;
                // Only remove the exact guardrail suffix installed by Side. The saved fork has
                // normal capabilities; inherited tasks remain reference context, not active work.
                let side_policy = match state.storage {
                    crate::app_server_session::SideChatStorage::Temporary => {
                        codex_context_fragments::SideConversationPolicy::Temporary
                    }
                    crate::app_server_session::SideChatStorage::Attached => {
                        codex_context_fragments::SideConversationPolicy::Attached
                    }
                }
                .instructions(/*existing*/ None);
                let previous = config
                    .developer_instructions
                    .as_deref()
                    .map(|instructions| {
                        instructions
                            .strip_suffix(&side_policy)
                            .unwrap_or(instructions)
                            .trim_end()
                    })
                    .unwrap_or_default();
                config.developer_instructions =
                    Some(format!("{previous}\n\n{SAVED_FORK_INSTRUCTIONS}"));
                match app_server
                    .fork_thread_at(
                        &self.local_settings,
                        config,
                        thread_id,
                        /*last_turn_id*/ None,
                        /*before_turn_id*/ None,
                        crate::app_server_session::ForkGoalContinuation::DeferUntilNextTurn,
                        self.selected_server_profile(thread_id).as_ref(),
                    )
                    .await
                {
                    Ok(forked) => {
                        let saved_id = forked.session.thread_id;
                        // Persist the boundary without an inference turn so cold resume does not
                        // revive historical temporary-role restrictions.
                        if let Err(error) = app_server
                            .thread_inject_items(
                                saved_id,
                                vec![ContextualUserFragment::into(
                                    AdditionalContextDeveloperFragment::new(
                                        "saved_fork".to_string(),
                                        SAVED_FORK_INSTRUCTIONS.to_string(),
                                    ),
                                )],
                            )
                            .await
                        {
                            self.chat_widget.add_error_message(format!("Saved fork {saved_id}, but preparing ordinary-chat context failed: {error}"));
                            return Ok(());
                        }
                        if let Err(error) = app_server.thread_set_name(saved_id, name.clone()).await
                        {
                            self.chat_widget.add_error_message(format!(
                                "Saved fork {saved_id}, but naming failed: {error}"
                            ));
                        } else {
                            self.chat_widget.add_info_message(
                                format!("Saved {name} · {saved_id}. Open later with /resume."),
                                /*hint*/ None,
                            );
                        }
                    }
                    Err(error) => self.chat_widget.add_error_message(format!(
                        "Failed to save fork; Side is unchanged: {error}"
                    )),
                }
            }
        }
        Ok(())
    }
}
