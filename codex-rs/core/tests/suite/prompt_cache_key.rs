use std::time::Duration;

use anyhow::Result;
use anyhow::anyhow;
use codex_features::Feature;
use codex_login::CodexAuth;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_protocol::protocol::TurnEnvironmentSelection;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call_with_namespace;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once_match;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

const ROOT_PROMPT: &str = "delegate the cache audit";
const CHILD_TASK: &str = "inspect the repository";
const SPAWN_CALL_ID: &str = "spawn-worker";
const COLLABORATION_NAMESPACE: &str = "collaboration";

fn body_contains(request: &wiremock::Request, text: &str) -> bool {
    serde_json::from_slice::<Value>(&request.body).is_ok_and(|body| body.to_string().contains(text))
}

fn request_has_input_type(request: &wiremock::Request, input_type: &str) -> bool {
    serde_json::from_slice::<Value>(&request.body)
        .ok()
        .and_then(|body| body.get("input").and_then(Value::as_array).cloned())
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| item.get("type").and_then(Value::as_str) == Some(input_type))
        })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn api_key_subagent_uses_session_id_as_prompt_cache_key() -> Result<()> {
    let server = start_mock_server().await;
    let spawn_args = serde_json::to_string(&json!({
        "message": CHILD_TASK,
        "task_name": "worker",
    }))?;
    let root_request = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            body_contains(request, ROOT_PROMPT)
                && !request_has_input_type(request, "agent_message")
                && !body_contains(request, SPAWN_CALL_ID)
        },
        sse(vec![
            ev_response_created("root-response-1"),
            ev_function_call_with_namespace(
                SPAWN_CALL_ID,
                COLLABORATION_NAMESPACE,
                "spawn_agent",
                &spawn_args,
            ),
            ev_completed("root-response-1"),
        ]),
    )
    .await;
    let child_request = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            body_contains(request, CHILD_TASK) && !body_contains(request, SPAWN_CALL_ID)
        },
        sse(vec![
            ev_response_created("child-response"),
            ev_assistant_message("child-message", "inspection complete"),
            ev_completed("child-response"),
        ]),
    )
    .await;
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| body_contains(request, SPAWN_CALL_ID),
        sse(vec![
            ev_response_created("root-response-2"),
            ev_assistant_message("root-message", "worker finished"),
            ev_completed("root-response-2"),
        ]),
    )
    .await;

    let mut builder = test_codex()
        .with_auth(CodexAuth::from_api_key("dummy"))
        .with_config(|config| {
            config
                .features
                .enable(Feature::Collab)
                .expect("test config should allow feature update");
            config
                .features
                .enable(Feature::MultiAgentV2)
                .expect("test config should allow feature update");
        });
    let test = builder.build(&server).await?;
    let expected_session_id = test.session_configured.session_id.to_string();
    test.submit_turn(ROOT_PROMPT).await?;

    let root_request = root_request
        .requests()
        .into_iter()
        .next()
        .expect("root request");
    let root_thread_id = root_request.header("thread-id").expect("root thread ID");
    let child_request = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(request) = child_request.requests().into_iter().find(|request| {
                request
                    .header("thread-id")
                    .is_some_and(|thread_id| thread_id != root_thread_id)
            }) {
                break request;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| anyhow!("timed out waiting for the child request"))?;
    let child_thread_id = child_request.header("thread-id").expect("child thread ID");

    assert_eq!(
        json!({
            "differentThreadIds": root_thread_id != child_thread_id,
            "root": {
                "sessionId": root_request.header("session-id"),
                "threadId": &root_thread_id,
                "clientRequestId": root_request.header("x-client-request-id"),
                "promptCacheKey": root_request.body_json()["prompt_cache_key"].clone(),
            },
            "child": {
                "sessionId": child_request.header("session-id"),
                "threadId": &child_thread_id,
                "clientRequestId": child_request.header("x-client-request-id"),
                "promptCacheKey": child_request.body_json()["prompt_cache_key"].clone(),
            },
        }),
        json!({
            "differentThreadIds": true,
            "root": {
                "sessionId": &expected_session_id,
                "threadId": &root_thread_id,
                "clientRequestId": &root_thread_id,
                "promptCacheKey": &expected_session_id,
            },
            "child": {
                "sessionId": &expected_session_id,
                "threadId": &child_thread_id,
                "clientRequestId": &child_thread_id,
                "promptCacheKey": &expected_session_id,
            },
        })
    );

    Ok(())
}

