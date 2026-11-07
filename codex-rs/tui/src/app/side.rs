//! Shared navigation for temporary sides and saved parallel conversations.
//! Inherited history is reference context, not an instruction to continue the parent task.
//! Keep upstream lifecycle helpers here to limit rebase conflicts despite this module's size.
//! Saved-only policy belongs in `app::parallel`.

use super::*;
use crate::chatwidget::InterruptedTurnNoticeMode;
use codex_app_server_protocol::ThreadUnsubscribeParams;
use codex_app_server_protocol::ThreadUnsubscribeResponse;
use codex_app_server_protocol::TurnInterruptParams;
use codex_app_server_protocol::TurnInterruptResponse;
use codex_context_fragments::ContextualUserFragment;
use codex_context_fragments::SideConversationPolicy;
use codex_context_fragments::side_conversation_boundary;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;

const SIDE_RENAME_BLOCK_MESSAGE: &str = "Side conversations are ephemeral and cannot be renamed.";
const SIDE_MAIN_THREAD_UNAVAILABLE_MESSAGE: &str =
    "'/side' is unavailable until the main thread is ready.";
const SIDE_NO_STARTED_CONVERSATION_MESSAGE: &str = concat!(
    "'/side' is unavailable until the current conversation has started. ",
    "Send a message first, then try /side again."
);
const SIDE_ALREADY_OPEN_MESSAGE: &str =
    "A side conversation is already open. Press ctrl + c to return before starting another.";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SideParentStatus {
    NeedsInput,
    NeedsApproval,
    Failed,
    Interrupted,
    Closed,
    Finished,
}

