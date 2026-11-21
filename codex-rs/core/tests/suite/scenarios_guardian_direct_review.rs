//! Direct review preserves live action rendering without executing commands or applying patches.

use anyhow::Result;
use codex_core::config::Constrained;
use codex_core::guardian_review::GuardianReviewResult;
use codex_features::Feature;
use codex_history::RolloutItem;
use codex_protocol::ThreadId;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::models::PermissionProfile;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::input_text;
use codex_protocol::protocol::AskForApproval;
use core_test_support::context_snapshot;
use core_test_support::context_snapshot::ContextSnapshotOptions;
use core_test_support::responses;
use core_test_support::responses::ResponsesRequest;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::sse;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::TestCodexHarness;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn direct_patch_review_assesses_writable_file_without_writes() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let builder = test_codex().with_model("gpt-5.5").with_config(|config| {
        super::configure_scenario_catalog(config);
        config.approvals_reviewer = ApprovalsReviewer::AutoReview;
        config.permissions.approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
        // A sandbox would allow the live patch without review.
        config.permissions.windows_sandbox_mode =
            Some(codex_config::types::WindowsSandboxModeToml::Unelevated);
        config
            .permissions
            .set_permission_profile(PermissionProfile::workspace_write())
            .expect("workspace-write permissions");
    });
    let harness = TestCodexHarness::with_auto_env_builder(builder).await?;
    let test = harness.test();
    harness
        .write_file("guardian-skipped.txt", "before\n")
        .await?;
    let assessment = json!({
        "outcome": "allow",
        "risk_level": "low",
        "user_authorization": "high",
        "rationale": "The writable patch is authorized."
    });
    responses::mount_sse_once(
        harness.server(),
        sse(vec![
            ev_assistant_message("review", &assessment.to_string()),
            ev_completed("review"),
        ]),
    )
    .await;

    test.codex
        .inject_response_items(vec![serde_json::from_value(json!({
            "type": "custom_tool_call",
            "call_id": "patch-skip",
            "name": "apply_patch",
            "input": "*** Begin Patch\n*** Update File: guardian-skipped.txt\n@@\n-before\n+after\n*** End Patch",
        }))?])
        .await?;
    assert_eq!(
        test.codex
            .guardian_review(&CancellationToken::new())
            .await?,
        GuardianReviewResult {
            call_id: "patch-skip".to_owned(),
            assessment: serde_json::from_value(assessment)?,
        }
    );
    assert_eq!(
        harness.read_file_text("guardian-skipped.txt").await?,
        "before\n"
    );
    test.codex.shutdown_and_wait().await?;
    Ok(())
}

#[test_case::test_case(AskForApproval::Never; "never")]
#[test_case::test_case(AskForApproval::Granular(codex_protocol::protocol::GranularApprovalConfig {
    sandbox_approval: true,
    rules: true,
    mcp_elicitations: true,
    request_permissions: false,
    skill_approval: true,
}); "granular")]
#[tokio::test]
async fn direct_permissions_review_returns_assessment_when_policy_disallows_requests(
    policy: AskForApproval,
) -> Result<()> {
    skip_if_no_network!(Ok(()));
    let builder = test_codex()
        .with_model("gpt-5.5")
        .with_config(move |config| {
            super::configure_scenario_catalog(config);
            config
                .features
                .enable(Feature::RequestPermissionsTool)
                .expect("enable permission requests");
            config.permissions.approval_policy = Constrained::allow_any(policy);
        });
    let harness = TestCodexHarness::with_auto_env_builder(builder).await?;
    let test = harness.test();
    let assessment = json!({
        "outcome": "allow",
        "risk_level": "low",
        "user_authorization": "high",
        "rationale": "The network request is authorized."
    });
    responses::mount_sse_once(
        harness.server(),
        sse(vec![
            ev_assistant_message("review", &assessment.to_string()),
            ev_completed("review"),
        ]),
    )
    .await;
    test.codex
        .inject_response_items(vec![serde_json::from_value(json!({
            "type": "function_call", "name": "request_permissions", "call_id": "permissions-review",
            "arguments": r#"{"permissions":{"network":{"enabled":true}}}"#,
        }))?])
        .await?;
    assert_eq!(
        test.codex
            .guardian_review(&CancellationToken::new())
            .await?,
        GuardianReviewResult {
            call_id: "permissions-review".into(),
            assessment: serde_json::from_value(assessment)?,
        }
    );
    test.codex.shutdown_and_wait().await?;
    Ok(())
}

pub(in crate::suite) fn model_input(
    request: &ResponsesRequest,
    parent: ThreadId,
) -> Result<String> {
    let mut input = request.input();
    for item in &mut input {
        item.as_object_mut().expect("response item").remove("id");
        if let Some(metadata) = item
            .get_mut("internal_chat_message_metadata_passthrough")
            .and_then(serde_json::Value::as_object_mut)
        {
            metadata.remove("turn_id");
            metadata.remove("create_time");
        }
    }
    Ok(serde_json::to_string_pretty(&input)?.replace(
        &format!("Reviewed Codex session id: {parent}"),
        "Reviewed Codex session id: <PARENT>",
    ))
}

