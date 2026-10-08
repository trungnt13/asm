//! Verifies that root service-tier changes reach existing child turns and future work.

use anyhow::Result;
use codex_core::TurnInputRequest;
use codex_core::config::AgentRoleConfig;
use codex_core::config::Config;
use codex_features::Feature;
use codex_protocol::config_types::SERVICE_TIER_DEFAULT_REQUEST_VALUE;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ResponseMock;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call_with_namespace;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once_match;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::submit_thread_settings;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use core_test_support::wait_for_event_match;
use pretty_assertions::assert_eq;
use serde_json::json;
use test_case::test_case;

const ROOT_PROMPT: &str = "spawn the service-tier worker";
const CHILD_PROMPT: &str = "pause the service-tier worker";
const FOLLOWUP_PROMPT: &str = "continue the service-tier worker later";
const FRESH_ROOT_PROMPT: &str = "spawn another service-tier worker";
const FRESH_CHILD_PROMPT: &str = "finish the new service-tier worker";
const SPAWN_CALL_ID: &str = "spawn-service-tier-worker";
const FRESH_SPAWN_CALL_ID: &str = "spawn-fresh-service-tier-worker";
const PAUSE_CALL_ID: &str = "pause-service-tier-worker";
const PRIORITY_ROLE: &str = "priority-worker";
const LOCAL_COMPACT_PROMPT: &str = "Summarize this service-tier child.";

fn body_contains(request: &wiremock::Request, text: &str) -> bool {
    let body = match request
        .headers
        .get("content-encoding")
        .and_then(|encoding| encoding.to_str().ok())
    {
        Some(encoding) if encoding.eq_ignore_ascii_case("zstd") => {
            zstd::stream::decode_all(std::io::Cursor::new(&request.body)).ok()
        }
        _ => Some(request.body.clone()),
    };
    body.and_then(|body| String::from_utf8(body).ok())
        .is_some_and(|body| body.contains(text))
}

fn assert_request_service_tier(request: &ResponseMock, expected: Option<&str>) {
    assert_eq!(
        request
            .single_request()
            .body_json()
            .get("service_tier")
            .and_then(serde_json::Value::as_str),
        expected
    );
}

fn configure_priority_role(config: &mut Config) {
    for feature in [Feature::Collab, Feature::MultiAgentV2] {
        config
            .features
            .enable(feature)
            .expect("test config should allow feature update");
    }
    config.model_provider.request_max_retries = Some(0);
    config.model_provider.stream_max_retries = Some(0);
    let role_path = config.codex_home.join("priority-worker.toml");
    std::fs::write(&role_path, "service_tier = \"priority\"\n")
        .expect("priority role should be written");
    config.agent_roles.insert(
        PRIORITY_ROLE.to_string(),
        AgentRoleConfig {
            description: Some("Role with a configured priority tier".to_string()),
            config_file: Some(role_path.to_path_buf()),
            nickname_candidates: None,
        },
    );
}

async fn wait_for_turn_complete(thread: &codex_core::CodexThread) {
    wait_for_event(thread, |event| matches!(event, EventMsg::TurnComplete(_))).await;
}

async fn mount_root_collaboration_call(
    server: &wiremock::MockServer,
    prompt: &'static str,
    call_id: &'static str,
    arguments: serde_json::Value,
) -> (ResponseMock, ResponseMock) {
    let response_id = format!("root-{call_id}");
    let root_request = mount_sse_once_match(
        server,
        move |request: &wiremock::Request| {
            body_contains(request, prompt) && !body_contains(request, call_id)
        },
        sse(vec![
            ev_response_created(&response_id),
            ev_function_call_with_namespace(
                call_id,
                "collaboration",
                "spawn_agent",
                &arguments.to_string(),
            ),
            ev_completed(&response_id),
        ]),
    )
    .await;

    let completion_id = format!("root-{call_id}-finished");
    let completion_request = mount_sse_once_match(
        server,
        move |request: &wiremock::Request| {
            body_contains(request, prompt) && body_contains(request, call_id)
        },
        sse(vec![
            ev_response_created(&completion_id),
            ev_assistant_message(&format!("{completion_id}-message"), "worker started"),
            ev_completed(&completion_id),
        ]),
    )
    .await;
    (root_request, completion_request)
}