#[test_case::test_case(/*ephemeral*/ false, ThreadHistoryMode::Legacy; "saved")]
#[test_case::test_case(/*ephemeral*/ true, ThreadHistoryMode::Legacy; "ephemeral")]
#[test_case::test_case(/*ephemeral*/ false, ThreadHistoryMode::Paginated; "saved_paginated")]
#[tokio::test]
async fn root_fork_shares_cache_routing_but_keeps_session_identity(
    ephemeral: bool,
    history_mode: ThreadHistoryMode,
) -> Result<()> {
    use codex_core::ForkSnapshot;
    use codex_core::StartThreadOptions;
    use core_test_support::responses::mount_sse_sequence;

    let server = start_mock_server().await;
    let response_ids = if ephemeral {
        vec!["parent", "fork"]
    } else {
        vec!["parent", "fork", "resumed"]
    };
    let requests = mount_sse_sequence(
        &server,
        response_ids
            .into_iter()
            .map(|id| sse(vec![ev_completed(id)]))
            .collect(),
    )
    .await;
    let mut test = test_codex()
        .with_history_mode(history_mode)
        .build_with_auto_env(&server)
        .await?;
    test.submit_text_turn("parent").await?;
    test.codex.flush_rollout().await?;
    let parent_session = test.session_configured.session_id.to_string();
    let mut config = test.config.clone();
    config.ephemeral = ephemeral;
    let options = StartThreadOptions {
        environments: Some(
            test.codex
                .environment_selections()
                .await
                .into_iter()
                .map(TurnEnvironmentSelection::into_request)
                .collect(),
        ),
        history_mode: Some(history_mode),
        ..StartThreadOptions::new(config)
    };
    let fork = match history_mode {
        ThreadHistoryMode::Legacy => {
            test.thread_manager
                .fork_legacy_thread(
                    ForkSnapshot::TruncateBeforeNthUserMessage(usize::MAX),
                    options,
                    test.codex.rollout_path().expect("parent rollout"),
                )
                .await?
        }
        ThreadHistoryMode::Paginated => {
            let prepared = test
                .thread_store
                .prepare_fork(codex_thread_store::PrepareForkParams {
                    thread_id: test.session_configured.thread_id,
                    boundary: codex_thread_store::ForkBoundary::Latest,
                })
                .await?;
            test.thread_manager
                .fork_prepared_thread(options, prepared)
                .await?
        }
    };
    let fork_session = fork.session_configured.session_id.to_string();
    assert_ne!(fork_session, parent_session);
    test.codex = fork.thread;
    test.submit_text_turn("side").await?;
    if !ephemeral {
        let fork_path = test.codex.rollout_path().expect("saved fork rollout");
        test.codex.shutdown_and_wait().await?;
        let context = test
            .thread_store
            .load_latest_model_context(codex_thread_store::LoadThreadHistoryParams {
                thread_id: fork.thread_id,
                include_archived: false,
            })
            .await?;
        let resumed = test
            .thread_manager
            .resume_thread_with_history(
                test.config.clone(),
                codex_history::InitialHistory::Resumed(codex_history::ResumedHistory {
                    history_revision: context.revision,
                    conversation_id: fork.thread_id,
                    history: std::sync::Arc::new(context.items),
                    rollout_path: Some(fork_path),
                }),
                codex_core::test_support::auth_manager_from_auth(CodexAuth::from_api_key("dummy")),
                /*parent_trace*/ None,
                codex_protocol::mcp::ClientMcpExtensions::default(),
            )
            .await?;
        assert_eq!(
            resumed.session_configured.session_id.to_string(),
            fork_session
        );
        test.codex = resumed.thread;
        test.submit_text_turn("resumed").await?;
    }
    let requests = requests.requests();
    assert_eq!(requests.len(), if ephemeral { 2 } else { 3 });
    for request in &requests[1..] {
        let body = request.body_json();
        let metadata: Value = serde_json::from_str(
            body["client_metadata"]["x-codex-turn-metadata"]
                .as_str()
                .expect("turn metadata"),
        )?;
        assert_eq!(
            json!({
                "cache": body["prompt_cache_key"],
                "route": request.header("session-id"),
                "session": metadata["session_id"],
                "thread": request.header("thread-id"),
                "tools": body["tools"],
            }),
            json!({
                "cache": parent_session, "route": parent_session,
                "session": fork_session, "thread": fork.thread_id.to_string(),
                "tools": requests[0].body_json()["tools"],
            }),
        );
    }
    Ok(())
}