#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn direct_exec_review_applies_hook_rewrite_before_policy_check() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let builder = test_codex()
        .with_model("gpt-5.5")
        .with_pre_build_hook(|home| {
            super::super::hooks::write_updating_pre_tool_use_hook(
                home,
                "^Bash$",
                &json!({"command": "git init guardian-rewritten"}),
            )
            .expect("write rewriting hook");
        })
        .with_config(|config| {
            super::configure_scenario_catalog(config);
            core_test_support::hooks::trust_discovered_hooks(config);
            config.approvals_reviewer = ApprovalsReviewer::AutoReview;
            config.permissions.approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
            let rules = config.codex_home.join("rules");
            std::fs::create_dir_all(&rules).expect("create rules directory");
            std::fs::write(
                rules.join("direct-review.rules"),
                r#"prefix_rule(pattern=["git", "init", "guardian-original"], decision="allow")
prefix_rule(pattern=["git", "init", "guardian-rewritten"], decision="prompt")
"#,
            )
            .expect("write command policies");
        });
    // These hook fixtures use the local Python interpreter and filesystem.
    let harness = TestCodexHarness::with_builder(builder).await?;
    let test = harness.test();

    let assessment = json!({
        "risk_level": "low",
        "user_authorization": "high",
        "outcome": "allow",
        "rationale": "The rewritten command is authorized."
    });
    let mock = responses::mount_sse_once(
        harness.server(),
        sse(vec![
            ev_assistant_message("review", &assessment.to_string()),
            ev_completed("review"),
        ]),
    )
    .await;

    test.codex
        .inject_response_items(vec![serde_json::from_value(json!({
            "type": "function_call", "call_id": "hook-review", "name": "exec_command",
            "arguments": r#"{"cmd":"git init guardian-original"}"#,
        }))?])
        .await?;
    assert_eq!(
        test.codex
            .guardian_review(&CancellationToken::new())
            .await?,
        GuardianReviewResult {
            call_id: "hook-review".to_owned(),
            assessment: serde_json::from_value(assessment)?,
        }
    );

    let mut snapshot = context_snapshot::format_request_history_snapshot(
        "Direct Guardian review applies the PreToolUse rewrite before checking approval policy.",
        &[mock.single_request()],
        &ContextSnapshotOptions::default().rewrite_known_segments(),
    );
    let environment_id = &test.executor_environment().selection().environment_id;
    snapshot = snapshot
        .replace(
            &format!("\"environment_id\": {environment_id:?}"),
            "\"environment_id\": \"<ENVIRONMENT>\"",
        )
        .replace(
            &format!("environment {environment_id:?}"),
            "environment \"<ENVIRONMENT>\"",
        );
    for (pattern, replacement) in [
        (r#"(?m)^(\s*"cwd": )"[^"]*""#, "$1\"<CWD>\""),
        (
            r#""command": \[\s*(?:"[^"]*",\s*)*"(git init guardian-(?:original|rewritten))"\s*\]"#,
            "\"command\": [\"<SHELL>\", \"$1\"]",
        ),
        (
            r"(?m)^(\s*\[\d+\] )`.*` requires approval by policy$",
            "$1<COMMAND> requires approval by policy",
        ),
    ] {
        snapshot = regex_lite::Regex::new(pattern)?
            .replace_all(&snapshot, replacement)
            .into_owned();
    }
    insta::assert_snapshot!("guardian_direct_pre_tool_use_rewrite", snapshot);

    assert!(!harness.path_exists("guardian-original").await?);
    assert!(!harness.path_exists("guardian-rewritten").await?);

    test.codex.shutdown_and_wait().await?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn direct_exec_review_honors_hook_denial_before_assessment() -> Result<()> {
    use codex_protocol::protocol::EventMsg;
    use core_test_support::wait_for_event;

    skip_if_no_network!(Ok(()));

    let reason = "blocked by direct-review hook";
    let builder = test_codex()
        .with_model("gpt-5.5")
        .with_pre_build_hook(move |home| {
            super::super::hooks::write_pre_tool_use_hook(
                home,
                Some("^Bash$"),
                "json_deny_with_context",
                reason,
            )
            .expect("write blocking hook");
        })
        .with_config(|config| {
            super::configure_scenario_catalog(config);
            core_test_support::hooks::trust_discovered_hooks(config);
            let rules = config.codex_home.join("rules");
            std::fs::create_dir_all(&rules).expect("create rules directory");
            std::fs::write(
                rules.join("direct-review.rules"),
                "prefix_rule(pattern=[\"git\", \"init\", \"guardian-original\"], decision=\"allow\")\n",
            )
            .expect("write allow rule");
        });
    // These hook fixtures use the local Python interpreter and filesystem.
    let harness = TestCodexHarness::with_builder(builder).await?;
    let test = harness.test();

    let mock =
        responses::mount_sse_once(harness.server(), sse(vec![ev_completed("unexpected")])).await;

    test.codex
        .inject_response_items(vec![serde_json::from_value(json!({
            "type": "function_call", "call_id": "hook-review", "name": "exec_command",
            "arguments": r#"{"cmd":"git init guardian-original"}"#,
        }))?])
        .await?;
    let error = test
        .codex
        .guardian_review(&CancellationToken::new())
        .await
        .expect_err("hook must reject the call before Guardian can assess it");
    assert!(matches!(
        error.details(),
        codex_protocol::error::CodexErrorDetails::InvalidRequest(_)
    ));
    assert!(mock.requests().is_empty());

    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::HookStarted(_))
    })
    .await;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::HookCompleted(_))
    })
    .await;

    test.codex.flush_rollout().await?;
    let history = test.codex.load_history(/*include_archived*/ false).await?;
    assert!(history.items.iter().any(|entry| {
        matches!(entry, RolloutItem::ResponseItem(item)
            if matches!(item.item.as_ref(), ResponseItem::Message { role, content, .. }
                if role == "developer" && content.as_slice() == &[input_text(reason)]))
    }));

    assert!(!harness.path_exists("guardian-original").await?);

    test.codex.shutdown_and_wait().await?;
    Ok(())
}
