//! Exercises completion metadata through live notifications and restored turn history.

use super::*;
use crate::clock_format::ClockFormat;
use chrono::Local;
use chrono::TimeZone;
use pretty_assertions::assert_eq;

const COMPLETED_AT: i64 = 1_700_000_000;
const SESSION_ID: &str = "019abc12-3456-7890-abcd-1234567890ab";
const TURN_USAGE: codex_app_server_protocol::TokenUsageBreakdown =
    codex_app_server_protocol::TokenUsageBreakdown {
        input_tokens: 12_400,
        cached_input_tokens: 9_800,
        cache_write_input_tokens: 0,
        output_tokens: 820,
        reasoning_output_tokens: 600,
        total_tokens: 13_220,
    };

fn completed_turn(duration_ms: Option<i64>, completed_at: Option<i64>) -> AppServerTurn {
    AppServerTurn {
        completed_at,
        items: vec![AppServerThreadItem::AgentMessage {
            id: "answer-1".to_string(),
            text: "The change is ready.".to_string(),
            phase: Some(MessagePhase::FinalAnswer),
            memory_citation: None,
            delivery: None,
            questions: None,
        }],
        ..app_server_turn(
            "turn-1",
            AppServerTurnStatus::Completed,
            duration_ms,
            /*error*/ None,
        )
    }
}

fn complete_turn(chat: &mut ChatWidget, turn: AppServerTurn) {
    chat.handle_server_notification(
        ServerNotification::TurnCompleted(TurnCompletedNotification {
            thread_id: chat.thread_id.map(|id| id.to_string()).unwrap_or_default(),
            turn,
        }),
        /*replay_kind*/ None,
    );
}