#[tokio::test]
async fn saved_nested_fork_retains_cache_routing_on_resume_without_ancestors() -> Result<()> {
    use codex_core::ForkSnapshot;
    use codex_core::StartThreadOptions;
    use core_test_support::responses::mount_sse_sequence;

    let server = start_mock_server().await;
    let requests = mount_sse_sequence(
        &server,
        ["parent", "fork", "nested", "resumed", "legacy-resumed"]
            .into_iter()
            .map(|id| sse(vec![ev_completed(id)]))
            .collect(),
    )
    .await;
    let mut builder = test_codex().with_history_mode(ThreadHistoryMode::Legacy);
    let mut test = builder.build_with_auto_env(&server).await?;
    let parent_session = test.session_configured.session_id;
    test.submit_text_turn("parent").await?;
    test.codex.flush_rollout().await?;
    let parent_path = test.codex.rollout_path().expect("parent rollout");
    let parent_meta = codex_rollout::read_session_meta_line(&parent_path).await?;
    assert_eq!(parent_meta.meta.prompt_cache_key, None);
    let first = test
        .thread_manager
        .fork_legacy_thread(
            ForkSnapshot::TruncateBeforeNthUserMessage(usize::MAX),
            StartThreadOptions {
                environments: Some(
                    test.codex
                        .environment_selections()
                        .await
                        .into_iter()
                        .map(TurnEnvironmentSelection::into_request)
                        .collect(),
                ),
                ..StartThreadOptions::new(test.config.clone())
            },
            parent_path.clone(),
        )
        .await?;
    test.codex.shutdown_and_wait().await?;
    test.codex = first.thread;
    test.submit_text_turn("fork").await?;
    test.codex.flush_rollout().await?;
    let first_path = test.codex.rollout_path().expect("first fork rollout");
    let nested = test
        .thread_manager
        .fork_legacy_thread(
            ForkSnapshot::TruncateBeforeNthUserMessage(usize::MAX),
            StartThreadOptions {
                environments: Some(
                    test.codex
                        .environment_selections()
                        .await
                        .into_iter()
                        .map(TurnEnvironmentSelection::into_request)
                        .collect(),
                ),
                ..StartThreadOptions::new(test.config.clone())
            },
            first_path.clone(),
        )
        .await?;
    let nested_session = nested.session_configured.session_id;
    let nested_thread = nested.thread_id;
    assert_ne!(nested_session, first.session_configured.session_id);
    assert_ne!(nested_session, parent_session);
    test.codex.shutdown_and_wait().await?;
    test.codex = nested.thread;
    test.submit_text_turn("nested").await?;
    test.codex.flush_rollout().await?;
    let nested_path = test.codex.rollout_path().expect("nested rollout");
    let nested_meta = codex_rollout::read_session_meta_line(&nested_path).await?;
    assert_eq!(
        (
            nested_meta.meta.session_id,
            nested_meta.meta.prompt_cache_key
        ),
        (nested_session, Some(parent_session)),
    );
    test.codex.shutdown_and_wait().await?;
    tokio::fs::remove_file(parent_path).await?;
    tokio::fs::remove_file(first_path).await?;
    let history = codex_rollout::RolloutRecorder::get_rollout_history(&nested_path).await?;
    let resumed = test
        .thread_manager
        .resume_thread_with_history(
            test.config.clone(),
            history,
            codex_core::test_support::auth_manager_from_auth(CodexAuth::from_api_key("dummy")),
            /*parent_trace*/ None,
            codex_protocol::mcp::ClientMcpExtensions::default(),
        )
        .await?;
    assert_eq!(resumed.session_configured.session_id, nested_session);
    test.codex = resumed.thread;
    test.submit_text_turn("resumed").await?;
    test.codex.shutdown_and_wait().await?;
    // A legacy fork's own header must win over cache keys in copied ancestor headers.
    let rollout = tokio::fs::read_to_string(&nested_path).await?;
    let mut lines = rollout
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    lines[0]["payload"]
        .as_object_mut()
        .expect("session metadata")
        .remove("prompt_cache_key");
    let rollout = lines
        .into_iter()
        .map(|line| serde_json::to_string(&line))
        .collect::<Result<Vec<_>, _>>()?
        .join("\n")
        + "\n";
    tokio::fs::write(&nested_path, rollout).await?;
    let history = codex_rollout::RolloutRecorder::get_rollout_history(&nested_path).await?;
    let legacy_resumed = test
        .thread_manager
        .resume_thread_with_history(
            test.config.clone(),
            history,
            codex_core::test_support::auth_manager_from_auth(CodexAuth::from_api_key("dummy")),
            /*parent_trace*/ None,
            codex_protocol::mcp::ClientMcpExtensions::default(),
        )
        .await?;
    test.codex = legacy_resumed.thread;
    test.submit_text_turn("legacy-resumed").await?;
    let actual = requests
        .requests()
        .into_iter()
        .map(|request| {
            let body = request.body_json();
            let metadata: Value = serde_json::from_str(
                body["client_metadata"]["x-codex-turn-metadata"]
                    .as_str()
                    .expect("turn metadata"),
            )
            .expect("valid turn metadata");
            json!({
                "cache": body["prompt_cache_key"],
                "route": request.header("session-id"),
                "session": metadata["session_id"],
                "thread": request.header("thread-id"),
            })
        })
        .collect::<Vec<_>>();
    let expected = [
        (
            parent_session,
            test.session_configured.thread_id,
            parent_session,
        ),
        (
            first.session_configured.session_id,
            first.thread_id,
            parent_session,
        ),
        (nested_session, nested_thread, parent_session),
        (nested_session, nested_thread, parent_session),
        (nested_session, nested_thread, nested_session),
    ]
    .into_iter()
    .map(|(session, thread, cache)| {
        json!({
            "cache": cache.to_string(),
            "route": cache.to_string(),
            "session": session.to_string(),
            "thread": thread.to_string(),
        })
    })
    .collect::<Vec<_>>();
    assert_eq!(actual, expected);
    Ok(())
}
