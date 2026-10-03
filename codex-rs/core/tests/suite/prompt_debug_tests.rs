use std::sync::Arc;

use anyhow::Result;
use codex_core::build_prompt_input;
use codex_core::build_prompt_request_from_thread;
use codex_core::config::ConfigBuilder;
use codex_core::config::ConfigOverrides;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_features::Feature;
use codex_home::CodexHomeUserInstructionsProvider;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ToolMode;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::received_responses_requests;
use core_test_support::responses::start_mock_server;
use core_test_support::responses::strip_metadata;
use core_test_support::responses::strip_response_item_id;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

const TEST_INSTRUCTIONS: &str = "Global test instructions";

#[tokio::test]
async fn build_prompt_input_includes_context_and_user_message() -> Result<()> {
    let codex_home = TempDir::new()?;
    let cwd = TempDir::new()?;
    std::fs::write(codex_home.path().join("AGENTS.md"), TEST_INSTRUCTIONS)?;
    let config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .harness_overrides(ConfigOverrides {
            cwd: Some(cwd.path().to_path_buf()),
            codex_self_exe: Some(std::env::current_exe()?),
            ..ConfigOverrides::default()
        })
        .build()
        .await?;
    let user_instructions_provider = Arc::new(CodexHomeUserInstructionsProvider::new(
        config.codex_home.clone(),
    ));
    let input = build_prompt_input(
        config,
        vec![UserInput::Text {
            text: "hello from debug prompt".to_string(),
            text_elements: Vec::new(),
        }],
        /*state_db*/ None,
        Arc::new(ExtensionRegistryBuilder::new().build()),
        user_instructions_provider.clone(),
    )
    .await?;

    let expected_user_message = ResponseItem::Message {
        status: None,
        encrypted_content: None,
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: "hello from debug prompt".to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    };
    assert_eq!(
        input
            .last()
            .cloned()
            .map(strip_metadata)
            .map(strip_response_item_id),
        Some(expected_user_message)
    );
    assert!(input.iter().any(|item| {
        let ResponseItem::Message { content, .. } = item else {
            return false;
        };

        content.iter().any(|content_item| {
            let (ContentItem::InputText { text } | ContentItem::OutputText { text, .. }) =
                content_item
            else {
                return false;
            };
            text.contains(TEST_INSTRUCTIONS)
        })
    }));

    skip_if_no_network!(Ok(()));
    let server = start_mock_server().await;
    for (use_responses_lite, incremental_tools) in
        [(false, false), (false, true), (true, false), (true, true)]
    {
        let test = test_codex()
            .with_model("gpt-5.5")
            .with_model_info_override("gpt-5.5", move |model| {
                model.use_responses_lite = use_responses_lite;
                model.tool_mode = Some(ToolMode::Direct);
            })
            .with_user_instructions_provider(user_instructions_provider.clone())
            .with_config(move |config| {
                config.base_instructions = Some("Audit base instructions".to_string());
                config.model_reasoning_effort = Some(ReasoningEffort::High);
                config
                    .features
                    .set_enabled(Feature::IncrementalTools, incremental_tools)
                    .expect("test config should allow incremental tools override");
            })
            .build_with_auto_env(&server)
            .await?;
        let schema = json!({
            "type": "object",
            "properties": {"answer": {"type": "string"}},
            "required": ["answer"],
            "additionalProperties": false,
        });
        let audit = build_prompt_request_from_thread(
            &test.codex,
            vec![UserInput::Text {
                text: "hello from debug prompt".to_string(),
                text_elements: Vec::new(),
            }],
            Some(schema.clone()),
        )
        .await?;
        let audit_json = serde_json::to_value(audit)?;
        assert_eq!(
            audit_json["base_instructions"]["text"],
            "Audit base instructions"
        );
        let request = &audit_json["request"];
        assert_eq!(request["model"], "gpt-5.5");
        assert_eq!(request["reasoning"]["effort"], "high");
        assert_eq!(
            request["text"]["format"],
            json!({
                "type": "json_schema",
                "name": "codex_output_schema",
                "strict": true,
                "schema": schema,
            })
        );
        let request_input = request["input"].as_array().expect("request input");
        let user_message = request_input.last().expect("user message");
        assert_eq!(user_message["role"], "user");
        assert_eq!(
            user_message["content"],
            json!([{"type": "input_text", "text": "hello from debug prompt"}])
        );
        assert!(request_input[..request_input.len() - 1].iter().any(|item| {
            item["role"] == "user"
                && item["content"].as_array().is_some_and(|content| {
                    content.iter().any(|part| {
                        part["text"]
                            .as_str()
                            .is_some_and(|text| text.contains(TEST_INSTRUCTIONS))
                    })
                })
        }));
        let tools = if use_responses_lite {
            assert!(request.get("instructions").is_none());
            assert!(request.get("tools").is_none());
            let (tools_index, instructions_index) = if incremental_tools { (1, 0) } else { (0, 1) };
            assert_eq!(
                request_input
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| item["type"] == "additional_tools")
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>(),
                vec![tools_index]
            );
            assert_eq!(request_input[tools_index]["role"], "developer");
            assert_eq!(request_input[instructions_index]["role"], "developer");
            assert_eq!(
                request_input[instructions_index]["content"][0]["text"],
                "Audit base instructions"
            );
            &request_input[tools_index]["tools"]
        } else {
            assert_eq!(request["instructions"], "Audit base instructions");
            assert!(
                request_input
                    .iter()
                    .all(|item| item["type"] != "additional_tools")
            );
            &request["tools"]
        };
        let exec_command = tools
            .as_array()
            .expect("full tool definitions")
            .iter()
            .flat_map(|tool| {
                tool["tools"]
                    .as_array()
                    .map_or_else(|| std::slice::from_ref(tool), Vec::as_slice)
            })
            .find(|tool| tool["name"] == "exec_command")
            .expect("exec_command definition");
        assert_eq!(exec_command["type"], "function");
        assert!(
            !exec_command["description"]
                .as_str()
                .expect("tool description")
                .is_empty()
        );
        assert_eq!(
            exec_command["parameters"]["properties"]["cmd"]["type"],
            "string"
        );
        assert_eq!(exec_command["parameters"]["required"], json!(["cmd"]));
        test.codex.shutdown_and_wait().await?;
    }
    assert!(received_responses_requests(&server).await.is_empty());
    Ok(())
}
