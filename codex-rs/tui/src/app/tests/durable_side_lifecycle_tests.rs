use super::session_lifecycle_requests::HistoryCapabilities;
use super::session_lifecycle_requests::RealtimeRequestBehavior;
use super::session_lifecycle_requests::TurnStartBehavior;
use super::session_lifecycle_requests::recorded_params;
use super::session_lifecycle_requests::start_recording_app_server_with_realtime_speech;
use super::*;
use crate::app::session_lifecycle::ThreadAttachPresentation;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn parallel_replacement_and_cancel_keep_history_and_resume_events() -> Result<()> {
    let (mut app, mut app_events, _ops) = make_test_app_with_channels().await;
    let config = app.chat_widget.config_ref().clone();
    let parent_id = ThreadId::from_string(
        &app_test_support::create_fake_rollout(
            config.codex_home.as_path(),
            "2025-01-05T12-00-00",
            "2025-01-05T12:00:00Z",
            "Parent history must remain saved",
            Some(config.model_provider_id.as_str()),
            /*git_info*/ None,
        )
        .expect("synthetic parent rollout"),
    )?;
    let (mut server, requests, proxy) = start_recording_app_server_with_realtime_speech(
        &config,
        HistoryCapabilities::Current,
        /*blocked_thread_list*/ None,
        /*failed_thread_name*/ None,
        crate::app_server_session::ThreadParamsMode::Embedded,
        RealtimeRequestBehavior::Forward,
        TurnStartBehavior::Accept,
        codex_config::LoaderOverrides::without_managed_config_for_tests(),
    )
    .await?;
    server = server.with_side_conversations(&app.config.codex_home, &app.app_server_target);
    let parent = server
        .resume_thread(
            &app.local_settings,
            config.clone(),
            parent_id,
            crate::app_server_session::ResumeModelSettings::RestoreFromThread,
        )
        .await?;
    app.enqueue_primary_thread_session(parent.session, parent.turns)
        .await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    Box::pin(app.start_companion_conversation(
        &mut tui,
        &mut server,
        parent_id,
        CompanionKind::Parallel,
        crate::app_event::SideConversationMode::Side,
        /*user_message*/ None,
    ))
    .await?;
    let first_id = app.current_displayed_thread_id().expect("first side");
    assert_ne!(first_id, parent_id);
    assert!(
        !server
            .thread_read(first_id, /*include_turns*/ false)
            .await?
            .ephemeral
    );

    let companion_request = |source: ThreadId, call_id: &str| {
        codex_app_server_client::AppServerEvent::ServerRequest(Box::new(
            codex_app_server_protocol::ServerRequest::DynamicToolCall {
                request_id: AppServerRequestId::String(call_id.to_string()),
                params: codex_app_server_protocol::DynamicToolCallParams {
                    thread_id: source.to_string(),
                    turn_id: "peer-turn".to_string(),
                    call_id: call_id.to_string(),
                    namespace: None,
                    tool: "send_companion_message".to_string(),
                    arguments: serde_json::json!({"message": "coordinate"}),
                },
            },
        ))
    };
    // The proxy accepts delivery; registration support is exercised separately
    // by the app-server integration tests.
    app.side_threads
        .get_mut(&first_id)
        .expect("active pair")
        .messaging_available = true;
    for (source, recipient) in [(parent_id, first_id), (first_id, parent_id)] {
        while app_events.try_recv().is_ok() {}
        requests.lock().expect("request recorder lock").clear();
        app.handle_app_server_event(&server, companion_request(source, "peer-route"))
            .await;
        let response = std::iter::from_fn(|| app_events.try_recv().ok())
            .find_map(|event| match event {
                AppEvent::DynamicToolCallCompleted { response, .. } => Some(response),
                _ => None,
            })
            .expect("peer delivery receipt");
        assert!(response.success);
        let params: codex_app_server_protocol::TurnStartParams = serde_json::from_value(
            recorded_params(&requests, "turn/start")
                .pop()
                .expect("peer turn request"),
        )?;
        assert_eq!(
            params,
            codex_app_server_protocol::TurnStartParams {
                thread_id: recipient.to_string(),
                assistant_message: Some(format!(
                    "Message Type: MESSAGE\nTask name: {recipient}\nSender: {source}\nPayload:\ncoordinate"
                )),
                ..Default::default()
            }
        );
    }
    requests.lock().expect("request recorder lock").clear();

    Box::pin(app.start_companion_conversation(
        &mut tui,
        &mut server,
        first_id,
        CompanionKind::Parallel,
        crate::app_event::SideConversationMode::Side,
        /*user_message*/ None,
    ))
    .await?;
    let second_id = app.current_displayed_thread_id().expect("replacement side");
    assert_ne!(second_id, first_id);
    assert_eq!(app.active_side_parent_thread_id(), Some(parent_id));
    assert_eq!(app.side_threads.len(), 1);
    assert_eq!(
        server
            .thread_read(second_id, /*include_turns*/ false)
            .await?
            .forked_from_id,
        Some(parent_id.to_string())
    );
    assert!(
        !server
            .thread_read(first_id, /*include_turns*/ false)
            .await?
            .ephemeral
    );

    let store = crate::side_conversations::SideConversationStore::new(
        &app.config.codex_home,
        &app.app_server_target,
    );
    let open_pair = store.side(second_id)?.expect("saved parallel record");
    assert!(Box::pin(app.maybe_return_from_side(&mut tui, &mut server)).await);
    assert_eq!(app.current_displayed_thread_id(), Some(parent_id));
    assert!(app.side_threads.is_empty());
    assert!(app.abandoned_side_threads.contains(&second_id));
    assert_eq!(store.pair(parent_id)?, None);
    assert_eq!(store.side(second_id)?, Some(open_pair));

    while app_events.try_recv().is_ok() {}
    requests.lock().expect("request recorder lock").clear();
    app.handle_app_server_event(&server, companion_request(parent_id, "closed-peer"))
        .await;
    let response = std::iter::from_fn(|| app_events.try_recv().ok())
        .find_map(|event| match event {
            AppEvent::DynamicToolCallCompleted { response, .. } => Some(response),
            _ => None,
        })
        .expect("closed pair receipt");
    assert!(!response.success);
    assert!(recorded_params(&requests, "turn/start").is_empty());

    let saved_path = server
        .thread_read(second_id, /*include_turns*/ false)
        .await?
        .path
        .expect("saved child rollout");
    let saved_history = std::fs::read_to_string(&saved_path)?;
    assert!(saved_history.contains("Parent history must remain saved"));

    let resumed = server
        .resume_thread(
            &app.local_settings,
            config.clone(),
            second_id,
            crate::app_server_session::ResumeModelSettings::PreserveExistingThread,
        )
        .await?;
    assert!(
        resumed.turns.is_empty(),
        "the inherited parent history stays hidden"
    );
    Box::pin(app.replace_chat_widget_with_app_server_thread(
        &mut tui,
        &server,
        resumed,
        ThreadAttachPresentation::SessionLineage,
        /*initial_user_message*/ None,
    ))
    .await?;
    assert_eq!(app.current_displayed_thread_id(), Some(second_id));
    assert_eq!(app.active_side_parent_thread_id(), None);
    assert_eq!(app.primary_thread_id, Some(second_id));
    assert!(app.side_threads.is_empty());
    assert!(!app.chat_widget.parallel_conversation_active());
    assert!(!app.chat_widget.side_conversation_active());
    assert!(!app.abandoned_side_threads.contains(&second_id));
    while app_events.try_recv().is_ok() {}
    if let Some(receiver) = app.active_thread_rx.as_mut() {
        while receiver.try_recv().is_ok() {}
    }
    app.enqueue_thread_notification(
        second_id,
        agent_message_delta_notification(second_id, "new-turn", "new-item", "after resume"),
    )
    .await?;
    assert!(
        app.active_thread_rx
            .as_mut()
            .expect("resumed receiver")
            .try_recv()
            .is_ok()
    );
    server.shutdown().await?;
    proxy.await??;
    Ok(())
}