impl SideParentStatus {
    fn label(self, parent_is_main: bool) -> &'static str {
        match (self, parent_is_main) {
            (SideParentStatus::NeedsInput, true) => "main needs input",
            (SideParentStatus::NeedsInput, false) => "parent needs input",
            (SideParentStatus::NeedsApproval, true) => "main needs approval",
            (SideParentStatus::NeedsApproval, false) => "parent needs approval",
            (SideParentStatus::Failed, true) => "main failed",
            (SideParentStatus::Failed, false) => "parent failed",
            (SideParentStatus::Interrupted, true) => "main interrupted",
            (SideParentStatus::Interrupted, false) => "parent interrupted",
            (SideParentStatus::Closed, true) => "main closed",
            (SideParentStatus::Closed, false) => "parent closed",
            (SideParentStatus::Finished, true) => "main finished",
            (SideParentStatus::Finished, false) => "parent finished",
        }
    }

    fn is_actionable(self) -> bool {
        matches!(
            self,
            SideParentStatus::NeedsInput | SideParentStatus::NeedsApproval
        )
    }

    pub(super) fn for_request(request: &ServerRequest) -> Option<Self> {
        match request {
            ServerRequest::ToolRequestUserInput { .. } => Some(SideParentStatus::NeedsInput),
            ServerRequest::CommandExecutionRequestApproval { .. }
            | ServerRequest::FileChangeRequestApproval { .. }
            | ServerRequest::McpServerElicitationRequest { .. }
            | ServerRequest::PermissionsRequestApproval { .. }
            | ServerRequest::ApplyPatchApproval { .. }
            | ServerRequest::ExecCommandApproval { .. } => Some(SideParentStatus::NeedsApproval),
            ServerRequest::DynamicToolCall { .. }
            | ServerRequest::AttestationGenerate { .. }
            | ServerRequest::CurrentTimeRead { .. }
            | ServerRequest::ChatgptAuthTokensRefresh { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn side_boundary_prompt_marks_inherited_history_reference_only() {
        let item = App::side_boundary_prompt_item(CompanionKind::Side);
        let ResponseItem::Message { role, content, .. } = item else {
            panic!("expected hidden side boundary prompt to be a user message");
        };
        assert_eq!(role, "user");
        let [ContentItem::InputText { text }] = content.as_slice() else {
            panic!("expected hidden side boundary prompt text");
        };
        assert!(text.contains("Side conversation boundary."));
        assert!(text.contains("Everything before this boundary is inherited history"));
        assert!(text.contains("It is not your current task."));
        assert!(text.contains("Only messages submitted after this boundary are active"));
        assert!(text.contains("Do not continue, execute, or complete"));
        assert!(text.contains("separate from the main thread"));
        assert!(
            text.contains("External tools may be available according to this thread's current")
        );
        assert!(text.contains("Any tool calls or outputs visible before this boundary happened"));
        assert!(text.contains("Sub-agents are off-limits in this side conversation."));
    }

    #[test]
    fn side_start_error_message_explains_missing_first_prompt() {
        let err = color_eyre::eyre::eyre!(
            "thread/fork failed during TUI bootstrap: thread/fork failed: no rollout found for thread id 019da1a1-bed9-7a43-88a2-b49d43915021"
        );

        assert_eq!(
            App::side_start_error_message(&err, CompanionKind::Side),
            "'/side' is unavailable until the current conversation has started. Send a message first, then try /side again."
        );
    }

    #[test]
    fn side_start_error_message_uses_generic_start_wording() {
        let err = color_eyre::eyre::eyre!("transport disconnected");

        assert_eq!(
            App::side_start_error_message(&err, CompanionKind::Side),
            "Failed to start side conversation: transport disconnected"
        );
    }

    #[test]
    fn side_developer_instructions_appends_existing_policy() {
        let developer_instructions = App::side_developer_instructions(
            Some("Existing developer policy."),
            CompanionKind::Side,
            /*side_prompt*/ None,
        );

        assert!(developer_instructions.contains("Existing developer policy."));
        assert!(
            developer_instructions.contains("You are in a side conversation, not the main thread.")
        );
        assert!(
            developer_instructions.contains("Sub-agents are off-limits in this side conversation.")
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SideParentStatusChange {
    Set(SideParentStatus),
    Clear,
    ClearActionable,
}

impl SideParentStatusChange {
    pub(super) fn for_notification(notification: &ServerNotification) -> Option<Self> {
        match notification {
            ServerNotification::TurnStarted(_) => Some(SideParentStatusChange::Clear),
            ServerNotification::TurnCompleted(notification) => match &notification.turn.status {
                TurnStatus::Completed => {
                    Some(SideParentStatusChange::Set(SideParentStatus::Finished))
                }
                TurnStatus::Interrupted => {
                    Some(SideParentStatusChange::Set(SideParentStatus::Interrupted))
                }
                TurnStatus::Failed => Some(SideParentStatusChange::Set(SideParentStatus::Failed)),
                TurnStatus::InProgress => None,
            },
            ServerNotification::ThreadClosed(_) => {
                Some(SideParentStatusChange::Set(SideParentStatus::Closed))
            }
            ServerNotification::ItemStarted(_) | ServerNotification::ServerRequestResolved(_) => {
                Some(SideParentStatusChange::ClearActionable)
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct SideThreadState {
    /// Thread to return to when the current side conversation is dismissed.
    pub(super) parent_thread_id: ThreadId,
    pub(super) kind: CompanionKind,
    /// Parent-thread condition that changed while this side thread is visible.
    pub(super) parent_status: Option<SideParentStatus>,
    pub(super) opening_message: Option<crate::chatwidget::UserMessage>,
}

impl SideThreadState {
    pub(super) fn new(parent_thread_id: ThreadId) -> Self {
        Self {
            parent_thread_id,
            kind: CompanionKind::Side,
            parent_status: None,
            opening_message: None,
        }
    }

    pub(super) fn parallel(parent_thread_id: ThreadId) -> Self {
        Self {
            kind: CompanionKind::Parallel,
            ..Self::new(parent_thread_id)
        }
    }
}

impl App {
    pub(super) async fn running_side_thread_id(&self) -> Option<ThreadId> {
        for thread_id in self.side_threads.keys().copied() {
            if self.active_turn_id_for_thread(thread_id).await.is_some() {
                return Some(thread_id);
            }
        }
        None
    }

    pub(super) fn sync_side_thread_ui(&mut self) {
        if self.zoomed_thread.is_some() && self.zoomed_thread != self.open_pane_focus() {
            self.zoomed_thread = None;
        }
        let clear_side_ui = |chat_widget: &mut crate::chatwidget::ChatWidget| {
            chat_widget.set_side_conversation_context_label(/*label*/ None);
            chat_widget.set_side_conversation_active(/*active*/ false);
            chat_widget.set_composer_mode(crate::bottom_pane::ComposerMode::Main);
            chat_widget.clear_thread_rename_block();
            chat_widget.set_parallel_conversation_active(/*active*/ false);
            chat_widget.set_interrupted_turn_notice_mode(InterruptedTurnNoticeMode::Default);
        };
        let Some(active_thread_id) = self.current_displayed_thread_id() else {
            clear_side_ui(&mut self.chat_widget);
            return;
        };
        let Some((parent_thread_id, parent_status, kind)) = self
            .side_threads
            .get(&active_thread_id)
            .map(|state| (state.parent_thread_id, state.parent_status, state.kind))
        else {
            clear_side_ui(&mut self.chat_widget);
            if let Some(state) = self
                .side_threads
                .values()
                .find(|state| state.parent_thread_id == active_thread_id)
                && let Some(binding) = self.keymap.primary_hint(
                    crate::keymap::KeymapContext::Global,
                    "toggle_side_conversation",
                )
            {
                self.chat_widget
                    .set_side_conversation_context_label(Some(format!(
                        "{} for {}",
                        binding.display_label(),
                        state.kind.name()
                    )));
            }
            return;
        };

        match kind {
            CompanionKind::Side => {
                self.chat_widget
                    .set_thread_rename_block_message(SIDE_RENAME_BLOCK_MESSAGE);
                self.chat_widget
                    .set_side_conversation_active(/*active*/ true);
            }
            CompanionKind::Parallel => {
                self.chat_widget.clear_thread_rename_block();
                self.chat_widget
                    .set_parallel_conversation_active(/*active*/ true);
            }
        }
        self.chat_widget
            .set_interrupted_turn_notice_mode(InterruptedTurnNoticeMode::Suppress);
        let mut label_parts = Vec::new();
        let parent_is_main = self.primary_thread_id == Some(parent_thread_id);
        if parent_is_main {
            label_parts.push("from main thread".to_string());
        } else {
            let parent_label = self.thread_label(parent_thread_id);
            label_parts.push(format!("from parent thread ({parent_label})"));
        }
        if let Some(parent_status) = parent_status {
            label_parts.push(parent_status.label(parent_is_main).to_string());
        }
        if let Some(binding) = self.keymap.primary_hint(
            crate::keymap::KeymapContext::Global,
            "toggle_side_conversation",
        ) {
            label_parts.push(format!("{} to switch", binding.display_label()));
        }
        label_parts.push(format!(
            "{} to close",
            crate::key_hint::ctrl(KeyCode::Char('c')).display_label()
        ));
        let mode = match kind {
            CompanionKind::Side => crate::bottom_pane::ComposerMode::Side,
            CompanionKind::Parallel => crate::bottom_pane::ComposerMode::Parallel,
        };
        self.chat_widget.set_composer_mode(mode);
        let title = mode.name();
        self.chat_widget
            .set_side_conversation_context_label(Some(format!(
                "{title} {}",
                label_parts.join(" · ")
            )));
    }

    pub(super) fn active_side_parent_thread_id(&self) -> Option<ThreadId> {
        self.current_displayed_thread_id()
            .and_then(|thread_id| self.side_threads.get(&thread_id))
            .map(|state| state.parent_thread_id)
    }

    pub(super) fn side_retention_enabled(&self, tui: &tui::Tui) -> bool {
        self.local_settings.workspace_enabled(&self.config) && tui.is_owned_screen()
    }

    pub(super) fn conversation_focus_available(&self, tui: &tui::Tui) -> bool {
        self.side_retention_enabled(tui)
            && self.overlay.is_none()
            && (self.chat_widget.no_modal_or_popup_active()
                // Replaying the parent may reopen a completion for its parked draft.
                // The focused chooser hides it and owns the keystrokes.
                || (self.chooser_focus() == Some(sidebar::PaneFocus::Sidebar)
                    && self.sidebar_chooser_available()
                    && self.chat_widget.completion_can_yield_focus()))
    }

    pub(super) fn main_composer_shortcut_available(&self) -> bool {
        !self.reconnect.offline || self.active_side_parent_thread_id().is_none()
    }

    /// Zoom follows the displayed conversation even when its peer cannot be selected offline.
    pub(super) fn open_pane_focus(&self) -> Option<ThreadId> {
        let focused = self.current_displayed_thread_id()?;
        let child = self.side_pair_key(focused)?;
        (self.sidebar()?.visible_content() == Some(sidebar::SidebarContent::Chat(child)))
            .then_some(focused)
    }

    pub(super) fn switchable_side_child(&self) -> Option<ThreadId> {
        if self.reconnect.offline {
            return None;
        }
        self.side_pair_key(self.current_displayed_thread_id()?)
    }

    pub(super) fn side_pair_key(&self, thread_id: ThreadId) -> Option<ThreadId> {
        if self.side_threads.contains_key(&thread_id) {
            Some(thread_id)
        } else {
            self.side_thread_for_parent(thread_id)
        }
    }

    pub(super) fn side_thread_for_parent(&self, parent_thread_id: ThreadId) -> Option<ThreadId> {
        self.sidebars
            .get(&Some(parent_thread_id))
            .and_then(sidebar::SidebarPane::selected_chat)
            .or_else(|| {
                self.side_threads.iter().find_map(|(thread_id, state)| {
                    (state.parent_thread_id == parent_thread_id).then_some(*thread_id)
                })
            })
    }

    pub(super) fn set_side_parent_status(
        &mut self,
        parent_thread_id: ThreadId,
        status: Option<SideParentStatus>,
    ) {
        let mut changed = false;
        for state in self
            .side_threads
            .values_mut()
            .filter(|state| state.parent_thread_id == parent_thread_id)
        {
            if state.parent_status != status {
                state.parent_status = status;
                changed = true;
            }
        }
        if changed {
            self.sync_side_thread_ui();
        }
    }

    pub(super) fn clear_side_parent_action_status(&mut self, parent_thread_id: ThreadId) {
        let mut changed = false;
        for state in self
            .side_threads
            .values_mut()
            .filter(|state| state.parent_thread_id == parent_thread_id)
        {
            if state
                .parent_status
                .is_some_and(SideParentStatus::is_actionable)
            {
                state.parent_status = None;
                changed = true;
            }
        }
        if changed {
            self.sync_side_thread_ui();
        }
    }

    pub(super) fn apply_side_parent_status_change(
        &mut self,
        parent_thread_id: ThreadId,
        change: SideParentStatusChange,
    ) {
        match change {
            SideParentStatusChange::Set(status) => {
                self.set_side_parent_status(parent_thread_id, Some(status));
            }
            SideParentStatusChange::Clear => {
                self.set_side_parent_status(parent_thread_id, /*status*/ None);
            }
            SideParentStatusChange::ClearActionable => {
                self.clear_side_parent_action_status(parent_thread_id);
            }
        }
    }

    pub(super) async fn maybe_return_from_side(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
    ) -> bool {
        if self.overlay.is_none()
            && self.chat_widget.no_modal_or_popup_active()
            && self.chat_widget.composer_is_empty()
            && let Some(parent_thread_id) = self.active_side_parent_thread_id()
            && let Some(side_thread_id) = self.current_displayed_thread_id()
        {
            if self.side_threads[&side_thread_id].kind == CompanionKind::Parallel {
                return self
                    .close_parallel_conversation(tui, app_server, parent_thread_id, side_thread_id)
                    .await;
            }
            if self
                .select_agent_thread_and_discard_side(tui, app_server, parent_thread_id)
                .await
                .is_err()
            {
                return false;
            }
            self.active_side_parent_thread_id().is_none()
        } else {
            false
        }
    }

    pub(super) fn side_thread_to_discard_after_switch(
        &self,
        target_thread_id: ThreadId,
    ) -> Option<ThreadId> {
        let active_thread_id = self.current_displayed_thread_id()?;
        let (&side_thread_id, state) = self.side_threads.iter().next()?;
        if state.kind != CompanionKind::Side
            || target_thread_id == side_thread_id
            || target_thread_id == active_thread_id
        {
            return None;
        }

        (active_thread_id == side_thread_id || active_thread_id == state.parent_thread_id)
            .then_some(side_thread_id)
    }

    pub(super) async fn toggle_side_conversation(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
    ) -> Result<()> {
        let Some(active_thread_id) = self.current_displayed_thread_id() else {
            return Ok(());
        };
        let Some((&side_thread_id, state)) = self.side_threads.iter().next() else {
            return Ok(());
        };
        let target_thread_id = if active_thread_id == side_thread_id {
            state.parent_thread_id
        } else if active_thread_id == state.parent_thread_id {
            side_thread_id
        } else {
            return Ok(());
        };

        if state.kind == CompanionKind::Parallel
            && !self
                .parallel_navigation_target_available(
                    app_server,
                    state.parent_thread_id,
                    side_thread_id,
                    target_thread_id,
                )
                .await?
        {
            return Ok(());
        }
        self.select_agent_thread(tui, app_server, target_thread_id)
            .await?;
        if self.active_thread_id == Some(target_thread_id)
            && self.active_side_parent_thread_id().is_none()
        {
            self.surface_pending_inactive_thread_interactive_requests()
                .await?;
        }
        Ok(())
    }

    pub(super) async fn hide_focused_side_pane(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
    ) -> Result<()> {
        let Some(child) = self.current_displayed_thread_id() else {
            return Ok(());
        };
        let Some(side) = self.side_threads.get(&child) else {
            return Ok(());
        };
        let parent = side.parent_thread_id;
        self.select_agent_thread(tui, app_server, parent).await?;
        if self.current_displayed_thread_id() == Some(parent) {
            self.sidebar_mut().hide();
            tui.frame_requester().schedule_frame();
            self.focus_returned_parent_composer(parent).await?;
        }
        Ok(())
    }

    pub(super) async fn discard_side_thread(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) -> bool {
        if let Err(message) = self.interrupt_side_thread(app_server, thread_id).await {
            tracing::warn!("{message}");
            self.add_agents_overview_error(message);
            return false;
        }
        self.unsubscribe_side_thread(app_server, thread_id).await
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn unsubscribe_side_thread(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) -> bool {
        if let Err(err) = app_server.thread_unsubscribe(thread_id).await {
            let message =
                format!("Failed to close side conversation {thread_id}; it is still open: {err}");
            tracing::warn!("{message}");
            self.add_agents_overview_error(message);
            return false;
        }
        self.abandoned_side_threads.insert(thread_id);
        self.discard_thread_local_state(thread_id).await;
        true
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn discard_side_thread_in_background(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) {
        self.abandoned_side_threads.insert(thread_id);
        self.pending_app_server_requests
            .cancel_thread_verification(&thread_id.to_string());
        let turn_id = self
            .active_turn_id_for_thread(thread_id)
            .await
            .unwrap_or_default();
        let request_handle = app_server.request_handle();
        let interrupt_request_id = app_server.next_request_id();
        let retry_interrupt_request_id = app_server.next_request_id();
        let unsubscribe_request_id = app_server.next_request_id();

        self.discard_thread_local_state(thread_id).await;

        tokio::spawn(async move {
            let interrupt_result = request_handle
                .request_typed::<TurnInterruptResponse>(ClientRequest::TurnInterrupt {
                    request_id: interrupt_request_id,
                    params: TurnInterruptParams {
                        thread_id: thread_id.to_string(),
                        turn_id: turn_id.clone(),
                    },
                })
                .await;
            let interrupt_result = if let Err(error) = &interrupt_result
                && let Some(actual_turn_id) = active_turn_interrupt_race(error)
            {
                request_handle
                    .request_typed::<TurnInterruptResponse>(ClientRequest::TurnInterrupt {
                        request_id: retry_interrupt_request_id,
                        params: TurnInterruptParams {
                            thread_id: thread_id.to_string(),
                            turn_id: actual_turn_id,
                        },
                    })
                    .await
            } else {
                interrupt_result
            };
            if let Err(error) = interrupt_result {
                tracing::warn!(%error, "failed to interrupt side conversation");
            }
            if let Err(error) = request_handle
                .request_typed::<ThreadUnsubscribeResponse>(ClientRequest::ThreadUnsubscribe {
                    request_id: unsubscribe_request_id,
                    params: ThreadUnsubscribeParams {
                        thread_id: thread_id.to_string(),
                    },
                })
                .await
            {
                tracing::warn!(%error, "failed to unsubscribe side conversation");
            }
        });
    }

    pub(super) async fn discard_closed_side_thread(&mut self, thread_id: ThreadId) {
        self.abandoned_side_threads.insert(thread_id);
        self.discard_thread_local_state(thread_id).await;
    }

    pub(super) async fn discard_thread_local_state(&mut self, thread_id: ThreadId) {
        self.stop_automatic_thread_titles(thread_id);
        self.pending_app_server_requests
            .cancel_thread_verification(&thread_id.to_string());
        let app_event_tx = self.app_event_tx.clone();
        self.dynamic_tool_tasks
            .retain(|request_id, (source, task)| {
                if source == &thread_id.to_string() {
                    app_event_tx.send(AppEvent::DynamicToolCallCompleted {
                        request_id: request_id.clone(),
                        response: crate::dynamic_tools::failure_response(
                            "Source task was closed while handling a dynamic tool call",
                        ),
                    });
                    task.abort();
                    false
                } else {
                    true
                }
            });
        self.abort_thread_event_listener(thread_id);
        self.thread_event_channels.remove(&thread_id);
        self.pending_server_profiles.remove(&thread_id);
        self.agents_overview.activity.remove(&thread_id);
        self.sidebars.remove(&Some(thread_id));
        if let Some(side) = self.side_threads.remove(&thread_id)
            && let Some(pane) = self.sidebars.get_mut(&Some(side.parent_thread_id))
        {
            pane.remove_chat(thread_id);
        }
        self.side_companions.remove(&thread_id);
        self.agent_navigation.remove(thread_id);
        if self.active_thread_id == Some(thread_id) {
            self.clear_active_thread().await;
        } else {
            self.refresh_pending_thread_approvals().await;
        }
        self.forget_realtime_replay_thread(thread_id);
        self.sync_active_agent_label();
    }

    pub(super) async fn interrupt_side_thread(
        &self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) -> std::result::Result<(), String> {
        let saved = if let Some(channel) = self.thread_event_channels.get(&thread_id) {
            channel
                .store
                .lock()
                .await
                .session
                .as_ref()
                .is_some_and(|session| session.rollout_path.is_some())
        } else {
            false
        };
        if saved
            && self
                .side_threads
                .get(&thread_id)
                .is_some_and(|state| state.kind == CompanionKind::Parallel)
            && self.config.features.enabled(Feature::Goals)
        {
            let response = app_server
                .thread_goal_get(thread_id)
                .await
                .map_err(|error| format!("Failed to read side goal before closing: {error}"))?;
            if response.goal.is_some_and(|goal| {
                goal.status == codex_app_server_protocol::ThreadGoalStatus::Active
            }) {
                app_server
                    .thread_goal_set(
                        thread_id,
                        /*objective*/ None,
                        Some(codex_app_server_protocol::ThreadGoalStatus::Paused),
                        /*token_budget*/ None,
                    )
                    .await
                    .map_err(|error| {
                        format!("Failed to pause side goal before closing: {error}")
                    })?;
            }
        }
        let interrupt_result =
            if let Some(turn_id) = self.active_turn_id_for_thread(thread_id).await {
                app_server.turn_interrupt(thread_id, turn_id).await
            } else {
                app_server.startup_interrupt(thread_id).await
            };
        // Replay-only sides may still be running after reconnect, so always interrupt.
        // If the ephemeral thread is gone, let unsubscribe and local cleanup finish.
        if let Err(TypedRequestError::Server { method, source }) = &interrupt_result
            && method == "turn/interrupt"
            && source.code == -32600
            && source.message == format!("thread not found: {thread_id}")
            && self
                .thread_event_channels
                .get(&thread_id)
                .is_some_and(|channel| channel.attachment() == ThreadEventAttachment::ReplayOnly)
        {
            return Ok(());
        }
        interrupt_result.map_err(|err| {
            format!("Failed to close side conversation {thread_id}; it is still open: {err}")
        })
    }

    async fn keep_side_thread_visible_after_cleanup_failure(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) {
        if self.active_thread_id != Some(thread_id)
            && let Err(err) = self.select_agent_thread(tui, app_server, thread_id).await
        {
            tracing::warn!(
                "failed to restore side conversation after cleanup failure for {thread_id}: {err}"
            );
        }
    }

    async fn discard_side_thread_or_keep_visible(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) -> bool {
        let parent = self
            .side_threads
            .get(&thread_id)
            .filter(|state| state.kind == CompanionKind::Parallel)
            .map(|state| state.parent_thread_id);
        if self.discard_side_thread(app_server, thread_id).await {
            if let Some(parent) = parent
                && let Err(error) = self.close_side_selection(parent, thread_id)
            {
                self.chat_widget.add_error_message(format!(
                    "Side conversation closed, but its saved navigation could not be cleared: {error}"
                ));
            }
            true
        } else {
            self.keep_side_thread_visible_after_cleanup_failure(tui, app_server, thread_id)
                .await;
            false
        }
    }

    pub(super) fn side_developer_instructions(
        existing_instructions: Option<&str>,
        kind: CompanionKind,
        side_prompt: Option<&str>,
    ) -> String {
        let instructions = match kind {
            CompanionKind::Side => {
                let policy = SideConversationPolicy::Temporary.instructions(/*existing*/ None);
                match side_prompt.filter(|prompt| !prompt.trim().is_empty()) {
                    Some(prompt) => format!("{prompt}\n\n{policy}"),
                    None => policy,
                }
            }
            CompanionKind::Parallel => super::parallel::PARALLEL_DEVELOPER_INSTRUCTIONS.to_string(),
        };
        match existing_instructions {
            Some(existing_instructions) if existing_instructions.ends_with(&instructions) => {
                existing_instructions.to_string()
            }
            Some(existing_instructions) if !existing_instructions.trim().is_empty() => {
                format!("{existing_instructions}\n\n{instructions}")
            }
            _ => instructions,
        }
    }

    pub(super) fn side_boundary_prompt_item(kind: CompanionKind) -> ResponseItem {
        if kind == CompanionKind::Side {
            return side_conversation_boundary();
        }
        ResponseItem::Message {
            status: None,
            encrypted_content: None,
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: super::parallel::PARALLEL_BOUNDARY_PROMPT.to_string(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }
    }

    pub(super) fn side_boundary_items(&self, kind: CompanionKind) -> Vec<ResponseItem> {
        let mut items = vec![Self::side_boundary_prompt_item(kind)];
        if kind == CompanionKind::Side
            && let Some(prompt) = self
                .config
                .side_prompt
                .as_deref()
                .filter(|text| !text.trim().is_empty())
        {
            items.push(ResponseItem::Message {
                status: None,
                encrypted_content: None,
                id: None,
                role: "developer".to_string(),
                content: vec![ContentItem::InputText {
                    text: prompt.to_string(),
                }],
                phase: None,
                internal_chat_message_metadata_passthrough: None,
            });
        }
        if kind == CompanionKind::Side {
            items.push(ContextualUserFragment::into(
                SideConversationPolicy::Temporary,
            ));
        }
        items
    }

    pub(super) fn current_thread_fork_config(&self) -> Config {
        let mut fork_config = self.chat_widget.config_ref().clone();
        let parent_model = self.chat_widget.current_model();
        if !parent_model.trim().is_empty() {
            fork_config.model = Some(parent_model.to_string());
        }
        fork_config.model_reasoning_effort = self.chat_widget.current_reasoning_effort();
        fork_config.service_tier = self.chat_widget.configured_service_tier();
        fork_config
    }

    pub(super) fn side_fork_config(&self, kind: CompanionKind) -> Config {
        let mut fork_config = self.current_thread_fork_config();
        fork_config.ephemeral = kind == CompanionKind::Side;
        if kind == CompanionKind::Side {
            if let Some(model) = &self.config.side_model {
                fork_config.model = Some(model.clone());
            }
            if let Some(effort) = &self.config.side_reasoning_effort {
                fork_config.model_reasoning_effort = Some(effort.clone());
            }
            if let Some(tier) = &self.config.side_service_tier {
                fork_config.service_tier = Some(
                    codex_protocol::config_types::ServiceTier::from_request_value(tier)
                        .map(|tier| tier.request_value().to_string())
                        .unwrap_or_else(|| tier.clone()),
                );
            }
            fork_config.daybreak_enabled = false;
        }
        fork_config.developer_instructions = Some(Self::side_developer_instructions(
            fork_config.developer_instructions.as_deref(),
            kind,
            self.config.side_prompt.as_deref(),
        ));
        fork_config
    }

    pub(super) fn side_start_block_message(&self, kind: CompanionKind) -> Option<&'static str> {
        if self.primary_thread_id.is_none() {
            Some(match kind {
                CompanionKind::Side => SIDE_MAIN_THREAD_UNAVAILABLE_MESSAGE,
                CompanionKind::Parallel => {
                    super::parallel::PARALLEL_MAIN_THREAD_UNAVAILABLE_MESSAGE
                }
            })
        } else if self
            .current_displayed_thread_id()
            .and_then(|id| self.side_threads.get(&id))
            .is_some_and(|state| state.kind == CompanionKind::Side)
        {
            Some(SIDE_ALREADY_OPEN_MESSAGE)
        } else if kind == CompanionKind::Side
            && self
                .side_threads
                .values()
                .any(|state| state.kind == CompanionKind::Parallel)
        {
            Some(super::parallel::SIDE_WHILE_PARALLEL_OPEN_MESSAGE)
        } else {
            None
        }
    }

    pub(super) fn side_start_error_message(
        err: &color_eyre::Report,
        kind: CompanionKind,
    ) -> String {
        if err.chain().any(|cause| {
            let message = cause.to_string();
            message.contains("no rollout found for thread id")
                || message.contains("includeTurns is unavailable before first user message")
        }) {
            match kind {
                CompanionKind::Side => SIDE_NO_STARTED_CONVERSATION_MESSAGE,
                CompanionKind::Parallel => {
                    super::parallel::PARALLEL_NO_STARTED_CONVERSATION_MESSAGE
                }
            }
            .to_string()
        } else {
            match kind {
                CompanionKind::Side => format!("Failed to start side conversation: {err}"),
                CompanionKind::Parallel => format!("Failed to start parallel conversation: {err}"),
            }
        }
    }

    pub(super) fn restore_side_user_message(
        &mut self,
        user_message: Option<crate::chatwidget::UserMessage>,
    ) {
        if let Some(user_message) = user_message {
            self.chat_widget
                .restore_user_message_to_composer(user_message);
        }
    }

    pub(super) fn install_side_thread_snapshot(
        store: &mut ThreadEventStore,
        mut session: ThreadSessionState,
        _forked_turns: Vec<Turn>,
    ) {
        // The forked history remains available to the model through core state, but side
        // conversations should visually start at the side boundary.
        session.forked_from_id = None;
        store.set_session(session, Vec::new());
    }

    // Stack-budget exception: tracing instrumentation duplicates this large future in
    // unoptimized poll frames, overflowing the unchanged 16 MiB UI stack.
    pub(super) async fn select_agent_thread_and_discard_side(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) -> Result<()> {
        let side_thread_to_discard = self.side_thread_to_discard_after_switch(thread_id);
        self.select_agent_thread(tui, app_server, thread_id).await?;
        if self.active_thread_id == Some(thread_id)
            && let Some(side_thread_id) = side_thread_to_discard
        {
            self.discard_side_thread_in_background(app_server, side_thread_id)
                .await;
            self.surface_pending_inactive_thread_interactive_requests()
                .await?;
        }
        Ok(())
    }

    // Stack-budget exception: tracing instrumentation duplicates this large future in
    // unoptimized poll frames, overflowing the unchanged 16 MiB UI stack.
    pub(super) async fn start_companion_conversation(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        mut parent_thread_id: ThreadId,
        kind: CompanionKind,
        mut user_message: Option<crate::chatwidget::UserMessage>,
    ) -> Result<AppRunControl> {
        let workspace = kind == CompanionKind::Side && self.side_retention_enabled(tui);
        if let Some(pane) = self.sidebars.get_mut(&Some(parent_thread_id))
            && pane.creation.is_some()
        {
            // Retry and Back consume the saved attempt. A new /side must not overwrite its prompt.
            pane.show(
                sidebar::SidebarContent::Chooser,
                sidebar::PaneFocus::Sidebar,
            );
            tui.frame_requester().schedule_frame();
            self.restore_side_user_message(user_message);
            return Ok(AppRunControl::Continue);
        }
        if self.sidebars.values().any(|pane| pane.creation.is_some()) {
            self.restore_side_user_message(user_message.take());
            self.chat_widget
                .add_error_message("Finish or cancel the pending side conversation first.".into());
            return Ok(AppRunControl::Continue);
        }
        if let Some(message) = self.side_start_block_message(kind) {
            self.restore_side_user_message(user_message.take());
            self.sync_side_thread_ui();
            self.chat_widget.add_error_message(message.to_string());
            return Ok(AppRunControl::Continue);
        }
        if kind == CompanionKind::Side
            && let Some(existing) = self.side_thread_for_parent(parent_thread_id)
        {
            self.select_agent_thread(tui, app_server, existing).await?;
            if self.active_thread_id == Some(existing) {
                if let Some(message) = user_message.take() {
                    self.chat_widget
                        .submit_user_message_as_plain_user_turn(message);
                }
            } else {
                self.restore_side_user_message(user_message.take());
            }
            return Ok(AppRunControl::Continue);
        }
        if let Some(parent) = self.active_side_parent_thread_id() {
            self.select_agent_thread(tui, app_server, parent).await?;
            if self.current_displayed_thread_id() != Some(parent) {
                self.restore_side_user_message(user_message.take());
                self.sync_side_thread_ui();
                return Ok(AppRunControl::Continue);
            }
            parent_thread_id = parent;
        }
        if self.pending_server_profiles.contains_key(&parent_thread_id) {
            self.restore_side_user_message(user_message.take());
            self.sync_side_thread_ui();
            self.chat_widget
                .add_error_message("Wait for permissions to update before forking.".into());
            return Ok(AppRunControl::Continue);
        }

        let name = kind.name();
        self.refresh_in_memory_config_from_disk_best_effort(&format!(
            "starting a {name} conversation"
        ))
        .await;

        let fork_config = self.side_fork_config(kind);

        if let Some((&side_thread_id, state)) = self.side_threads.iter().next() {
            let previous_parallel_parent =
                (state.kind == CompanionKind::Parallel).then_some(state.parent_thread_id);
            if !self.thread_event_channels.contains_key(&side_thread_id) {
                // Restored UI pairing does not imply this client attached the saved side.
                self.discard_closed_side_thread(side_thread_id).await;
            } else if !self.discard_side_thread(app_server, side_thread_id).await {
                self.restore_side_user_message(user_message.take());
                self.sync_side_thread_ui();
                return Ok(AppRunControl::Continue);
            }
            if let Some(parent) = previous_parallel_parent
                && let Err(error) = self.close_side_selection(parent, side_thread_id)
            {
                self.restore_side_user_message(user_message.take());
                self.sync_side_thread_ui();
                self.chat_widget
                    .add_error_message(format!("Failed to close parallel navigation: {error}"));
                return Ok(AppRunControl::Continue);
            }
        }

        self.session_telemetry.counter(
            match kind {
                CompanionKind::Side => "codex.thread.side",
                CompanionKind::Parallel => "codex.thread.parallel",
            },
            /*inc*/ 1,
            &[("source", "slash_command")],
        );
        let request = uuid::Uuid::new_v4();
        if workspace {
            let pane = self.sidebars.entry(Some(parent_thread_id)).or_default();
            pane.creation = Some(sidebar::SideCreation {
                owner: app_server.backend_id(),
                state: sidebar::CreationState::Waiting {
                    request,
                    started: Instant::now(),
                },
                message: user_message.take(),
            });
            pane.show(
                sidebar::SidebarContent::Chooser,
                sidebar::PaneFocus::Sidebar,
            );
            let size = tui.prepare_draw_size()?;
            self.render_chat_widget_frame(tui, size)?;
        }
        let fork = app_server.sidebar_fork(
            self.local_settings.clone(),
            fork_config,
            parent_thread_id,
            self.selected_server_profile(parent_thread_id),
            self.side_boundary_items(kind),
        );
        let create = async move {
            fork.await
                .map(Box::new)
                .map_err(|err| Self::side_start_error_message(&err, kind))
        };
        if workspace {
            let tx = self.app_event_tx.clone();
            let backend_id = app_server.backend_id();
            tokio::spawn(async move {
                tx.send(AppEvent::SideCreated {
                    backend_id,
                    parent: parent_thread_id,
                    request,
                    result: create.await,
                });
            });
        } else {
            self.apply_side_creation(
                tui,
                app_server,
                parent_thread_id,
                kind,
                user_message,
                /*enter*/ true,
                create.await,
            )
            .await;
        }
        Ok(AppRunControl::Continue)
    }

    pub(super) async fn finish_side_creation(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        parent: ThreadId,
        request: uuid::Uuid,
        result: Result<Box<crate::app_server_session::SidebarFork>, String>,
    ) {
        let enter = !self.reconnect.offline
            && self.current_displayed_thread_id() == Some(parent)
            && self.chooser_focus() == Some(sidebar::PaneFocus::Sidebar)
            && self.conversation_focus_available(tui);
        let pending = self
            .sidebars
            .get_mut(&Some(parent))
            .and_then(|pane| pane.creation.as_mut())
            .filter(|creation| creation.request() == Some(request));
        let Some(pending) = pending else {
            if let Ok(fork) = result
                && let Err(error) = app_server
                    .thread_unsubscribe(fork.thread.session.thread_id)
                    .await
            {
                tracing::warn!(%error, "failed to release unclaimed side chat");
            }
            return;
        };
        tui.frame_requester().schedule_frame();
        if let Err(message) = result {
            pending.state = sidebar::CreationState::Failed {
                message,
                cleanup: None,
            };
            self.sync_side_thread_ui();
            return;
        }
        let message = pending.message.take();
        self.apply_side_creation(
            tui,
            app_server,
            parent,
            CompanionKind::Side,
            message,
            enter,
            result,
        )
        .await
    }

    async fn apply_side_creation(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        parent_thread_id: ThreadId,
        kind: CompanionKind,
        mut user_message: Option<crate::chatwidget::UserMessage>,
        enter: bool,
        result: Result<Box<crate::app_server_session::SidebarFork>, String>,
    ) {
        let name = kind.name();
        match result {
            Ok(fork) => {
                app_server.observe_sidebar_fork(&fork);
                let forked = fork.thread;
                let child_thread_id = forked.session.thread_id;
                if kind == CompanionKind::Parallel
                    && let Err(error) = app_server
                        .save_side_conversation(parent_thread_id, child_thread_id)
                        .await
                {
                    self.discard_side_thread(app_server, child_thread_id).await;
                    self.restore_side_user_message(user_message.take());
                    self.sync_side_thread_ui();
                    self.chat_widget
                        .add_error_message(format!("Failed to save parallel navigation: {error}"));
                    return;
                }
                if let Err(err) = fork.boundary {
                    let message =
                        format!("Failed to prepare side conversation {child_thread_id}: {err}");
                    let discarded = self.discard_side_thread(app_server, child_thread_id).await;
                    if let Some(pending) = self
                        .sidebars
                        .get_mut(&Some(parent_thread_id))
                        .and_then(|pane| pane.creation.as_mut())
                    {
                        pending.state = sidebar::CreationState::Failed {
                            message,
                            cleanup: (!discarded).then_some(child_thread_id),
                        };
                        pending.message = user_message;
                    } else {
                        if !discarded {
                            self.side_threads
                                .insert(child_thread_id, SideThreadState::new(parent_thread_id));
                            self.keep_side_thread_visible_after_cleanup_failure(
                                tui,
                                app_server,
                                child_thread_id,
                            )
                            .await;
                        }
                        self.restore_side_user_message(user_message);
                        self.chat_widget.add_error_message(message);
                    }
                    return;
                }
                let already_selected = self.active_thread_id == Some(child_thread_id);
                let enter = enter || already_selected;
                if fork.inherited_profile
                    && let Some(profile) = &forked.session.active_permission_profile
                {
                    self.agents_overview
                        .selected_permission_profiles
                        .insert(child_thread_id, profile.id.clone());
                }
                if !enter {
                    self.side_companions
                        .entry(child_thread_id)
                        .or_insert_with(|| SideCompanion::empty(forked.session.cwd.clone()));
                }
                let channel = self.ensure_thread_channel(child_thread_id);
                channel.backend_id = Some(app_server.backend_id());
                {
                    let mut store = channel.store.lock().await;
                    Self::install_side_thread_snapshot(&mut store, forked.session, forked.turns);
                }
                let mut child = match kind {
                    CompanionKind::Side => SideThreadState::new(parent_thread_id),
                    CompanionKind::Parallel => SideThreadState::parallel(parent_thread_id),
                };
                child.parent_status = self
                    .side_thread_for_parent(parent_thread_id)
                    .and_then(|id| self.side_threads.get(&id))
                    .and_then(|side| side.parent_status);
                if !enter {
                    child.opening_message = user_message.take();
                }
                self.side_threads.insert(child_thread_id, child);
                // `thread/started` is delivered after the fork response; seed navigation before
                // the first selection without blocking on another app-server read.
                self.upsert_agent_picker_thread(
                    child_thread_id,
                    /*agent_nickname*/ None,
                    /*agent_role*/ None,
                    /*is_closed*/ false,
                );
                self.sync_agent_picker_thread(child_thread_id);
                self.sidebars
                    .entry(Some(parent_thread_id))
                    .or_default()
                    .chat_ready(child_thread_id);
                if !enter {
                    self.sync_side_thread_ui();
                    return;
                }
                if already_selected {
                    // Selection normally skips the live thread. Replay the initialized snapshot
                    // even if the picker exposed this child before the boundary RPC completed.
                    self.store_active_thread_receiver().await;
                    self.active_thread_id = None;
                }
                if let Err(err) = self
                    .select_agent_thread_and_discard_side(tui, app_server, child_thread_id)
                    .await
                {
                    let discarded = self
                        .discard_side_thread_or_keep_visible(tui, app_server, child_thread_id)
                        .await;
                    if discarded
                        && self.active_thread_id != Some(parent_thread_id)
                        && let Err(restore_err) = self
                            .select_agent_thread(tui, app_server, parent_thread_id)
                            .await
                    {
                        tracing::warn!(
                            "failed to restore parent thread after side conversation switch failure: {restore_err}"
                        );
                    }
                    self.restore_side_user_message(user_message.take());
                    self.chat_widget.add_error_message(format!(
                        "Failed to switch into side conversation {child_thread_id}: {err}"
                    ));
                    return;
                }
                if self.active_thread_id == Some(child_thread_id) {
                    if let Some(user_message) = user_message.take() {
                        let _ = self
                            .chat_widget
                            .submit_user_message_as_plain_user_turn(user_message);
                    }
                } else {
                    self.discard_side_thread_or_keep_visible(tui, app_server, child_thread_id)
                        .await;
                    self.restore_side_user_message(user_message.take());
                    self.chat_widget.add_error_message(format!(
                        "Failed to switch into {name} conversation {child_thread_id}."
                    ));
                }
            }
            Err(err) => {
                self.restore_side_user_message(user_message.take());
                self.chat_widget
                    .set_side_conversation_context_label(/*label*/ None);
                self.chat_widget.add_error_message(err);
            }
        }
    }
}

#[cfg(test)]
#[path = "side_parallel_tests.rs"]
mod parallel_tests;

#[cfg(test)]
#[path = "side_navigation_tests.rs"]
mod navigation_tests;
