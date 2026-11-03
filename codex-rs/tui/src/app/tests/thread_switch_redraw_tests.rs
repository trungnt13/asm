use super::*;
use crate::custom_terminal::test_support::last_rendered_buffer;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn owned_thread_switch_preserves_frame_until_current_history_finishes() -> Result<()> {
    let (mut app, mut events, _ops) = make_test_app_with_channels().await;
    app.config.features.disable(Feature::TuiWorkspace)?;
    let old_thread = ThreadId::new();
    app.enqueue_primary_thread_session(
        test_thread_session(old_thread, test_path_buf("/tmp/previous")),
        Vec::new(),
    )
    .await?;
    while events.try_recv().is_ok() {}
    app.transcript_cells = vec![plain_line_cell("Previous thread transcript")];
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.set_owned_screen(/*owned*/ true)?;
    app.render_owned_transcript(&mut tui, Size::new(/*width*/ 80, /*height*/ 24))?;
    let old_frame = last_rendered_buffer(&tui.terminal).clone();
    let mut server = crate::start_embedded_app_server_for_picker(&app.config).await?;
    let next_thread = ThreadId::new();
    app.active_thread_id = Some(next_thread);
    app.render_thread_snapshot(
        &mut tui,
        &server,
        next_thread,
        ThreadEventSnapshot {
            output_tps: None,
            inferred_session: false,
            delegated_turns: Vec::new(),
            session: Some(test_thread_session(next_thread, test_path_buf("/tmp/next"))),
            turns: vec![test_turn(
                "next-turn",
                TurnStatus::Completed,
                vec![ThreadItemPayload::UserMessage {
                    id: "next-user".to_string(),
                    client_id: None,
                    content: vec![AppServerUserInput::Text {
                        text: "Current destination history".to_string(),
                        text_elements: Vec::new(),
                    }],
                }],
            )],
            events: Vec::new(),
            active_reasoning_item: None,
            reasoning_state: Default::default(),
            latest_turn_id: None,
            input_state: None,
        },
        /*resume_restored_queue*/ false,
    )?;
    assert!(app.awaiting_thread_switch_replay(&tui));
    let mut finished = false;
    while let Ok(event) = events.try_recv() {
        let finishes_replay =
            matches!(event, AppEvent::SideReplayFinished(id) if id == next_thread);
        if !finished {
            assert!(app.awaiting_thread_switch_replay(&tui));
        }
        app.handle_event(&mut tui, &mut server, event).await?;
        assert_eq!(last_rendered_buffer(&tui.terminal), &old_frame);
        if finishes_replay {
            finished = true;
            assert!(!app.awaiting_thread_switch_replay(&tui));
        }
    }
    assert!(finished);
    assert_eq!(app.pending_thread_switch_resets, 0);
    app.render_owned_transcript(&mut tui, Size::new(/*width*/ 80, /*height*/ 24))?;
    let text = last_rendered_buffer(&tui.terminal)
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(text.contains("Current destination history"));
    assert!(!text.contains("Previous thread transcript"));
    server.shutdown().await?;
    tui.set_owned_screen(/*owned*/ false)?;
    Ok(())
}
