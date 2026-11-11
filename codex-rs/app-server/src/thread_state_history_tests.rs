use super::*;
use codex_app_server_protocol::ThreadItemPayload;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::items::UserMessageItem;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::protocol::TurnAbortedEvent;
use codex_protocol::protocol::TurnCompleteEvent;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::protocol::UserMessageEvent;
use codex_protocol::user_input::UserInput;
use pretty_assertions::assert_eq;

#[test]
fn aborted_tracker_discards_old_payloads_without_changing_current_snapshot() {
    for mode in [ThreadHistoryMode::Legacy, ThreadHistoryMode::Paginated] {
        let mut state = ThreadState {
            history_mode: mode,
            evict_superseded_agent_text: mode == ThreadHistoryMode::Paginated,
            ..Default::default()
        };
        let mut canonical = ThreadHistoryBuilder::new();
        for index in 0..4 {
            let turn_id = format!("turn-{index}");
            let mut events = vec![
                EventMsg::TurnStarted(TurnStartedEvent {
                    turn_attribution: None,
                    turn_id: turn_id.clone(),
                    root_turn_id: None,
                    trace_id: None,
                    started_at: Some(index),
                    model_context_window: None,
                    collaboration_mode_kind: Default::default(),
                }),
                EventMsg::UserMessage(UserMessageEvent {
                    message: "x".repeat(256 * 1024),
                    ..Default::default()
                }),
                EventMsg::TurnAborted(TurnAbortedEvent {
                    root_turn_id: None,
                    turn_id: Some(turn_id.clone()),
                    reason: TurnAbortReason::Interrupted,
                    error: None,
                    started_at: None,
                    completed_at: Some(index + 1),
                    duration_ms: None,
                }),
            ];
            if mode == ThreadHistoryMode::Paginated {
                events[1] = EventMsg::ItemCompleted(ItemCompletedEvent {
                    content_token_count: None,
                    thread_id: Default::default(),
                    turn_id: turn_id.clone(),
                    item: CoreTurnItem::UserMessage(UserMessageItem {
                        id: format!("user-{index}"),
                        client_id: None,
                        content: vec![UserInput::Text {
                            text: "x".repeat(256 * 1024),
                            text_elements: Vec::new(),
                        }],
                    }),
                    started_at_ms: None,
                    completed_at_ms: index,
                });
                for message in 0..2 {
                    events.insert(
                        events.len() - 1,
                        EventMsg::ItemCompleted(ItemCompletedEvent {
                            content_token_count: None,
                            thread_id: Default::default(),
                            turn_id: turn_id.clone(),
                            item: CoreTurnItem::AgentMessage(AgentMessageItem {
                                id: format!("agent-{index}-{message}"),
                                content: vec![AgentMessageContent::Text {
                                    text: "assistant text".into(),
                                }],
                                moderation: None,
                                phase: None,
                                memory_citation: None,
                                delivery: None,
                                questions: None,
                            }),
                            started_at_ms: None,
                            completed_at_ms: index,
                        }),
                    );
                }
            }
            for event in &events[..events.len() - 1] {
                if mode == ThreadHistoryMode::Paginated {
                    canonical.handle_live_paginated_event(event);
                } else {
                    canonical.handle_event_for_history_mode(event, mode);
                }
                state.track_current_turn_event(&turn_id, event);
                assert_eq!(
                    state.active_turn_snapshot(),
                    canonical.active_turn_snapshot()
                );
            }
            if index > 0 {
                let before = state.active_turn_snapshot();
                let old_id = format!("turn-{}", index - 1);
                for id in [Some(old_id.clone()), None] {
                    state.track_current_turn_event(
                        &old_id,
                        &EventMsg::TurnAborted(TurnAbortedEvent {
                            root_turn_id: None,
                            turn_id: id,
                            reason: TurnAbortReason::Interrupted,
                            error: None,
                            started_at: None,
                            completed_at: Some(100),
                            duration_ms: None,
                        }),
                    );
                    assert_eq!(state.active_turn_snapshot(), before);
                }
                state.track_current_turn_event(
                    &old_id,
                    &EventMsg::TurnComplete(TurnCompleteEvent {
                        root_turn_id: None,
                        turn_id: old_id.clone(),
                        started_at: None,
                        last_agent_message: None,
                        error: None,
                        completed_at: Some(100),
                        duration_ms: None,
                        time_to_first_token_ms: None,
                    }),
                );
                assert_eq!(state.active_turn_snapshot(), before);
                assert_eq!(
                    state.last_terminal_turn_id.as_deref(),
                    Some(old_id.as_str())
                );
            }
            let aborted = events.last().unwrap();
            canonical.handle_event_for_history_mode(aborted, mode);
            state.track_current_turn_event(&turn_id, aborted);
            assert_eq!(
                state.active_turn_snapshot(),
                canonical.active_turn_snapshot()
            );
        }
        let retained = state.current_turn_history.finish();
        let original = canonical.finish();
        assert_eq!(retained, original[3..]);
        assert_eq!(original.len(), 4);
        assert_eq!(
            retained[0].items.len(),
            if mode == ThreadHistoryMode::Paginated {
                3
            } else {
                1
            }
        );
        if mode == ThreadHistoryMode::Paginated {
            assert!(
                matches!(&retained[0].items[1].payload, ThreadItemPayload::AgentMessage { text, .. } if text.is_empty())
            );
            assert!(
                matches!(&retained[0].items[2].payload, ThreadItemPayload::AgentMessage { text, .. } if text == "assistant text")
            );
        }
    }
}
