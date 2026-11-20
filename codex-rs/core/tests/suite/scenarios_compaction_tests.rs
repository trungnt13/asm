//! Snapshots context retention across compaction, interruption, restart, and normal inference.

use super::*;
use codex_core::TurnInputSubmission;
use codex_protocol::AgentPath;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::protocol::TurnAbortReason;
use core_test_support::responses::ev_function_call;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use pretty_assertions::assert_eq;
use tokio::sync::oneshot;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interrupt_during_remote_compaction_preserves_summary_on_resume() -> Result<()> {
    skip_if_no_network!(Ok(()));
    const SUMMARY: &str = "REMOTE_COMPACTION_CHECKPOINT";
    const POST_TURN_SUMMARY: &str = "POST_TURN_COMPACTION_CHECKPOINT";
    const FINAL_ANSWER: &str = "Fixed escaping in parser.rs; all tests pass.";
    let chunk = |gate, events| StreamingSseChunk {
        gate,
        body: sse(events),
    };
    let (release_summary, summary_gate) = oneshot::channel();
    let (release_post_summary, post_summary_gate) = oneshot::channel();
    let (server, _) = start_streaming_sse_server(vec![
        vec![chunk(
            None,
            vec![
                ev_function_call(
                    "plan",
                    "update_plan",
                    r#"{"plan":[{"step":"Fix escaping in parser.rs","status":"in_progress"}]}"#,
                ),
                ev_completed_with_tokens("first", /*total_tokens*/ 200_000),
            ],
        )],
        vec![
            chunk(None, vec![ev_response_created("compact")]),
            chunk(
                Some(summary_gate),
                vec![
                    compaction_output(SUMMARY),
                    ev_completed_with_tokens("compact", /*total_tokens*/ 50),
                ],
            ),
        ],
        vec![chunk(
            None,
            vec![
                ev_assistant_message("answer", FINAL_ANSWER),
                ev_completed_with_tokens("answer", /*total_tokens*/ 200_000),
            ],
        )],
        vec![
            chunk(None, vec![ev_response_created("post-compact")]),
            chunk(
                Some(post_summary_gate),
                vec![
                    compaction_output(POST_TURN_SUMMARY),
                    ev_completed_with_tokens("post-compact", /*total_tokens*/ 50),
                ],
            ),
        ],
        vec![chunk(
            None,
            vec![
                ev_assistant_message("resumed", "Continuing with the escaping fix."),
                ev_completed_with_tokens("resume", /*total_tokens*/ 60),
            ],
        )],
    ])
    .await;
    let config_server = start_mock_server().await;
    let base_url = format!("{}/v1", server.uri());
    let test = test_codex()
        .with_model("gpt-5.4")
        .with_config(move |config| {
            config.model_provider.base_url = Some(base_url);
            config.model_provider.supports_websockets = false;
            config.model_auto_compact_token_limit = Some(100_000);
            config.model_post_turn_compact_threshold_percent = 50;
            config.update_plan_enabled = true;
            config.features.enable(Feature::MultiAgentV2).unwrap();
            config
                .features
                .disable(Feature::WakeOnAgentMessages)
                .unwrap();
            config
                .features
                .disable(Feature::EnableRequestCompression)
                .unwrap();
            config.multi_agent_v2.interrupt_at_item_boundary = true;
        })
        .build_with_auto_env(&config_server)
        .await?;
    let child = test
        .thread_manager
        .start_thread(StartThreadOptions {
            session_source: Some(SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                parent_thread_id: test.session_configured.thread_id,
                depth: 1,
                agent_path: Some(AgentPath::try_from("/root/worker").expect("child path")),
                agent_nickname: None,
                agent_role: None,
            })),
            ..test.start_thread_options().await
        })
        .await?
        .thread;
    child
        .start_or_steer_turn(TurnInputRequest::user_input(vec![text(
            "Find and fix the bug.",
        )]))
        .await?;
    let EventMsg::TurnStarted(started) =
        wait_for_event(&child, |event| matches!(event, EventMsg::TurnStarted(_))).await
    else {
        unreachable!("waited for turn start");
    };
    let mut previous_turn_id = started.turn_id;
    let next = |event: &EventMsg| {
        assert!(!matches!(event, EventMsg::Error(_)), "{event:?}");
        matches!(event, EventMsg::ItemCompleted(item) if matches!(item.item, TurnItem::ContextCompaction(_)))
            || matches!(
                event,
                EventMsg::TurnAborted(_) | EventMsg::TurnStarted(_) | EventMsg::TurnComplete(_)
            )
    };
    // The same child stops during mid-turn compaction, then after its completed answer.
    for (request_count, release_summary, expected_answer) in [
        (2, release_summary, None),
        (4, release_post_summary, Some(FINAL_ANSWER)),
    ] {
        server.wait_for_request_count(request_count).await;
        child.submit(Op::InterruptAgent).await?;
        // This reply confirms the interrupt was accepted while the summary is still sampling.
        child
            .update_thread_settings(ThreadSettingsOverrides::default())
            .await?;
        assert_eq!(child.agent_status().await, AgentStatus::Running);
        let resumed =
            child.start_or_steer_turn(TurnInputRequest::user_input(vec![text("Resume.")]));
        tokio::pin!(resumed);
        assert!(futures::poll!(resumed.as_mut()).is_pending());
        release_summary
            .send(())
            .expect("finish sampling the summary");
        assert!(matches!(
            wait_for_event(&child, next).await,
            EventMsg::ItemCompleted(_)
        ));
        let terminal = wait_for_event(&child, next).await;
        if let Some(answer) = expected_answer {
            let EventMsg::TurnComplete(completed) = terminal else {
                panic!("completed answer was lost: {terminal:?}");
            };
            assert_eq!(completed.last_agent_message.as_deref(), Some(answer));
            assert_eq!(completed.turn_id, previous_turn_id);
        } else {
            assert!(matches!(terminal,
                EventMsg::TurnAborted(event) if event.reason == TurnAbortReason::Interrupted));
        }
        let TurnInputSubmission::Started { turn_id, .. } = resumed.await? else {
            panic!("Resume must start a new turn");
        };
        assert_ne!(turn_id, previous_turn_id);
        let EventMsg::TurnStarted(started) = wait_for_event(&child, next).await else {
            panic!("expected resumed turn start");
        };
        assert_eq!(started.turn_id, turn_id);
        previous_turn_id = turn_id;
    }
    assert!(matches!(
        wait_for_event(&child, next).await,
        EventMsg::TurnComplete(_)
    ));

    let requests = server
        .requests()
        .await
        .iter()
        .map(|body| serde_json::from_slice::<serde_json::Value>(body))
        .collect::<serde_json::Result<Vec<_>>>()?;
    assert_eq!(requests.len(), 5);
    for (request_index, summary) in [(2, SUMMARY), (4, POST_TURN_SUMMARY)] {
        assert_eq!(
            requests[request_index - 1]["input"]
                .as_array()
                .expect("compaction input")
                .last(),
            Some(&json!({"type": "compaction_trigger"})),
        );
        let resumed_input = requests[request_index]["input"]
            .as_array()
            .expect("resumed input");
        let summaries = resumed_input
            .iter()
            .filter(|item| item["type"] == "compaction")
            .map(|item| item["encrypted_content"].clone())
            .collect::<Vec<_>>();
        assert_eq!(summaries, vec![json!(summary)]);
        let resume_messages = resumed_input
            .iter()
            .filter(|item| item["role"] == "user")
            .filter_map(|item| item["content"].as_array())
            .flatten()
            .filter_map(|content| content["text"].as_str())
            .filter(|text| *text == "Resume.")
            .collect::<Vec<_>>();
        assert_eq!(resume_messages, vec!["Resume."; request_index / 2]);
    }
    let entries = requests.iter().map(SnapshotEntry::body).collect::<Vec<_>>();
    insta::assert_snapshot!(context_snapshot::format_context_snapshot(
        "A child interrupted during mid-turn and post-turn remote compaction keeps both checkpoints, delivers its completed answer, and handles each Resume in a separate turn.",
        &entries,
        &ContextSnapshotOptions::default().rewrite_known_segments(),
    ));
    child.shutdown_and_wait().await?;
    server.shutdown().await;
    Ok(())
}
