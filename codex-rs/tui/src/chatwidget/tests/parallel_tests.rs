use super::*;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

#[tokio::test]
async fn slash_rename_with_args_updates_saved_parallel_name() {
    let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.set_parallel_conversation_active(/*active*/ true);

    chat.dispatch_command_with_args(SlashCommand::Rename, "investigate".to_string(), Vec::new());

    assert_matches!(
        rx.try_recv(),
        Ok(AppEvent::CodexOp(Op::SetThreadName { name })) if name == "investigate"
    );
    assert!(rx.try_recv().is_err());
    assert!(op_rx.try_recv().is_err());
}

#[tokio::test]
async fn slash_review_opens_normal_picker_in_parallel() {
    let (mut chat, _rx, mut op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.set_parallel_conversation_active(/*active*/ true);

    chat.dispatch_command(SlashCommand::Review);

    assert_chatwidget_snapshot!(
        "slash_review_picker_in_parallel",
        render_bottom_popup(&chat, /*width*/ 80)
    );
    assert!(op_rx.try_recv().is_err());
}

#[tokio::test]
async fn parallel_can_start_while_running_and_from_saved_parallel() {
    for already_parallel in [false, true] {
        for prompt in ["/parallel", "/parallel explore the codebase"] {
            let (mut chat, mut rx, mut op_rx) =
                make_chatwidget_manual(/*model_override*/ None).await;
            let parent_thread_id = ThreadId::new();
            chat.thread_id = Some(parent_thread_id);
            chat.set_parallel_conversation_active(already_parallel);
            chat.on_task_started();
            chat.bottom_pane
                .set_composer_text(prompt.to_string(), Vec::new(), Vec::new());

            chat.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

            assert_matches!(rx.try_recv(), Ok(AppEvent::FollowTranscript));
            let Ok(AppEvent::StartParallel {
                parent_thread_id: actual,
                user_message,
            }) = rx.try_recv()
            else {
                panic!("expected saved parallel event for {prompt}");
            };
            assert_eq!(actual, parent_thread_id);
            assert_eq!(
                user_message,
                prompt.strip_prefix("/parallel ").map(UserMessage::from)
            );
            assert!(op_rx.try_recv().is_err());
            assert!(chat.input_queue.queued_user_messages.is_empty());
            if !already_parallel && prompt.contains(' ') {
                let width = 80;
                let height = chat.desired_height(width);
                let mut terminal =
                    Terminal::new(TestBackend::new(width, height)).expect("create terminal");
                terminal
                    .draw(|f| chat.render(f.area(), f.buffer_mut()))
                    .expect("draw parallel starting footer");
                assert_chatwidget_snapshot!(
                    "parallel_starting",
                    normalized_backend_snapshot(terminal.backend())
                );
            }
        }
    }
}

#[tokio::test]
async fn side_and_parallel_modes_are_mutually_exclusive() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.set_side_conversation_active(/*active*/ true);
    assert_eq!(
        (
            chat.side_conversation_active(),
            chat.parallel_conversation_active()
        ),
        (true, false)
    );
    chat.set_parallel_conversation_active(/*active*/ true);
    assert_eq!(
        (
            chat.side_conversation_active(),
            chat.parallel_conversation_active()
        ),
        (false, true)
    );
    chat.set_side_conversation_active(/*active*/ true);
    assert_eq!(
        (
            chat.side_conversation_active(),
            chat.parallel_conversation_active()
        ),
        (true, false)
    );
    chat.set_side_conversation_active(/*active*/ false);
    assert_eq!(
        (
            chat.side_conversation_active(),
            chat.parallel_conversation_active()
        ),
        (false, false)
    );
}

#[tokio::test]
async fn parallel_context_label_preserves_status_line_snapshot() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.show_welcome_banner = false;
    chat.local_settings.tui.status_line = Some(vec!["model-name".to_string()]);
    chat.refresh_status_line();
    chat.set_parallel_conversation_active(/*active*/ true);
    chat.set_side_conversation_context_label(Some(
        "Parallel from main thread · ctrl+/ to switch · ctrl+c to close".to_string(),
    ));

    let width = 80;
    let height = chat.desired_height(width);
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("create terminal");
    terminal
        .draw(|f| chat.render(f.area(), f.buffer_mut()))
        .expect("draw parallel conversation footer");
    assert_chatwidget_snapshot!(
        "parallel_context_label_preserves_status_line",
        terminal.backend()
    );
}
