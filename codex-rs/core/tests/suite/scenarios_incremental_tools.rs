//! Request-history coverage for incremental tools on Responses and Responses Lite.

use anyhow::Result;
use codex_core::ForkSnapshot;
use codex_core::McpServersDisabled;
use codex_core::compact::AutoCompactionDisabled;
use codex_features::Feature;
use codex_protocol::openai_models::ToolMode;
use core_test_support::context_snapshot;
use core_test_support::context_snapshot::ContextSnapshotOptions;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use core_test_support::skip_if_wine_exec;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_mcp_server;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::super::rmcp_client::remote_aware_environment_id;
use super::super::rmcp_client::remote_aware_stdio_server_bin;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prediction_fork_keeps_inherited_tools() -> Result<()> {
    skip_if_wine_exec!(
        Ok(()),
        "requires a Windows test_stdio_server in the Wine-exec environment"
    );
    skip_if_no_network!(Ok(()));
    let server = responses::start_mock_server().await;
    let mcp = serde_json::from_value(json!({
        "command": remote_aware_stdio_server_bin()?,
        "environment_id": remote_aware_environment_id(),
        "enabled_tools": ["echo"],
        "required": true,
    }))?;
    let mut test = test_codex()
        .with_model_info_override("gpt-5.4", |model| {
            model.use_responses_lite = true;
            model.supports_search_tool = false;
            model.tool_mode = Some(ToolMode::CodeModeOnly);
        })
        .with_config(move |config| {
            config.workspace_roots = vec![config.cwd.clone()];
            config.base_instructions = Some("Use the available tools to help the user.".into());
            config
                .features
                .enable(Feature::IncrementalTools)
                .expect("enable incremental tools");
            config
                .mcp_servers
                .set(std::collections::HashMap::from([(
                    "echo_service".into(),
                    mcp,
                )]))
                .expect("set MCP server");
        })
        .build_with_auto_env(&server)
        .await?;
    wait_for_mcp_server(&test.codex, "echo_service").await?;
    let mock = responses::mount_sse_sequence(
        &server,
        ["parent", "prediction", "continued-parent"]
            .into_iter()
            .map(|id| responses::sse(vec![responses::ev_completed(id)]))
            .collect(),
    )
    .await;
    test.submit_text_turn("Start with the echo service.")
        .await?;

    let parent = std::sync::Arc::clone(&test.codex);
    parent.flush_rollout().await?;
    let (mut options, settings) = test
        .thread_manager
        .fork_options_from_parent(test.session_configured.thread_id)
        .await?;
    // Match experimentalPredictionMode with experimentalDisablePredictionMcp enabled.
    options.config.ephemeral = true;
    options
        .config
        .features
        .enable(Feature::ReasoningEffortOverride)?;
    options.thread_extension_init.insert(AutoCompactionDisabled);
    options.thread_extension_init.insert(McpServersDisabled);
    let child = test
        .thread_manager
        .fork_legacy_thread(
            ForkSnapshot::Interrupted,
            options,
            parent.rollout_path().expect("parent rollout"),
        )
        .await?;
    child.thread.restore_thread_settings(settings).await?;
    test.codex = child.thread;
    test.submit_text_turn("Predict the next message.").await?;
    test.codex = parent;
    test.submit_text_turn("Continue using the echo service.")
        .await?;

    let requests = mock.requests();
    let parent = requests[0].body_json();
    let child = requests[1].body_json();
    let prefix = parent["input"].as_array().expect("parent input");
    let exec = prefix
        .iter()
        .filter(|item| item["type"] == "additional_tools")
        .find_map(|item| responses::namespace_child_tool(item, "functions", "exec"))
        .expect("parent declares functions.exec");
    assert!(
        exec["description"]
            .as_str()
            .expect("exec description")
            .contains("mcp__echo_service__echo"),
        "the inherited functions.exec must describe the MCP helper",
    );
    assert_eq!(
        &child["input"].as_array().expect("child input")[..prefix.len()],
        prefix
    );
    insta::assert_snapshot!(
        "prediction_fork_inherited_tools",
        context_snapshot::format_request_history_snapshot(
            "Request 1 starts the parent thread. Request 2 is the prediction child's only turn. Request 3 continues the original parent and verifies that it still has its MCP tools.",
            &requests,
            &ContextSnapshotOptions::default()
                .rewrite_known_segments()
                .include_request_settings(),
        )
    );
    Ok(())
}
