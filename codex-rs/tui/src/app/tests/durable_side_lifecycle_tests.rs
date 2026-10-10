use super::session_lifecycle_requests::HistoryCapabilities;
use super::session_lifecycle_requests::RealtimeRequestBehavior;
use super::session_lifecycle_requests::TurnStartBehavior;
use super::session_lifecycle_requests::recorded_params;
use super::session_lifecycle_requests::start_recording_app_server_with_realtime_speech;
use super::*;
use crate::app::session_lifecycle::ThreadAttachPresentation;
use crate::app_event::SideConversationAction;
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

    // A completion drained during switching must leave its follow-up on Main.
    Box::pin(app.select_agent_thread(&mut tui, &mut server, parent_id)).await?;
    app.enqueue_thread_notification(
        parent_id,
        turn_started_notification(parent_id, "queued-main-turn"),
    )
    .await?;
    app.drain_active_thread_events(&mut tui, &server).await?;
    app.chat_widget
        .apply_external_edit("Main-only queued follow-up".to_string());
    app.chat_widget
        .handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(
        app.chat_widget.queued_user_message_texts(),
        vec!["Main-only queued follow-up".to_string()],
    );
    while app_events.try_recv().is_ok() {}
    requests.lock().expect("request recorder lock").clear();
    app.enqueue_thread_notification(
        parent_id,
        turn_completed_notification(parent_id, "queued-main-turn", TurnStatus::Completed),
    )
    .await?;
    Box::pin(app.select_agent_thread(&mut tui, &mut server, first_id)).await?;
    while let Ok(event) = app_events.try_recv() {
        assert!(!matches!(event, AppEvent::CodexOp(Op::UserTurn { .. })));
    }
    assert!(recorded_params(&requests, "turn/start").is_empty());
    Box::pin(app.select_agent_thread(&mut tui, &mut server, parent_id)).await?;
    while let Ok(event) = app_events.try_recv() {
        Box::pin(app.handle_event(&mut tui, &mut server, event)).await?;
    }
    let request = recorded_params(&requests, "turn/start")
        .pop()
        .expect("Main's restored follow-up");
    assert_eq!(
        (
            request["threadId"].clone(),
            request["input"][0]["text"].clone()
        ),
        (
            serde_json::json!(parent_id.to_string()),
            serde_json::json!("Main-only queued follow-up"),
        ),
    );
    app.enqueue_thread_notification(
        parent_id,
        turn_completed_notification(parent_id, "forwarded-turn", TurnStatus::Completed),
    )
    .await?;
    Box::pin(app.select_agent_thread(&mut tui, &mut server, first_id)).await?;

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
    let parent_selection = store.pair(parent_id)?;
    let first_record = store.side(first_id)?;
    for thread_id in [second_id, parent_id] {
        Box::pin(app.select_agent_thread(&mut tui, &mut server, thread_id)).await?;
        Box::pin(app.start_companion_conversation(
            &mut tui,
            &mut server,
            thread_id,
            CompanionKind::Side,
            crate::app_event::SideConversationMode::Side,
            Some("Keep this inline side question".into()),
        ))
        .await?;
        assert_eq!(
            (
                app.current_displayed_thread_id(),
                app.side_threads
                    .get(&second_id)
                    .map(|state| (state.parent_thread_id, state.kind)),
                app.side_threads.len(),
                store.pair(parent_id)?,
                store.side(second_id)?,
                store.side(first_id)?,
                app.chat_widget.composer_text_with_pending(),
            ),
            (
                Some(thread_id),
                Some((parent_id, CompanionKind::Parallel)),
                1,
                parent_selection.clone(),
                Some(open_pair.clone()),
                first_record.clone(),
                "Keep this inline side question".to_string(),
            ),
        );
        app.chat_widget.apply_external_edit(String::new());
    }
    Box::pin(app.select_agent_thread(&mut tui, &mut server, second_id)).await?;
    assert!(Box::pin(app.maybe_return_from_side(&mut tui, &mut server)).await);
    assert_eq!(app.current_displayed_thread_id(), Some(parent_id));
    assert!(app.side_threads.is_empty());
    assert!(app.abandoned_side_threads.contains(&second_id));
    assert_eq!(store.pair(parent_id)?, None);
    assert_eq!(store.side(second_id)?, Some(open_pair));

    // Closed saved chats remain on disk but no longer block a temporary side.
    Box::pin(app.start_companion_conversation(
        &mut tui,
        &mut server,
        parent_id,
        CompanionKind::Side,
        crate::app_event::SideConversationMode::Side,
        /*user_message*/ None,
    ))
    .await?;
    let temporary_id = app.current_displayed_thread_id().expect("temporary side");
    let temporary = server
        .thread_read(temporary_id, /*include_turns*/ false)
        .await?;
    assert!(temporary.ephemeral);
    assert!(app.chat_widget.side_conversation_active());
    assert!(!app.chat_widget.parallel_conversation_active());
    Box::pin(app.start_companion_conversation(
        &mut tui,
        &mut server,
        temporary_id,
        CompanionKind::Parallel,
        crate::app_event::SideConversationMode::Side,
        /*user_message*/ None,
    ))
    .await?;
    assert_eq!(app.current_displayed_thread_id(), Some(temporary_id));
    assert_eq!(app.active_side_parent_thread_id(), Some(parent_id));
    assert_eq!(store.side(temporary_id)?, None);
    assert_eq!(store.pair(parent_id)?, None);
    assert!(store.side(second_id)?.is_some());
    assert!(
        !server
            .thread_read(second_id, /*include_turns*/ false)
            .await?
            .ephemeral
    );
    assert_eq!(app.chat_widget.last_side_reply_markdown(), None);
    Box::pin(app.apply_side_conversation_action(
        &mut tui,
        &mut server,
        SideConversationAction::SendLast {
            text: String::new(),
        },
    ))
    .await?;
    assert!(recorded_params(&requests, "turn/start").is_empty());
    app.chat_widget.handle_server_notification(
        ServerNotification::ItemCompleted(codex_app_server_protocol::ItemCompletedNotification {
            thread_id: temporary_id.to_string(),
            turn_id: "side-answer".to_string(),
            completed_at_ms: 0,
            item: ThreadItem::AgentMessage {
                id: "answer".to_string(),
                text: "Completed side answer".to_string(),
                phase: None,
                memory_citation: None,
                delivery: None,
                questions: None,
            },
        }),
        /*replay_kind*/ None,
    );
    assert_eq!(
        app.chat_widget.last_side_reply_markdown(),
        Some("Completed side answer")
    );
    for busy in [false, true] {
        app.ensure_thread_channel(parent_id)
            .store
            .lock()
            .await
            .active_turn_id = busy.then(|| "main-busy".to_string());
        Box::pin(app.apply_side_conversation_action(
            &mut tui,
            &mut server,
            SideConversationAction::SendLast {
                text: if busy { "Use this answer" } else { "" }.to_string(),
            },
        ))
        .await?;
        let forwarded = recorded_params(&requests, "turn/start")
            .pop()
            .expect("forwarded parent turn");
        assert_eq!(forwarded["threadId"], parent_id.to_string());
        assert_eq!(
            forwarded["input"],
            serde_json::json!([{ "type": "text", "text": if busy { "Completed side answer\n\nUse this answer" } else { "Completed side answer" }, "text_elements": [] }])
        );
        for key in [
            "model",
            "effort",
            "serviceTier",
            "collaborationMode",
            "permissions",
            "approvalPolicy",
            "sandboxPolicy",
        ] {
            assert!(
                forwarded[key].is_null(),
                "recipient setting overwritten: {key}"
            );
        }
        assert_eq!(app.current_displayed_thread_id(), Some(temporary_id));
    }
    app.ensure_thread_channel(parent_id)
        .store
        .lock()
        .await
        .active_turn_id = None;
    assert_eq!(recorded_params(&requests, "turn/start").len(), 2);
    Box::pin(app.apply_side_conversation_action(
        &mut tui,
        &mut server,
        SideConversationAction::SendLast {
            text: "x".repeat(/*n*/ 10_000),
        },
    ))
    .await?;
    assert_eq!(recorded_params(&requests, "turn/start").len(), 2);
    assert_eq!(app.current_displayed_thread_id(), Some(temporary_id));
    server
        .thread_set_name(parent_id, "Main lifecycle".to_string())
        .await?;
    server.thread_inject_items(temporary_id, vec![serde_json::from_value(serde_json::json!({
        "type": "message", "role": "assistant", "content": [{ "type": "output_text", "text": "Side-only marker" }]
    }))?]).await?;
    for name in [None, Some("Explicit saved chat".to_string())] {
        let expected_name = name.clone().unwrap_or("Main lifecycle (fork)".to_string());
        Box::pin(app.apply_side_conversation_action(
            &mut tui,
            &mut server,
            SideConversationAction::Fork { name },
        ))
        .await?;
        let request = recorded_params(&requests, "thread/name/set")
            .pop()
            .expect("fork naming request");
        assert_eq!(request["name"], expected_name);
        let saved_id =
            ThreadId::from_string(request["threadId"].as_str().expect("saved thread ID"))?;
        let saved = server
            .thread_read(saved_id, /*include_turns*/ false)
            .await?;
        assert!(!saved.ephemeral);
        let saved_history = std::fs::read_to_string(saved.path.expect("persistent fork rollout"))?;
        assert!(saved_history.contains("Parent history must remain saved"));
        assert!(saved_history.contains("Side-only marker"));
        assert!(saved_history.contains(
            "Earlier temporary Side/Chat role and capability restrictions no longer apply"
        ));
        assert_eq!(
            (
                app.current_displayed_thread_id(),
                app.active_side_parent_thread_id(),
                app.side_threads.len()
            ),
            (Some(temporary_id), Some(parent_id), 1)
        );
    }
    server.thread_inject_items(parent_id, vec![serde_json::from_value(serde_json::json!({
        "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "Latest main marker" }]
    }))?]).await?;
    let settings = (
        app.chat_widget.current_model().to_string(),
        app.chat_widget.current_reasoning_effort(),
        app.chat_widget.configured_service_tier(),
    );
    Box::pin(app.apply_side_conversation_action(
        &mut tui,
        &mut server,
        SideConversationAction::Sync,
    ))
    .await?;
    let previous_temporary_id = temporary_id;
    let temporary_id = app.current_displayed_thread_id().expect("synced Side");
    assert_ne!(temporary_id, previous_temporary_id);
    assert_eq!(app.active_side_parent_thread_id(), Some(parent_id));
    assert_eq!(app.side_threads.len(), 1);
    assert_eq!(app.chat_widget.last_side_reply_markdown(), None);
    assert_eq!(
        (
            app.chat_widget.current_model().to_string(),
            app.chat_widget.current_reasoning_effort(),
            app.chat_widget.configured_service_tier()
        ),
        settings
    );
    Box::pin(app.apply_side_conversation_action(
        &mut tui,
        &mut server,
        SideConversationAction::Fork {
            name: Some("After sync".to_string()),
        },
    ))
    .await?;
    let request = recorded_params(&requests, "thread/name/set")
        .pop()
        .expect("synced fork naming request");
    let saved_id = ThreadId::from_string(request["threadId"].as_str().expect("saved thread ID"))?;
    let saved = server
        .thread_read(saved_id, /*include_turns*/ false)
        .await?;
    let saved_history = std::fs::read_to_string(saved.path.expect("synced fork rollout"))?;
    assert!(saved_history.contains("Latest main marker"));
    assert!(!saved_history.contains("Side-only marker"));
    Box::pin(app.toggle_side_conversation(&mut tui, &mut server)).await?;
    assert_eq!(app.current_displayed_thread_id(), Some(parent_id));
    Box::pin(app.toggle_side_conversation(&mut tui, &mut server)).await?;
    assert_eq!(app.current_displayed_thread_id(), Some(temporary_id));
    assert!(Box::pin(app.maybe_return_from_side(&mut tui, &mut server)).await);
    assert_eq!(app.current_displayed_thread_id(), Some(parent_id));
    assert!(app.side_threads.is_empty());

    let models = Box::pin(server.bootstrap(&app.config))
        .await?
        .available_models;
    let fast_model = models
        .iter()
        .find(|model| model.service_tiers.iter().any(|tier| tier.id == "priority"))
        .expect("a Fast-supported model")
        .model
        .clone();
    app.chat_widget.set_model(&fast_model);
    app.chat_widget
        .set_service_tier(Some("default".to_string()));
    app.config.service_tier = Some("default".to_string());
    app.local_settings.tui.status_line = Some(vec!["fast-mode".to_string()]);
    app.cli_kv_overrides.extend([
        (
            "service_tier".to_string(),
            TomlValue::String("default".to_string()),
        ),
        (
            "tui.status_line".to_string(),
            TomlValue::Array(vec![TomlValue::String("fast-mode".to_string())]),
        ),
    ]);
    app.model_catalog = Arc::new(crate::model_catalog::ModelCatalog::new(models));
    Box::pin(app.start_companion_conversation(
        &mut tui,
        &mut server,
        parent_id,
        CompanionKind::Side,
        crate::app_event::SideConversationMode::Chat,
        /*user_message*/ None,
    ))
    .await?;
    app.keymap.app.toggle_fast_mode = vec![crate::key_hint::KeyBinding::new(
        KeyCode::Char('f'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    )];
    let mut saved_chat_target = None;
    for phase in ["created", "switched", "synced"] {
        let chat_id = app.current_displayed_thread_id().expect("active Chat");
        assert_ne!(chat_id, parent_id);
        assert_eq!(
            (
                app.chat_widget.configured_service_tier().as_deref(),
                app.chat_widget.current_service_tier(),
                app.config.service_tier.as_deref(),
            ),
            (Some("priority"), Some("priority"), Some("default")),
        );
        // Model refresh must not recompute Chat's tier from the parent's config.
        app.chat_widget.set_model(&fast_model);
        assert_eq!(app.chat_widget.current_service_tier(), Some("priority"));
        assert!(render_bottom_popup(&app.chat_widget, /*width*/ 120).contains("F:on"));
        assert!(app.chat_widget.can_toggle_fast_mode_from_keybinding());
        Box::pin(app.handle_shared_app_keymap_action(
            &mut tui,
            &mut server,
            KeyEvent::new(
                KeyCode::Char('f'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
        ))
        .await;
        assert_eq!(app.chat_widget.current_service_tier(), Some("priority"));
        while app_events.try_recv().is_ok() {}
        let _ = app
            .chat_widget
            .submit_user_message_as_plain_user_turn(crate::chatwidget::UserMessage::from(phase));
        while let Ok(event) = app_events.try_recv() {
            Box::pin(app.handle_event(&mut tui, &mut server, event)).await?;
        }
        let request = recorded_params(&requests, "turn/start")
            .pop()
            .expect("Chat turn request");
        assert_eq!(request["threadId"], chat_id.to_string());
        assert_eq!(request["input"][0]["text"], phase);
        assert_eq!(request["serviceTier"], "priority");
        Box::pin(app.enqueue_thread_notification(
            chat_id,
            turn_completed_notification(chat_id, "forwarded-turn", TurnStatus::Completed),
        ))
        .await?;
        match phase {
            "created" => {
                Box::pin(app.toggle_side_conversation(&mut tui, &mut server)).await?;
                assert_eq!(app.current_displayed_thread_id(), Some(parent_id));
                assert_eq!(
                    app.chat_widget.configured_service_tier().as_deref(),
                    Some("default")
                );
                Box::pin(app.toggle_side_conversation(&mut tui, &mut server)).await?;
            }
            "switched" => {
                Box::pin(app.apply_side_conversation_action(
                    &mut tui,
                    &mut server,
                    SideConversationAction::Sync,
                ))
                .await?;
                assert_ne!(app.current_displayed_thread_id(), Some(chat_id));
            }
            "synced" => {
                Box::pin(app.apply_side_conversation_action(
                    &mut tui,
                    &mut server,
                    SideConversationAction::Fork {
                        name: Some("Saved Fast Chat".to_string()),
                    },
                ))
                .await?;
                let request = recorded_params(&requests, "thread/name/set")
                    .pop()
                    .expect("saved Chat naming request");
                let saved_id = ThreadId::from_string(request["threadId"].as_str().unwrap())?;
                let saved = server
                    .thread_read(saved_id, /*include_turns*/ false)
                    .await?;
                saved_chat_target = Some(crate::resume_picker::SessionTarget {
                    path: saved.path,
                    thread_id: saved_id,
                    cwd: None,
                    history_mode: None,
                });
            }
            _ => unreachable!("known lifecycle phase"),
        }
    }
    assert!(Box::pin(app.maybe_return_from_side(&mut tui, &mut server)).await);

    for selected_view in ["child", "parent"] {
        Box::pin(app.select_agent_thread(&mut tui, &mut server, parent_id)).await?;
        Box::pin(app.start_companion_conversation(
            &mut tui,
            &mut server,
            parent_id,
            CompanionKind::Parallel,
            crate::app_event::SideConversationMode::Side,
            /*user_message*/ None,
        ))
        .await?;
        let active_child = app
            .current_displayed_thread_id()
            .expect("new parallel child");
        let selected = if selected_view == "child" {
            active_child
        } else {
            parent_id
        };
        Box::pin(app.select_agent_thread(&mut tui, &mut server, selected)).await?;
        let path = server
            .thread_read(selected, /*include_turns*/ false)
            .await?
            .path;
        Box::pin(app.resume_target_session(
            &mut tui,
            &mut server,
            crate::resume_picker::SessionTarget {
                path,
                thread_id: selected,
                cwd: None,
                history_mode: None,
            },
        ))
        .await?;
        assert_eq!(
            (
                app.current_displayed_thread_id(),
                app.primary_thread_id,
                app.side_threads.is_empty(),
                app.chat_widget.parallel_conversation_active(),
                app.agent_navigation.tracked_thread_ids(),
            ),
            (Some(selected), Some(selected), true, false, vec![selected]),
        );
        while app_events.try_recv().is_ok() {}
        if let Some(receiver) = app.active_thread_rx.as_mut() {
            while receiver.try_recv().is_ok() {}
        }
        app.enqueue_thread_notification(
            selected,
            agent_message_delta_notification(
                selected,
                "same-resume-turn",
                "same-resume-item",
                "after same-session resume",
            ),
        )
        .await?;
        assert!(
            app.active_thread_rx
                .as_mut()
                .expect("same-session receiver")
                .try_recv()
                .is_ok()
        );
        // A successful unsubscribe proves navigation retained the just-established subscription.
        let request_id = server.next_request_id();
        let subscription = server
            .request_handle()
            .request_typed::<codex_app_server_protocol::ThreadUnsubscribeResponse>(
                codex_app_server_protocol::ClientRequest::ThreadUnsubscribe {
                    request_id,
                    params: codex_app_server_protocol::ThreadUnsubscribeParams {
                        thread_id: selected.to_string(),
                    },
                },
            )
            .await?;
        assert_eq!(
            subscription.status,
            codex_app_server_protocol::ThreadUnsubscribeStatus::Unsubscribed
        );
    }

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
    let parent = server
        .resume_thread(
            &app.local_settings,
            app.config.clone(),
            parent_id,
            crate::app_server_session::ResumeModelSettings::PreserveExistingThread,
        )
        .await?;
    Box::pin(app.replace_chat_widget_with_app_server_thread(
        &mut tui,
        &server,
        parent,
        ThreadAttachPresentation::SessionLineage,
        /*initial_user_message*/ None,
    ))
    .await?;
    assert_eq!(app.current_displayed_thread_id(), Some(parent_id));
    assert!(app.side_threads.is_empty());

    // A deleted leaf must not leave the parent toggling to an unavailable saved child.
    let deleted_pair = store.side(first_id)?.expect("saved first child record");
    store.save(&deleted_pair)?;
    app.side_threads
        .insert(first_id, SideThreadState::parallel(parent_id));
    server.thread_delete(first_id).await?;
    Box::pin(app.toggle_side_conversation(&mut tui, &mut server)).await?;
    assert_eq!(
        (
            app.current_displayed_thread_id(),
            app.primary_thread_id,
            app.side_threads.is_empty(),
            store.pair(parent_id)?,
            store.side(first_id)?,
        ),
        (
            Some(parent_id),
            Some(parent_id),
            true,
            None,
            Some(deleted_pair)
        ),
    );
    Box::pin(app.toggle_side_conversation(&mut tui, &mut server)).await?;
    assert_eq!(app.current_displayed_thread_id(), Some(parent_id));

    // Legacy forks copy history, so deleting their parent does not break that saved history.
    let orphan_pair = store.side(second_id)?.expect("saved second child record");
    let orphan_path = server
        .thread_read(second_id, /*include_turns*/ false)
        .await?
        .path
        .expect("saved child rollout");
    let orphan_history = std::fs::read_to_string(&orphan_path)?;
    assert!(orphan_history.contains("Parent history must remain saved"));
    store.save(&orphan_pair)?;
    server.thread_delete(parent_id).await?;
    assert_eq!(std::fs::read_to_string(&orphan_path)?, orphan_history);
    let orphan = server
        .resume_thread(
            &app.local_settings,
            app.config.clone(),
            second_id,
            crate::app_server_session::ResumeModelSettings::PreserveExistingThread,
        )
        .await?;
    Box::pin(app.replace_chat_widget_with_app_server_thread(
        &mut tui,
        &server,
        orphan,
        ThreadAttachPresentation::SessionLineage,
        /*initial_user_message*/ None,
    ))
    .await?;
    assert_eq!(
        (
            app.active_side_parent_thread_id(),
            app.primary_thread_id,
            app.side_threads.is_empty(),
            app.chat_widget.parallel_conversation_active(),
        ),
        (None, Some(second_id), true, false),
    );
    // Missing-parent recovery still applies to pairs opened in the current runtime.
    app.side_threads
        .insert(second_id, SideThreadState::parallel(parent_id));
    app.primary_thread_id = Some(parent_id);
    app.primary_session_configured = None;
    app.sync_side_thread_ui();
    for _ in 0..2 {
        assert!(!Box::pin(app.maybe_return_from_side(&mut tui, &mut server)).await);
        assert_eq!(
            (
                app.current_displayed_thread_id(),
                app.primary_thread_id,
                app.primary_session_configured
                    .as_ref()
                    .map(|session| session.thread_id),
                app.side_threads.is_empty(),
                app.chat_widget.parallel_conversation_active(),
                app.chat_widget.side_conversation_active(),
            ),
            (
                Some(second_id),
                Some(second_id),
                Some(second_id),
                true,
                false,
                false
            ),
        );
    }
    assert_eq!(store.pair(parent_id)?, None);
    assert_eq!(store.side(second_id)?, Some(orphan_pair));
    assert!(std::fs::read_to_string(&orphan_path)?.starts_with(&orphan_history));
    // A global default must not replace a saved Chat's tier on ordinary /resume.
    app.cli_kv_overrides
        .retain(|(key, _)| key != "service_tier");
    crate::legacy_core::config::edit::ConfigEditsBuilder::for_config(&app.config)
        .set_service_tier(Some("default".to_string()))
        .apply()
        .await
        .map_err(std::io::Error::other)?;
    let target = saved_chat_target.expect("saved Fast Chat");
    let saved_chat_id = target.thread_id;
    Box::pin(app.resume_target_session(&mut tui, &mut server, target)).await?;
    for phase in ["resumed", "reattached"] {
        assert_eq!(
            (
                app.current_displayed_thread_id(),
                app.chat_widget.configured_service_tier().as_deref(),
                app.config.service_tier.as_deref(),
                app.chat_widget.side_conversation_active(),
            ),
            (
                Some(saved_chat_id),
                Some("priority"),
                Some("default"),
                false
            ),
        );
        while app_events.try_recv().is_ok() {}
        let _ = app
            .chat_widget
            .submit_user_message_as_plain_user_turn(crate::chatwidget::UserMessage::from(phase));
        while let Ok(event) = app_events.try_recv() {
            Box::pin(app.handle_event(&mut tui, &mut server, event)).await?;
        }
        let request = recorded_params(&requests, "turn/start")
            .pop()
            .expect("saved Chat turn request");
        assert_eq!(request["threadId"], saved_chat_id.to_string());
        assert_eq!(request["input"][0]["text"], phase);
        assert_eq!(request["serviceTier"], "priority");
        Box::pin(app.enqueue_thread_notification(
            saved_chat_id,
            turn_completed_notification(saved_chat_id, "forwarded-turn", TurnStatus::Completed),
        ))
        .await?;
        if phase == "resumed" {
            Box::pin(app.select_agent_thread(&mut tui, &mut server, second_id)).await?;
            Box::pin(app.select_agent_thread(&mut tui, &mut server, saved_chat_id)).await?;
        }
    }
    // Parallel's selected tier must survive changes to main's persisted default.
    Box::pin(app.start_companion_conversation(
        &mut tui,
        &mut server,
        saved_chat_id,
        CompanionKind::Parallel,
        crate::app_event::SideConversationMode::Side,
        /*user_message*/ None,
    ))
    .await?;
    let parallel_id = app
        .current_displayed_thread_id()
        .expect("tier probe Parallel");
    assert_ne!(parallel_id, saved_chat_id);
    app.chat_widget
        .set_service_tier(Some("priority".to_string()));
    Box::pin(app.handle_event(
        &mut tui,
        &mut server,
        AppEvent::PersistServiceTierSelection {
            service_tier: Some("priority".to_string()),
        },
    ))
    .await?;
    assert_eq!(app.chat_widget.current_service_tier(), Some("priority"));
    Box::pin(app.toggle_side_conversation(&mut tui, &mut server)).await?;
    assert_eq!(app.current_displayed_thread_id(), Some(saved_chat_id));
    app.chat_widget
        .set_service_tier(Some("default".to_string()));
    Box::pin(app.handle_event(
        &mut tui,
        &mut server,
        AppEvent::PersistServiceTierSelection {
            service_tier: Some("default".to_string()),
        },
    ))
    .await?;
    Box::pin(app.toggle_side_conversation(&mut tui, &mut server)).await?;
    assert_eq!(app.current_displayed_thread_id(), Some(parallel_id));
    let configured_tier = app.chat_widget.configured_service_tier();
    let displayed_tier = app.chat_widget.current_service_tier().map(str::to_string);
    while app_events.try_recv().is_ok() {}
    let _ = app.chat_widget.submit_user_message_as_plain_user_turn(
        crate::chatwidget::UserMessage::from("tier probe after main default change"),
    );
    while let Ok(event) = app_events.try_recv() {
        Box::pin(app.handle_event(&mut tui, &mut server, event)).await?;
    }
    let request = recorded_params(&requests, "turn/start")
        .pop()
        .expect("tier probe Parallel turn/start");
    assert_eq!(request["threadId"], parallel_id.to_string());
    assert_eq!(
        (
            configured_tier,
            displayed_tier,
            request["serviceTier"].clone()
        ),
        (
            Some("priority".to_string()),
            Some("priority".to_string()),
            serde_json::json!("priority"),
        ),
    );
    server.shutdown().await?;
    proxy.await??;
    Ok(())
}
