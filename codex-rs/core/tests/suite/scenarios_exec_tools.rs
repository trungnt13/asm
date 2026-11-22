//! Coverage for Code Mode catalog refresh on resume.

use anyhow::Result;
use codex_features::Feature;
use codex_protocol::openai_models::ToolMode;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test_case::test_case(None, true; "bundled_helpers")]
#[test_case::test_case(Some("Run JavaScript."), false; "helper_free_override")]
#[test_case::test_case(Some("Run JavaScript. {{ image_helper }}"), true; "image_helper_override")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_resume_refreshes_only_when_helper_signatures_change(
    description_override: Option<&'static str>,
    includes_image_helper: bool,
) -> Result<()> {
    skip_if_no_network!(Ok(()));
    let server = responses::start_mock_server().await;
    let mock = responses::mount_sse_sequence(
        &server,
        ["initial", "refreshed", "steady"]
            .map(responses::sse_completed)
            .to_vec(),
    )
    .await;
    let builder = |unified_image_budget: bool| {
        test_codex()
            .with_config(move |config| {
                super::configure_scenario_catalog(config);
                for feature in [Feature::IncrementalTools, Feature::IncrementalExecTools] {
                    config
                        .features
                        .enable(feature)
                        .expect("enable catalog updates");
                }
                config
                    .features
                    .set_enabled(Feature::UnifiedImageBudget, unified_image_budget)
                    .expect("set image budget mode");
            })
            .with_model_info_override("catalog-initial", move |model| {
                model.use_responses_lite = true;
                model.tool_mode = Some(ToolMode::CodeModeOnly);
                model.model_messages = description_override.map(|description| {
                    serde_json::from_value(json!({
                        "tools": {"code_mode": {"exec": {"description": description}}},
                    }))
                    .expect("exec description override")
                });
            })
            .with_model("catalog-initial")
    };

    let original = builder(false).build_with_auto_env(&server).await?;
    original.submit_text_turn("Publish exec.").await?;
    let resumed = builder(true)
        .restart_with_auto_env(&server, &original)
        .await?;
    resumed
        .submit_text_turn("Refresh exec after resuming.")
        .await?;
    resumed
        .submit_text_turn("Keep the refreshed catalog.")
        .await?;

    let requests = mock.requests();
    assert_eq!(requests.len(), 3);
    let expected_counts = if includes_image_helper {
        [1, 2, 2]
    } else {
        [1, 1, 1]
    };
    for (request, expected_count) in requests.iter().zip(expected_counts) {
        let input = request.input();
        let declarations = input
            .iter()
            .filter(|item| item["type"] == "additional_tools")
            .filter_map(|item| responses::namespace_child_tool(item, "functions", "exec"))
            .collect::<Vec<_>>();
        assert_eq!(declarations.len(), expected_count);
        let description = declarations.last().expect("exec declaration")["description"]
            .as_str()
            .expect("exec declaration should have a string description");
        if description_override.is_some() {
            assert!(description.starts_with("Run JavaScript."));
        }
        if includes_image_helper {
            let expected_signature = if expected_count == 1 {
                "image(imageUrlOrItem: string | { image_url: string; detail?"
            } else {
                "image(imageUrlOrItem: string | { image_url: string } | ImageContent)"
            };
            assert!(description.contains(expected_signature));
        } else {
            assert!(!description.contains("image(imageUrlOrItem:"));
        }
    }
    Ok(())
}