#[test_case("gpt-6-sol", ReasoningEffort::High, Some("priority"); "sol high uses fast")]
#[test_case("gpt-6-luna", ReasoningEffort::Max, Some("priority"); "luna max uses fast")]
#[test_case("gpt-6-sol", ReasoningEffort::Low, None; "unmatched effort inherits standard")]
#[test_case("gpt-6-luna", ReasoningEffort::High, None; "unmatched model effort pair inherits standard")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn configured_model_effort_pair_overrides_only_matching_subagent(
    child_model: &str,
    child_effort: ReasoningEffort,
    expected_child_tier: Option<&str>,
) -> Result<()> {
    let server = start_mock_server().await;
    let mut builder = test_codex().with_model("gpt-6-sol").with_config(|config| {
        configure_priority_role(config);
        config.features.enable(Feature::FastMode).unwrap();
        config.multi_agent_v2.expose_spawn_agent_model_overrides = true;
        config.model_reasoning_effort = Some(ReasoningEffort::High);
        config.subagent_service_tiers.insert(
            "gpt-6-sol".to_string(),
            std::collections::HashMap::from([(ReasoningEffort::High, "fast".to_string())]),
        );
        config.subagent_service_tiers.insert(
            "gpt-6-luna".to_string(),
            std::collections::HashMap::from([(ReasoningEffort::Max, "fast".to_string())]),
        );
    });
    let test = builder.build_with_auto_env(&server).await?;
    let mut created_threads = test.thread_manager.subscribe_thread_created();
    let root_request = mount_root_collaboration_call(
        &server,
        ROOT_PROMPT,
        SPAWN_CALL_ID,
        json!({
            "message": CHILD_PROMPT,
            "task_name": "worker",
            "model": child_model,
            "reasoning_effort": child_effort.as_str(),
            "fork_turns": "none",
        }),
    )
    .await;
    let child_request = mount_completed_child(&server, CHILD_PROMPT, ROOT_PROMPT).await;

    test.submit_text_turn(ROOT_PROMPT).await?;
    let child_id = created_threads.recv().await?;
    let child = test.thread_manager.get_thread(child_id).await?;
    wait_for_turn_complete(&child).await;
    assert_request_service_tier(&root_request.0, None);
    assert_request_service_tier(&child_request, expected_child_tier);
    assert_eq!(
        child.config_snapshot().await.service_tier.as_deref(),
        expected_child_tier
    );
    Ok(())
}

