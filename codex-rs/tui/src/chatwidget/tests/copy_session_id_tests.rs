use super::*;
use crate::clipboard_copy::CopyFormat;
use crate::clipboard_copy::CopyStatus;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn slash_copyid_copies_displayed_session_without_status_or_response() {
    for mode in ["idle", "running", "side", "parent-owned"] {
        let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
        let session_id = "00000000-0000-0000-0000-000000000123";
        chat.thread_id = Some(ThreadId::from_string(session_id).expect("valid thread ID"));
        if mode == "running" {
            handle_turn_started(&mut chat, "active");
        }
        if mode == "side" {
            chat.active_side_conversation = true;
        }
        if mode == "parent-owned" {
            chat.set_parent_owned_thread();
        }
        while rx.try_recv().is_ok() {}
        chat.bottom_pane
            .set_composer_text("/copyid".to_string(), Vec::new(), Vec::new());

        chat.handle_key_event(KeyEvent::from(KeyCode::Enter));

        let copied = std::iter::from_fn(|| rx.try_recv().ok()).find_map(|event| match event {
            AppEvent::CopySelection {
                text,
                label,
                format,
            } => Some((text.to_string(), label, format)),
            _ => None,
        });
        assert_eq!(
            copied,
            Some((
                session_id.to_string(),
                "Session ID".to_string(),
                CopyFormat::PlainText,
            )),
            "{mode}"
        );
        assert!(chat.bottom_pane.no_modal_or_popup_active());
        assert_eq!(chat.bottom_pane.composer_text(), "");
        assert_matches!(op_rx.try_recv(), Err(TryRecvError::Empty));
    }
}

#[tokio::test]
async fn slash_copyid_reports_missing_session_without_copying() {
    let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual(/*model_override*/ None).await;

    chat.dispatch_command(SlashCommand::CopyId);

    let rendered = match rx.try_recv() {
        Ok(AppEvent::InsertHistoryCell(cell)) => {
            lines_to_single_string(&cell.display_lines(/*width*/ 80))
        }
        other => panic!("expected missing session ID error, got {other:?}"),
    };
    insta::assert_snapshot!(rendered);
    assert!(chat.bottom_pane.no_modal_or_popup_active());
    assert_matches!(rx.try_recv(), Err(TryRecvError::Empty));
    assert_matches!(op_rx.try_recv(), Err(TryRecvError::Empty));
}

#[tokio::test]
async fn slash_copyid_uses_existing_clipboard_result_feedback() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    let session_id = "00000000-0000-0000-0000-000000000456";
    chat.thread_id = Some(ThreadId::from_string(session_id).expect("valid thread ID"));
    chat.dispatch_command(SlashCommand::CopyId);
    let label = match rx.try_recv() {
        Ok(AppEvent::CopySelection {
            text,
            label,
            format,
        }) => {
            assert_eq!((text.as_ref(), format), (session_id, CopyFormat::PlainText));
            label
        }
        other => panic!("expected direct session ID copy, got {other:?}"),
    };

    chat.show_copy_result(&label, Ok(CopyStatus::Confirmed));
    let success = lines_to_single_string(&drain_insert_history(&mut rx)[0]);
    chat.show_copy_result(&label, Err("clipboard unavailable".to_string()));
    let failure = lines_to_single_string(&drain_insert_history(&mut rx)[0]);
    insta::assert_snapshot!(format!("{success}\n{failure}"));
}