fn completion_labels(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> String {
    std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|event| match event {
            AppEvent::InsertHistoryCell(cell)
                if cell.as_any().is::<history_cell::FinalMessageSeparator>() =>
            {
                let label = lines_to_single_string(&cell.raw_lines()).trim().to_string();
                (!label.is_empty()).then_some(label)
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn saved_completion_label() -> String {
    Local
        .timestamp_opt(COMPLETED_AT, /*nsecs*/ 0)
        .unwrap()
        .format(&format!(
            "%b %-d, %Y at {}",
            ClockFormat::system().time_format()
        ))
        .to_string()
}

#[tokio::test]
async fn completion_follows_plain_and_streamed_tool_answers() {
    for streamed in [false, true] {
        let (mut chat, mut rx, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
        handle_turn_started(&mut chat, "turn-1");
        if streamed {
            let command = begin_exec(&mut chat, "command-1", "echo ready");
            end_exec(&mut chat, command, "ready", "", /*exit_code*/ 0);
            handle_agent_message_delta(&mut chat, "The change is ready.\n");
            chat.run_commit_tick();
        }
        chat.thread_id = Some(ThreadId::from_string(SESSION_ID).unwrap());
        let mut turn = completed_turn(Some(125_000), Some(COMPLETED_AT));
        turn.token_usage = Some(TURN_USAGE);
        complete_turn(&mut chat, turn);

        let text = drain_insert_history_normalized(&mut rx)
            .iter()
            .map(|lines| lines_to_single_string(lines))
            .collect::<String>();
        assert_chatwidget_snapshot!(
            if streamed {
                "completion_after_streamed_tool_answer"
            } else {
                "completion_after_plain_answer"
            },
            text,
        );
    }
}

#[tokio::test]
async fn completion_live_shows_known_durations_and_preserves_timestamp_fallback() {
    for (duration_ms, completed_at, prefix) in [
        (598, Some(COMPLETED_AT), "Worked for <1s • "),
        (1_000, Some(COMPLETED_AT), "Worked for 1s • "),
        (60_000, Some(COMPLETED_AT), "Worked for 1m 0s • "),
        (60_999, Some(COMPLETED_AT), "Worked for 1m 0s • "),
        (61_000, Some(COMPLETED_AT), "Worked for 1m 1s • "),
        (125_000, Some(COMPLETED_AT), "Worked for 2m 5s • "),
        (1_000, None, "Worked for 1s • "),
    ] {
        let (mut chat, mut rx, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
        chat.thread_id = Some(ThreadId::from_string(SESSION_ID).unwrap());
        handle_turn_started(&mut chat, "turn-1");
        let turn = completed_turn(Some(duration_ms), completed_at);
        let before = Local::now();
        complete_turn(&mut chat, turn.clone());
        let after = Local::now();
        let possible = [before, after].map(|time| {
            let time = if completed_at.is_some() {
                saved_completion_label()
            } else {
                time.format(ClockFormat::system().time_format()).to_string()
            };
            format!("{prefix}{time} • {SESSION_ID}")
        });
        let label = completion_labels(&mut rx);
        assert!(possible.contains(&label), "{label:?}");
        complete_turn(&mut chat, turn);
        assert_eq!(completion_labels(&mut rx), "");
    }
}

#[tokio::test]
async fn completion_replay_preserves_metadata_and_input_without_live_side_effects() {
    let done = saved_completion_label();
    for (duration_ms, completed_at, expected) in [
        (
            Some(125_000),
            Some(COMPLETED_AT),
            format!("Worked for 2m 5s • {done}"),
        ),
        (None, None, String::new()),
        (None, Some(COMPLETED_AT), done.clone()),
        (
            Some(598),
            Some(COMPLETED_AT),
            format!("Worked for <1s • {done}"),
        ),
        (
            Some(60_000),
            Some(COMPLETED_AT),
            format!("Worked for 1m 0s • {done}"),
        ),
        (
            Some(61_000),
            Some(COMPLETED_AT),
            format!("Worked for 1m 1s • {done}"),
        ),
        (Some(60_000), None, "Worked for 1m 0s".to_string()),
        (Some(125_000), None, "Worked for 2m 5s".to_string()),
    ] {
        for replay_kind in [
            ReplayKind::ResumeInitialMessages,
            ReplayKind::ThreadSnapshot,
        ] {
            let (mut chat, mut rx, mut op_rx) =
                make_chatwidget_manual(/*model_override*/ None).await;
            chat.thread_id = Some(ThreadId::from_string(SESSION_ID).unwrap());
            handle_turn_started(&mut chat, "turn-1");
            if matches!(replay_kind, ReplayKind::ThreadSnapshot) {
                chat.queue_user_message("Continue".into());
            }
            let queued_input = chat.input_queue.queued_user_messages.clone();
            while op_rx.try_recv().is_ok() {}
            let mut turn = completed_turn(duration_ms, completed_at);
            let mut expected_label = expected.clone();
            if duration_ms == Some(125_000) {
                turn.token_usage = Some(TURN_USAGE);
                expected_label.push_str(" • 12.4k in • 9.8k cc • 820|600 ou");
            }
            if !expected_label.is_empty() {
                expected_label.push_str(&format!(" • {SESSION_ID}"));
            }
            chat.replay_thread_turns(vec![turn.clone()], replay_kind);
            assert_eq!(completion_labels(&mut rx), expected_label);
            assert_matches!(chat.pending_notification, None);
            assert_eq!(chat.input_queue.queued_user_messages, queued_input);
            assert_matches!(op_rx.try_recv(), Err(TryRecvError::Empty));

            // An empty replay label must not suppress later authoritative live metadata.
            chat.input_queue.queued_user_messages.clear();
            turn.completed_at = Some(COMPLETED_AT);
            complete_turn(&mut chat, turn.clone());
            assert_eq!(
                completion_labels(&mut rx),
                if expected.is_empty() {
                    format!("{done} • {SESSION_ID}")
                } else {
                    String::new()
                }
            );
            complete_turn(&mut chat, turn);
            assert_eq!(completion_labels(&mut rx), "");
        }
    }
}

#[tokio::test]
async fn completion_replay_waits_for_older_turn_items_to_load() {
    let (mut chat, mut rx, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    let older_completed_at = COMPLETED_AT - 86_400;
    let mut older = completed_turn(Some(125_000), Some(older_completed_at));
    older.id = "older-turn".to_string();
    let mut unloaded = older.clone();
    unloaded.items_view = codex_app_server_protocol::TurnItemsView::NotLoaded;
    unloaded.items.clear();
    let newer = completed_turn(Some(1_000), Some(COMPLETED_AT));

    chat.replay_thread_turns(vec![unloaded, newer], ReplayKind::ResumeInitialMessages);
    assert_eq!(
        completion_labels(&mut rx),
        format!("Worked for 1s • {}", saved_completion_label())
    );

    chat.replay_thread_turns(vec![older], ReplayKind::ThreadSnapshot);
    let older_time = Local
        .timestamp_opt(older_completed_at, /*nsecs*/ 0)
        .unwrap()
        .format(&format!(
            "%b %-d, %Y at {}",
            ClockFormat::system().time_format()
        ))
        .to_string();
    assert_eq!(
        completion_labels(&mut rx),
        format!("Worked for 2m 5s • {older_time}"),
    );
}

#[tokio::test]
async fn completion_failed_and_interrupted_turns_do_not_report_success() {
    for status in [
        AppServerTurnStatus::Failed,
        AppServerTurnStatus::Interrupted,
    ] {
        for replay_kind in [None, Some(ReplayKind::ResumeInitialMessages)] {
            let (mut chat, mut rx, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
            let mut turn = completed_turn(Some(125_000), Some(COMPLETED_AT));
            turn.status = status.clone();
            turn.error = (status == AppServerTurnStatus::Failed).then(|| AppServerTurnError {
                misalignment: None,
                message: "The task failed.".to_string(),
                codex_error_info: None,
                additional_details: None,
            });
            if let Some(replay_kind) = replay_kind {
                chat.replay_thread_turns(vec![turn], replay_kind);
            } else {
                handle_turn_started(&mut chat, "turn-1");
                complete_turn(&mut chat, turn);
            }
            assert_eq!(completion_labels(&mut rx), "");
            assert!(!chat.bottom_pane.is_task_running());
        }
    }
}

#[test]
fn completion_snapshot_normalization_preserves_clock_only_message_lines() {
    let timestamp = "Sep 6, 2000 at 2:32 PM";
    let transcript = format!(
        "› {timestamp}\n• {timestamp}\n  └ {timestamp}\n  The job was done {timestamp}\n  {timestamp} is the expected text\n  Worked for 2m 5s · {timestamp} is quoted\n\n  3:24 PM\n  Worked for 1h 2m 3s · {timestamp}\n"
    );
    let message = history_cell::PlainHistoryCell::new(
        transcript
            .lines()
            .map(|line| Line::from(line.to_owned()))
            .collect(),
    );
    assert_chatwidget_snapshot!("completion_like_transcript_text", &transcript);
    assert_eq!(
        normalize_completion_timestamps(&message, &transcript),
        transcript
    );

    for clock_format in [ClockFormat::TwelveHour, ClockFormat::TwentyFourHour] {
        let footer =
            history_cell::FinalMessageSeparator::new(Some(3_723), /*runtime_metrics*/ None)
                .with_completed_at(
                    Local.timestamp_opt(COMPLETED_AT, /*nsecs*/ 0).unwrap(),
                    clock_format,
                );
        assert_eq!(
            normalize_completion_timestamps(
                &footer,
                lines_to_single_string(&footer.display_lines(/*width*/ 80))
            ),
            "  Worked for [duration] • [completion time]\n"
        );
    }
}