#[test_case("unknown-tier", true, "not supported"; "unsupported matched tier fails")]
#[test_case("fast", false, "requires features.fast_mode"; "fast rule needs fast mode")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_subagent_service_tier_rule_rejects_spawn(
    tier: &str,
    fast_mode_enabled: bool,
    expected_error: &str,
) -> Result<()> {
    let server = start_mock_server().await;
    let configured_tier = tier.to_string();
    let mut builder = test_codex()
        .with_model("gpt-6-sol")
        .with_config(move |config| {
            configure_priority_role(config);
            if fast_mode_enabled {
                config.features.enable(Feature::FastMode).unwrap();
            } else {
                config.features.disable(Feature::FastMode).unwrap();
            }
            config.model_reasoning_effort = Some(ReasoningEffort::High);
            config.subagent_service_tiers.insert(
                "gpt-6-sol".to_string(),
                std::collections::HashMap::from([(ReasoningEffort::High, configured_tier.clone())]),
            );
        });
    let test = builder.build_with_auto_env(&server).await?;
    let mut created_threads = test.thread_manager.subscribe_thread_created();
    let (_, completion_request) = mount_root_collaboration_call(
        &server,
        ROOT_PROMPT,
        SPAWN_CALL_ID,
        json!({ "message": CHILD_PROMPT, "task_name": "worker", "fork_turns": "none" }),
    )
    .await;
    test.submit_text_turn(ROOT_PROMPT).await?;
    let output = completion_request
        .single_request()
        .function_call_output_text(SPAWN_CALL_ID)
        .expect("failed spawn should return a tool error");
    assert!(output.contains(expected_error), "{output}");
    assert!(created_threads.try_recv().is_err());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subagent_rule_tracks_live_model_and_effort_changes() -> Result<()> {
    let server = start_mock_server().await;
    let mut builder = test_codex().with_model("gpt-6-sol").with_config(|config| {
        configure_priority_role(config);
        config.features.enable(Feature::FastMode).unwrap();
        config.model_reasoning_effort = Some(ReasoningEffort::High);
        config.subagent_service_tiers.insert(
            "gpt-6-sol".to_string(),
            std::collections::HashMap::from([(ReasoningEffort::High, "fast".to_string())]),
        );
        config.subagent_service_tiers.insert(
            "gpt-6-luna".to_string(),
            std::collections::HashMap::from([(ReasoningEffort::Max, "fast".to_string())]),
        );
    });
    let test = builder.build_with_auto_env(&server).await?;
    let mut created_threads = test.thread_manager.subscribe_thread_created();
    mount_root_collaboration_call(
        &server,
        ROOT_PROMPT,
        SPAWN_CALL_ID,
        json!({ "message": CHILD_PROMPT, "task_name": "worker", "fork_turns": "none" }),
    )
    .await;
    let initial_request = mount_completed_child(&server, CHILD_PROMPT, ROOT_PROMPT).await;
    test.submit_text_turn(ROOT_PROMPT).await?;
    let child_id = created_threads.recv().await?;
    let child = test.thread_manager.get_thread(child_id).await?;
    wait_for_turn_complete(&child).await;
    assert_request_service_tier(&initial_request, Some("priority"));

    submit_thread_settings(
        &child,
        ThreadSettingsOverrides {
            effort: Some(Some(ReasoningEffort::Low)),
            ..Default::default()
        },
    )
    .await?;
    let low_request = mount_completed_child(&server, "child low effort", ROOT_PROMPT).await;
    child
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "child low effort".to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    wait_for_turn_complete(&child).await;
    assert_request_service_tier(&low_request, None);

    submit_thread_settings(
        &child,
        ThreadSettingsOverrides {
            model: Some("gpt-6-luna".to_string()),
            effort: Some(Some(ReasoningEffort::Max)),
            ..Default::default()
        },
    )
    .await?;
    let luna_request = mount_completed_child(&server, "child luna max", ROOT_PROMPT).await;
    child
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "child luna max".to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    wait_for_turn_complete(&child).await;
    assert_request_service_tier(&luna_request, Some("priority"));
    Ok(())
}

async fn mount_completed_child(
    server: &wiremock::MockServer,
    prompt: &'static str,
    root_prompt: &'static str,
) -> ResponseMock {
    mount_sse_once_match(
        server,
        move |request: &wiremock::Request| {
            body_contains(request, prompt) && !body_contains(request, root_prompt)
        },
        sse(vec![
            ev_response_created(prompt),
            ev_assistant_message(&format!("{prompt}-message"), "worker completed"),
            ev_completed(prompt),
        ]),
    )
    .await
}

#[test_case(Some("priority"), None, false; "disabling fast mode updates active and idle child work")]
#[test_case(Some("priority"), Some("default"), false; "explicit default updates active and idle child work")]
#[test_case(None, Some("priority"), false; "enabling fast mode updates active and idle child work")]
#[test_case(Some("priority"), None, true; "matching child rule survives root tier changes and compaction")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn root_service_tier_change_updates_existing_subagent(
    initial_service_tier: Option<&str>,
    updated_service_tier: Option<&str>,
    configured_override: bool,
) -> Result<()> {
    let server = start_mock_server().await;
    let initial_service_tier_owned = initial_service_tier.map(str::to_string);
    let updated_request_service_tier = configured_override.then_some("priority").or_else(|| {
        updated_service_tier
            .filter(|service_tier| *service_tier != SERVICE_TIER_DEFAULT_REQUEST_VALUE)
    });
    let mut builder = test_codex()
        .with_model(if configured_override {
            "gpt-6-sol"
        } else {
            "gpt-5.6-sol"
        })
        .with_config(move |config| {
            config.service_tier = initial_service_tier_owned;
            configure_priority_role(config);
            if configured_override {
                config.features.enable(Feature::FastMode).unwrap();
                config.model_reasoning_effort = Some(ReasoningEffort::High);
                config.model_provider.name = "OpenAI (test)".to_string();
                config.compact_prompt = Some(LOCAL_COMPACT_PROMPT.to_string());
                config.subagent_service_tiers.insert(
                    "gpt-6-sol".to_string(),
                    std::collections::HashMap::from([(ReasoningEffort::High, "fast".to_string())]),
                );
            }
        });
    let test = builder.build_with_auto_env(&server).await?;
    assert!(!test.config.features.enabled(Feature::StepModelSwitching));
    let mut created_threads = test.thread_manager.subscribe_thread_created();

    mount_root_collaboration_call(
        &server,
        ROOT_PROMPT,
        SPAWN_CALL_ID,
        json!({
            "message": CHILD_PROMPT,
            "task_name": "worker",
            "agent_type": PRIORITY_ROLE,
            "fork_turns": "none",
        }),
    )
    .await;

    let initial_child_request = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            body_contains(request, CHILD_PROMPT)
                && !body_contains(request, ROOT_PROMPT)
                && !body_contains(request, PAUSE_CALL_ID)
        },
        sse(vec![
            ev_response_created("child-paused"),
            ev_function_call_with_namespace(
                PAUSE_CALL_ID,
                "collaboration",
                "wait_agent",
                &json!({ "timeout_ms": 30_000 }).to_string(),
            ),
            ev_completed("child-paused"),
        ]),
    )
    .await;
    let continued_child_request = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            body_contains(request, PAUSE_CALL_ID) && !body_contains(request, ROOT_PROMPT)
        },
        sse(vec![
            ev_response_created("child-continued"),
            ev_assistant_message("child-continued-message", "child continued"),
            ev_completed("child-continued"),
        ]),
    )
    .await;
    test.submit_text_turn(ROOT_PROMPT).await?;
    let child_thread_id = created_threads.recv().await?;
    let child = test.thread_manager.get_thread(child_thread_id).await?;
    let original_child_service_tier = child.config_snapshot().await.service_tier;
    assert_eq!(original_child_service_tier.as_deref(), initial_service_tier);
    wait_for_event_match(child.as_ref(), |event| match event {
        EventMsg::CollabWaitingBegin(request) if request.call_id == PAUSE_CALL_ID => Some(()),
        EventMsg::Error(error) => panic!("child failed before pausing: {}", error.message),
        EventMsg::TurnComplete(completed) => {
            panic!("child completed before pausing: {completed:?}")
        }
        _ => None,
    })
    .await;
    assert_request_service_tier(&initial_child_request, initial_service_tier);

    submit_thread_settings(
        &test.codex,
        ThreadSettingsOverrides {
            service_tier: Some(updated_service_tier.map(str::to_string)),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(
        child.config_snapshot().await.service_tier,
        original_child_service_tier,
        "the root routing policy must not rewrite child-owned settings"
    );

    child
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "continue after the service-tier update".to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    wait_for_turn_complete(&child).await;
    assert_request_service_tier(&continued_child_request, updated_request_service_tier);

    let compaction_sse = if configured_override {
        sse(vec![
            ev_response_created("child-local-compaction"),
            ev_assistant_message("child-local-summary", "summarized child history"),
            ev_completed("child-local-compaction"),
        ])
    } else {
        sse(vec![
            ev_response_created("child-remote-compaction"),
            json!({
                "type": "response.output_item.done",
                "item": {
                    "type": "compaction",
                    "encrypted_content": "compacted child history",
                },
            }),
            ev_completed("child-remote-compaction"),
        ])
    };
    let child_compaction_request = mount_sse_once_match(
        &server,
        move |request: &wiremock::Request| {
            body_contains(
                request,
                if configured_override {
                    LOCAL_COMPACT_PROMPT
                } else {
                    "compaction_trigger"
                },
            )
        },
        compaction_sse,
    )
    .await;
    child.submit(Op::Compact).await?;
    wait_for_turn_complete(&child).await;
    assert_request_service_tier(&child_compaction_request, updated_request_service_tier);

    let future_child_request = mount_completed_child(&server, FOLLOWUP_PROMPT, ROOT_PROMPT).await;
    child
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: FOLLOWUP_PROMPT.to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    wait_for_turn_complete(&child).await;
    assert_request_service_tier(&future_child_request, updated_request_service_tier);

    mount_root_collaboration_call(
        &server,
        FRESH_ROOT_PROMPT,
        FRESH_SPAWN_CALL_ID,
        json!({
            "message": FRESH_CHILD_PROMPT,
            "task_name": "fresh",
            "agent_type": PRIORITY_ROLE,
            "fork_turns": "none",
        }),
    )
    .await;
    let fresh_child_request =
        mount_completed_child(&server, FRESH_CHILD_PROMPT, FRESH_ROOT_PROMPT).await;
    test.submit_text_turn(FRESH_ROOT_PROMPT).await?;
    let fresh_thread_id = created_threads.recv().await?;
    let fresh_thread = test.thread_manager.get_thread(fresh_thread_id).await?;
    wait_for_turn_complete(&fresh_thread).await;
    assert_request_service_tier(&fresh_child_request, updated_request_service_tier);
    Ok(())
}

#[test_case(false; "unmatched child reload inherits root")]
#[test_case(true; "matching child reload keeps configured tier")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn evicted_role_subagent_uses_selected_service_tier_after_reload(
    configured_override: bool,
) -> Result<()> {
    let server = start_mock_server().await;
    let mut builder = test_codex()
        .with_model(if configured_override {
            "gpt-6-sol"
        } else {
            "gpt-5.6-sol"
        })
        .with_config(move |config| {
            config.service_tier = Some("priority".to_string());
            config.multi_agent_v2.max_concurrent_threads_per_session = 2;
            configure_priority_role(config);
            if configured_override {
                config.features.enable(Feature::FastMode).unwrap();
                config.model_reasoning_effort = Some(ReasoningEffort::High);
                config.subagent_service_tiers.insert(
                    "gpt-6-sol".to_string(),
                    std::collections::HashMap::from([(ReasoningEffort::High, "fast".to_string())]),
                );
            }
        });
    let test = builder.build_with_auto_env(&server).await?;
    let mut created_threads = test.thread_manager.subscribe_thread_created();

    mount_root_collaboration_call(
        &server,
        ROOT_PROMPT,
        SPAWN_CALL_ID,
        json!({
            "message": CHILD_PROMPT,
            "task_name": "original",
            "agent_type": PRIORITY_ROLE,
            "fork_turns": "none",
        }),
    )
    .await;
    let original_request = mount_completed_child(&server, CHILD_PROMPT, ROOT_PROMPT).await;
    test.submit_text_turn(ROOT_PROMPT).await?;
    let original_thread_id = created_threads.recv().await?;
    let original_thread = test.thread_manager.get_thread(original_thread_id).await?;
    wait_for_turn_complete(&original_thread).await;
    assert_request_service_tier(&original_request, Some("priority"));
    drop(original_thread);

    mount_root_collaboration_call(
        &server,
        FRESH_ROOT_PROMPT,
        FRESH_SPAWN_CALL_ID,
        json!({
            "message": FRESH_CHILD_PROMPT,
            "task_name": "replacement",
            "fork_turns": "none",
        }),
    )
    .await;
    mount_completed_child(&server, FRESH_CHILD_PROMPT, FRESH_ROOT_PROMPT).await;
    test.submit_text_turn(FRESH_ROOT_PROMPT).await?;
    let replacement_thread_id = created_threads.recv().await?;
    let replacement_thread = test
        .thread_manager
        .get_thread(replacement_thread_id)
        .await?;
    wait_for_turn_complete(&replacement_thread).await;
    assert!(
        test.thread_manager
            .get_thread(original_thread_id)
            .await
            .is_err()
    );

    submit_thread_settings(
        &test.codex,
        ThreadSettingsOverrides {
            service_tier: Some(None),
            ..Default::default()
        },
    )
    .await?;
    test.thread_manager
        .ensure_multi_agent_v2_child_loaded(original_thread_id)
        .await?;
    let reloaded_thread = test.thread_manager.get_thread(original_thread_id).await?;
    assert_eq!(
        reloaded_thread
            .config_snapshot()
            .await
            .service_tier
            .as_deref(),
        Some(if configured_override {
            "priority"
        } else {
            "default"
        }),
        "reload should keep a matching child rule and otherwise inherit the root"
    );

    let reloaded_request = mount_completed_child(&server, FOLLOWUP_PROMPT, ROOT_PROMPT).await;
    reloaded_thread
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: FOLLOWUP_PROMPT.to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    wait_for_turn_complete(&reloaded_thread).await;
    assert_request_service_tier(&reloaded_request, configured_override.then_some("priority"));
    reloaded_thread.shutdown_and_wait().await?;
    test.codex.shutdown_and_wait().await?;

    Ok(())
}
