//! Multi-turn Astra scenarios snapshot the model-visible request history of shipped features.

#[path = "scenarios_bundled_app_hooks.rs"]
mod bundled_app_hooks;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn executor_discovery_v2_refreshes_inventory_and_reuses_it_across_turns() -> anyhow::Result<()>
{
    super::executor_discovery_v2::check_acquisition(/*enabled*/ true, "0.164.0").await
}

#[test_case::test_case(DiscoveryFailure::None; "success")]
#[test_case::test_case(DiscoveryFailure::Server; "server_failure")]
#[tokio::test]
async fn executor_discovery_v2_prepares_inventory_while_sampling(
    failure: DiscoveryFailure,
) -> anyhow::Result<()> {
    super::executor_discovery_v2::check_discovery_readiness(failure).await
}

#[path = "scenarios_turn_input_agent_message.rs"]
mod turn_input_agent_message;

#[path = "scenarios_multi_environment_mcp.rs"]
mod multi_environment_mcp;

use super::executor_discovery_v2::DiscoveryFailure;
use super::image_rollout::RecordingFileAttachmentStore;
use codex_attachment_store::AttachmentStoreErrorKind;
use codex_attachment_store::UploadContent;
use codex_core::RequestCountry;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use base64::Engine;
use codex_analytics::CompactionTrigger;
use codex_config::ConfigLayerEntry;
use codex_config::ConfigLayerSource;
use codex_config::ConfigLayerStack;
use codex_context_fragments::ContextualUserFragment;
use codex_core::StartThreadOptions;
use codex_core::TurnInput;
use codex_core::TurnInputRequest;
use codex_core::config::Config;
use codex_exec_server::CreateDirectoryOptions;
use codex_extension_api::ExtensionRegistry;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_features::Feature;
use codex_history::RolloutItem;
use codex_login::CodexAuth;
use codex_models_manager::bundled_models_response;
use codex_protocol::dynamic_tools::DynamicToolCallOutputContentItem;
use codex_protocol::dynamic_tools::DynamicToolFunctionSpec;
use codex_protocol::dynamic_tools::DynamicToolResponse;
use codex_protocol::dynamic_tools::DynamicToolSpec;
use codex_protocol::items::TurnItem;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ImageDetail;
use codex_protocol::models::ImageReference;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ToolMessage;
use codex_protocol::openai_models::ToolMode;
use codex_protocol::protocol::AdditionalContextEntry;
use codex_protocol::protocol::AdditionalContextKind;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::TurnSettingsUpdate;
use codex_protocol::protocol::TurnSettingsUpdateOutcome;
use codex_protocol::user_input::UserInput;
use codex_skills_extension::SkillsExtensionConfig;
use codex_skills_extension::install;
use core_test_support::context_snapshot;
use core_test_support::context_snapshot::ContextSnapshotOptions;
use core_test_support::context_snapshot::SnapshotEntry;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_completed_with_tokens;
use core_test_support::responses::ev_custom_tool_call;
use core_test_support::responses::ev_function_call_with_namespace;
use core_test_support::responses::ev_message_item_added;
use core_test_support::responses::ev_output_text_delta;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::skip_if_wine_exec;
use core_test_support::stdio_server_bin;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::TestCodexBuilder;
use core_test_support::test_codex::TestEnv;
use core_test_support::test_codex::configure_scenario_catalog;
use core_test_support::test_codex::test_codex;
use core_test_support::test_codex::test_env;
use core_test_support::wait_for_event;
use core_test_support::wait_for_event_match;
use core_test_support::wait_for_mcp_server;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::oneshot;

const ONE_PIXEL_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";

#[path = "scenarios_injected_agent_mail.rs"]
mod injected_agent_mail;

#[path = "scenarios_dynamic_tools.rs"]
mod dynamic_tools;

#[path = "scenarios_audio_file_tests.rs"]
mod audio_file;

#[path = "scenarios_restored_history_prewarm.rs"]
mod restored_history_prewarm;

#[path = "scenarios_retained_tool_outputs.rs"]
mod retained_tool_outputs;

#[path = "scenarios_exec_tools.rs"]
mod exec_tools;
#[path = "scenarios_incremental_tools.rs"]
mod incremental_tools;
#[path = "scenarios_incremental_tools_resume.rs"]
mod incremental_tools_resume;

#[path = "scenarios_model_instructions.rs"]
mod model_instructions;

#[path = "scenarios/catalog_settings.rs"]
mod catalog_settings;

#[path = "scenarios_catalog_approval_instructions_tests.rs"]
mod catalog_approval_instructions;

#[path = "scenarios_catalog_apps_instructions_tests.rs"]
mod catalog_apps_instructions;

#[path = "scenarios_managed_human_approval_tests.rs"]
mod managed_human_approval;

#[path = "scenarios_context_management_tools.rs"]
mod context_management_tools;

#[path = "scenarios_code_mode_settled_helpers_tests.rs"]
mod code_mode_settled_helpers;

#[path = "scenarios_command_sessions_tests.rs"]
mod command_sessions;

#[path = "scenarios_strict_3p_cache.rs"]
mod strict_3p_cache;

#[path = "scenarios_agent_message_board.rs"]
mod agent_message_board;

#[path = "scenarios_mailbox_preemption_tests.rs"]
mod mailbox_preemption;

#[path = "scenarios_agent_eviction_tests.rs"]
mod agent_eviction;

#[path = "scenarios_reserved_spawn.rs"]
mod reserved_spawn;

#[path = "host_turn_admission.rs"]
mod host_turn_admission;

#[path = "scenarios_partial_answers.rs"]
mod partial_answers;

#[path = "scenarios_guardian_async_elastic.rs"]
mod guardian_async_elastic;

#[path = "scenarios_guardian_sender_context.rs"]
mod guardian_sender_context;

#[path = "scenarios_guardian_direct_review.rs"]
mod guardian_direct_review;

#[path = "scenarios_guardian_direct_mcp.rs"]
mod guardian_direct_mcp;

#[path = "scenarios_guardian_extra_policy.rs"]
mod guardian_extra_policy;

#[path = "scenarios_guardian_reconsideration.rs"]
mod guardian_reconsideration;

#[path = "scenarios_guardian_orchestrator_connector.rs"]
mod guardian_orchestrator_connector;

#[path = "scenarios_guardian_conversation_history_tests.rs"]
mod guardian_conversation_history;

#[path = "scenarios_indirect_namespace_prefixes.rs"]
mod indirect_namespace_prefixes;

#[path = "scenarios_mcp_resource_messages.rs"]
mod mcp_resource_messages;

#[path = "scenarios_mcp_ema.rs"]
mod mcp_ema;

#[path = "scenarios_mcp_sharing.rs"]
mod mcp_sharing;

#[path = "scenarios_guardian_agent_messages_tests.rs"]
mod guardian_agent_messages;

#[cfg(not(target_os = "windows"))]
#[path = "scenarios_guardian_handoff.rs"]
mod guardian_handoff;

#[path = "scenarios_guardian_heartbeat.rs"]
mod guardian_heartbeat;

#[path = "scenarios_preparation.rs"]
mod preparation;

#[path = "scenarios_content_filter.rs"]
mod content_filter;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "scenarios_credential_masking.rs"]
mod credential_masking;

#[path = "scenarios_provider_capabilities.rs"]
mod provider_capabilities;

#[path = "scenarios_web_search.rs"]
mod web_search;

#[path = "scenarios_shared_instructions.rs"]
mod shared_instructions;

#[path = "scenarios_mxc.rs"]
mod mxc;

#[path = "scenarios_tools_namespace_budget.rs"]
mod tools_namespace_budget;

#[path = "scenarios_skill_catalog_dedup.rs"]
mod skill_catalog_dedup;

#[path = "scenarios_frozen_cloud_skills.rs"]
mod frozen_cloud_skills;

#[path = "scenarios_compaction_tests.rs"]
mod compaction;

#[path = "scenarios_statusline_authoring_tests.rs"]
mod statusline_authoring;

#[path = "scenarios_budget_context.rs"]
mod budget_context;

#[path = "scenarios_world_state_prefix_tests.rs"]
mod world_state_prefix;

fn skills_extensions() -> Arc<ExtensionRegistry<Config>> {
    let mut extensions = ExtensionRegistryBuilder::<Config>::new();
    install(&mut extensions, |config: &Config| SkillsExtensionConfig {
        include_instructions: config.include_skill_instructions,
        catalog_deltas_enabled: config.features.enabled(Feature::SkillCatalogDeltas),
        omit_descriptions: config
            .features
            .enabled(codex_features::Feature::OmitSkillDescriptions),
        max_context_tokens: config.skill_max_context_tokens,
        bundled_skills_enabled: config.bundled_skills_enabled(),
        cloud_skill_enabled: config.cloud_skill_enabled,
        shadow_selection_enabled: config.features.enabled(Feature::SkillSearch),
    });
    Arc::new(extensions.build())
}

fn write_skill(path: &Path, name: &str, description: &str, body: &str) -> Result<PathBuf> {
    fs::create_dir_all(path)?;
    let skill = path.join("SKILL.md");
    fs::write(
        &skill,
        format!("---\nname: {name}\ndescription: {description}\n---\n\n{body}\n"),
    )?;
    Ok(fs::canonicalize(skill)?)
}

struct ScenarioSkills {
    outline: PathBuf,
    agenda: PathBuf,
    summarize: PathBuf,
    final_check: PathBuf,
}

fn write_scenario_capabilities(home: &TempDir) -> Result<ScenarioSkills> {
    fs::write(
        home.path().join("config.toml"),
        "[features]\nplugins = true\n\n[skills.bundled]\nenabled = false\n\n[plugins.\"calendar@test\"]\nenabled = true\n\n[plugins.\"notes@test\"]\nenabled = true\n",
    )?;
    let plugin_cache = home.path().join("plugins/cache/test");
    for (name, description) in [
        ("calendar", "Prepare a team schedule"),
        ("notes", "Summarize meeting notes"),
    ] {
        let manifest = plugin_cache
            .join(name)
            .join("local/.codex-plugin/plugin.json");
        fs::create_dir_all(manifest.parent().expect("manifest parent"))?;
        fs::write(
            manifest,
            json!({ "name": name, "description": description }).to_string(),
        )?;
    }

    Ok(ScenarioSkills {
        outline: write_skill(
            &home.path().join("skills/outline"),
            "outline",
            "Draft a project outline",
            "List the goals and owners.",
        )?,
        agenda: write_skill(
            &plugin_cache.join("calendar/local/skills/agenda"),
            "agenda",
            "Plan a team agenda",
            "List meetings with dates and attendees.",
        )?,
        summarize: write_skill(
            &plugin_cache.join("notes/local/skills/summarize"),
            "summarize",
            "Summarize team notes",
            "Extract decisions and action items.",
        )?,
        final_check: write_skill(
            &home.path().join("skills/final-check"),
            "final-check",
            "Review a final brief",
            "Check that the brief has an owner for every action.",
        )?,
    })
}

fn text(value: &str) -> UserInput {
    UserInput::Text {
        text: value.to_string(),
        text_elements: Vec::new(),
    }
}

fn selected_skill(name: &str, path: &Path) -> UserInput {
    UserInput::Skill {
        name: name.to_string(),
        path: path.to_path_buf(),
    }
}

fn plugin(name: &str) -> UserInput {
    UserInput::Mention {
        name: name.to_string(),
        path: format!("plugin://{name}@test"),
    }
}

fn configure_catalog_plugin_scenario(config: &mut Config) {
    configure_scenario_catalog(config);
    config.workspace_roots = vec![config.cwd.clone()];
    let stack = &config.config_layer_stack;
    let fixture_config = stack.effective_config();
    // Keep the fixture's plugin and App policy while avoiding discovery of the real
    // $HOME/.agents/skills, which a User layer adds alongside the fixture's skills.
    config.config_layer_stack = ConfigLayerStack::new(
        vec![ConfigLayerEntry::new(
            ConfigLayerSource::System {
                file: config.codex_home.join("config.toml"),
            },
            fixture_config,
        )],
        stack.requirements().clone(),
        stack.requirements_toml().clone(),
    )
    .expect("isolated plugin scenario config")
    .with_cloud_config_binding(stack.cloud_config_binding().cloned());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admitted_prompt_id_reaches_responses() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let server = start_mock_server().await;
    let mut builder = test_codex().with_config(|config| {
        configure_scenario_catalog(config);
        config.include_environment_context = false;
    });
    let test = builder.build_with_auto_env(&server).await?;
    let mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![ev_completed("first")]),
            sse(vec![ev_completed("second")]),
        ],
    )
    .await;
    let uuid = uuid::Uuid::new_v4();
    for id in [Some(uuid), None] {
        let mut request = TurnInputRequest::user_input(vec![text("Repeat this input.")]);
        request.user_item_uuid = id;
        test.codex.start_or_steer_turn(request).await?;
        wait_for_event(&test.codex, |event| {
            matches!(event, EventMsg::TurnComplete(_))
        })
        .await;
    }

    let requests = mock.requests();
    assert_eq!(requests.len(), 2);
    let reserved = codex_protocol::ResponseItemId::for_admitted_user_input(uuid);
    for (index, request) in requests.iter().enumerate() {
        let input = request.input();
        let prompts: Vec<_> = input
            .iter()
            .filter(|item| {
                item["role"] == "user" && item["content"][0]["text"] == "Repeat this input."
            })
            .collect();
        assert_eq!(prompts.len(), index + 1);
        assert_eq!(prompts[0]["id"], reserved.as_str());
        if let Some(legacy) = prompts.get(1) {
            let id = legacy["id"].as_str().expect("legacy message id");
            assert!(id.starts_with("msg_") && !id.starts_with("msg_input_"));
        }
    }
    insta::assert_snapshot!(context_snapshot::format_request_history_snapshot(
        "An admitted prompt and an ordinary prompt with identical text reach Responses on successive turns.",
        &requests,
        &ContextSnapshotOptions::default().rewrite_known_segments(),
    ));
    Ok(())
}

#[test_case::test_case(false; "messages")]
#[test_case::test_case(true; "tool_outputs")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn additional_context_tool_outputs(tool_outputs: bool) -> Result<()> {
    skip_if_no_network!(Ok(()));
    let server = start_mock_server().await;
    let test = test_codex()
        .with_config(move |config| {
            configure_scenario_catalog(config);
            config.include_environment_context = false;
            if !tool_outputs {
                config
                    .features
                    .disable(Feature::UntrustedAsToolOutput)
                    .expect("test config should allow feature update");
            }
        })
        .build_with_auto_env(&server)
        .await?;
    let mock = mount_sse_sequence(
        &server,
        (0..4)
            .map(|index| sse(vec![ev_completed(&format!("response-{index}"))]))
            .collect(),
    )
    .await;

    for (prompt, name, value) in [
        (
            "Summarize the selected document.",
            Some("browser"),
            "The launch is on Monday.",
        ),
        (
            "The same document is now open in the editor.",
            Some("editor"),
            "The launch is on Monday.",
        ),
        (
            "Check the date again.",
            Some("editor"),
            "The launch is on Monday.",
        ),
        (
            "Summarize the updated document.",
            None,
            "The launch is on Tuesday.",
        ),
    ] {
        test.codex
            .start_or_steer_turn(
                TurnInputRequest::user_input(vec![text(prompt)]).with_additional_context(
                    std::collections::BTreeMap::from([
                        (
                            "selection".to_owned(),
                            AdditionalContextEntry {
                                value: value.to_owned(),
                                kind: AdditionalContextKind::Untrusted,
                                name: name.map(str::to_owned),
                            },
                        ),
                        (
                            "application".to_owned(),
                            AdditionalContextEntry::new(
                                "Keep the answer brief.".to_owned(),
                                AdditionalContextKind::Application,
                            ),
                        ),
                    ]),
                ),
            )
            .await?;
        wait_for_event(&test.codex, |event| {
            matches!(event, EventMsg::TurnComplete(_))
        })
        .await;
    }
    let requests = mock.requests();
    assert_eq!(requests.len(), 4);
    if tool_outputs {
        assert_eq!(
            requests
                .iter()
                .map(|request| request.inputs_of_type("function_call_output").len())
                .collect::<Vec<_>>(),
            vec![1, 2, 2, 3]
        );
        assert_eq!(requests[3].message_input_texts("user").len(), 4);
        let input = requests[0].input();
        let context_index = input
            .iter()
            .position(|item| item["type"] == "function_call_output")
            .expect("context output");
        let user_index = input
            .iter()
            .position(|item| item["role"] == "user")
            .expect("user message");
        assert!(context_index < user_index);
    } else {
        assert!(
            requests
                .iter()
                .all(|request| request.inputs_of_type("function_call_output").is_empty())
        );
        assert_eq!(
            requests
                .iter()
                .map(|request| request.message_input_texts("user").len())
                .collect::<Vec<_>>(),
            vec![2, 3, 4, 6]
        );
    }
    insta::assert_snapshot!(
        if tool_outputs {
            "additional_context_tool_outputs"
        } else {
            "additional_context_legacy_messages"
        },
        context_snapshot::format_request_history_snapshot(
            "Named context changes are appended before the user; unchanged values are deduplicated.",
            &requests,
            &ContextSnapshotOptions::default().rewrite_known_segments(),
        )
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn realtime_attachment_replacement_preserves_handoff_context() -> Result<()> {
    let requests = super::realtime_attachment::attachment_replacement_scenario().await?;
    insta::assert_snapshot!(
        "realtime_attachment_replacement",
        context_snapshot::format_request_history_snapshot(
            "A replacement fences an older pending connection and stale stops; only the selected call starts a Codex handoff.",
            &requests,
            &ContextSnapshotOptions::default().rewrite_known_segments(),
        )
    );
    Ok(())
}

/// Expiring files change request copies while canonical history retains the original references.
/// A shared barrier requires the three distinct file IDs to resolve concurrently on each request.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unavailable_images_are_rechecked_without_rewriting_history() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let server = start_mock_server().await;
    let image_store = Arc::new(RecordingFileAttachmentStore {
        resolve_barrier: Some(Arc::new(tokio::sync::Barrier::new(/*n*/ 3))),
        ..Default::default()
    });
    let test = test_codex()
        .with_model("gpt-6-astra")
        .with_image_store(image_store.clone())
        .with_config(configure_scenario_catalog)
        .build_with_auto_env(&server)
        .await?;
    let mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_assistant_message("first", "The images are available."),
                ev_completed("first"),
            ]),
            sse(vec![
                ev_assistant_message("second", "Some images are no longer available."),
                ev_completed("second"),
            ]),
        ],
    )
    .await;
    let file_ids = [
        "file_expiring",
        "file_available",
        "file_expiring",
        "file_backend_error",
    ];
    let mut input = vec![text("Compare these images.")];
    input.extend(file_ids.map(|file_id| UserInput::Image {
        image: ImageReference::File {
            file_id: file_id.to_string(),
        },
        detail: Some(ImageDetail::High),
    }));
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(input))
        .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    assert_eq!(
        *image_store.resolves.lock().expect("resolves lock"),
        HashMap::from([
            ("file_expiring".to_string(), 1),
            ("file_available".to_string(), 1),
            ("file_backend_error".to_string(), 1),
        ])
    );

    image_store
        .resolve_errors
        .lock()
        .expect("resolve errors lock")
        .extend([
            (
                "file_expiring".to_string(),
                AttachmentStoreErrorKind::NotFound,
            ),
            (
                "file_backend_error".to_string(),
                AttachmentStoreErrorKind::Backend,
            ),
        ]);
    test.submit_text_turn("Compare them again.").await?;
    assert_eq!(
        *image_store.resolves.lock().expect("resolves lock"),
        HashMap::from([
            ("file_expiring".to_string(), 2),
            ("file_available".to_string(), 2),
            ("file_backend_error".to_string(), 2),
        ])
    );
    let requests = mock.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[0]
            .body_json()
            .to_string()
            .contains("file_expiring")
    );
    let unavailable_parts = requests[1]
        .input()
        .into_iter()
        .filter_map(|item| {
            item["internal_chat_message_metadata_passthrough"]["content_item_kinds"]
                .as_array()
                .cloned()
        })
        .flatten()
        .filter(|kind| kind == "images.unavailable")
        .count();
    assert_eq!(unavailable_parts, 2);
    assert!(requests[1].body_contains_text("[image unavailable]"));
    assert!(
        !requests[1]
            .body_json()
            .to_string()
            .contains("file_expiring")
    );
    assert!(
        requests[1]
            .body_json()
            .to_string()
            .contains("file_backend_error")
    );
    assert!(
        requests[1]
            .body_json()
            .to_string()
            .contains("file_available")
    );

    let recorded_images = test
        .codex
        .load_history(/*include_archived*/ false)
        .await?
        .items
        .into_iter()
        .filter_map(|item| match item {
            RolloutItem::ResponseItem(envelope) => match envelope.into_item() {
                ResponseItem::Message { content, .. } => Some(content),
                _ => None,
            },
            _ => None,
        })
        .flatten()
        .filter(|content| matches!(content, ContentItem::InputImage { .. }))
        .collect::<Vec<_>>();
    assert_eq!(
        recorded_images,
        file_ids.map(|file_id| ContentItem::InputImage {
            image: ImageReference::File {
                file_id: file_id.to_string()
            },
            detail: Some(ImageDetail::High),
        })
    );
    insta::assert_snapshot!(context_snapshot::format_request_history_snapshot(
        "Available file references pass through. A later request replaces missing files, preserves references on backend errors, rechecks each distinct ID once, and keeps canonical history intact.",
        &requests,
        &ContextSnapshotOptions::default().rewrite_known_segments(),
    ));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn code_mode_forwards_mixed_mcp_content() -> Result<()> {
    skip_if_no_network!(Ok(()));
    skip_if_wine_exec!(Ok(()), "requires a Windows test_stdio_server binary");
    let server = start_mock_server().await;
    let (_test, response) = super::code_mode::run_code_mode_turn_with_rmcp_model(
        &server,
        "Show the caption and image from the MCP image tool.",
        r#"const result = await tools.mcp__rmcp__image_scenario({scenario: "text_then_image", caption: "sample"});
content(result.content);"#,
        "gpt-5.5",
    )
    .await?;
    let output = response.single_request().custom_tool_call_output("call-1");
    assert_eq!(output["output"][1]["text"], "sample");
    assert_eq!(output["output"][2]["type"], "input_image");
    insta::assert_snapshot!(context_snapshot::format_request_history_snapshot(
        "Code Mode forwards MCP text and image blocks to the model in their original order.",
        &core_test_support::responses::received_responses_requests(&server).await,
        &ContextSnapshotOptions::default().rewrite_known_segments(),
    ));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn code_mode_mcp_schema_limits_preserve_explicit_overrides() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let requests = super::code_mode::mcp_schema_max_bytes_scenario().await?;
    insta::assert_snapshot!(
        "code_mode_mcp_schema_limits",
        context_snapshot::format_request_history_snapshot(
            "Code Mode exposes explicit MCP schema limits in its prompt and runtime tool catalog.",
            &requests,
            &ContextSnapshotOptions::default()
                .rewrite_known_segments()
                .include_request_settings(),
        )
    );
    Ok(())
}

#[derive(Clone, Copy)]
enum ExecutorPluginDisablement {
    ThreadSelection,
    ConfigRefresh,
}

#[test_case::test_case(false; "simplified_selection_hints_off")]
#[test_case::test_case(true; "simplified_selection_hints_on")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn astra_refreshes_plugin_tools_and_skills_in_an_existing_thread(
    simplified_selection_hints: bool,
) -> Result<()> {
    skip_if_no_network!(Ok(()));
    core_test_support::skip_if_remote!(Ok(()), "plugin and MCP fixtures use host-local paths");

    let server = start_mock_server().await;
    let home = Arc::new(TempDir::new()?);
    let config = format!(
        "[features]\nplugins = true\nsimplified_selection_hints = {simplified_selection_hints}\n\n[skills.bundled]\nenabled = false\n"
    );
    fs::write(home.path().join("config.toml"), &config)?;
    let lookup = r#"text(ALL_TOOLS.filter(({ name }) => name === "mcp__notes__echo").map(({ name }) => name));"#;
    let mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("before-refresh"),
                ev_custom_tool_call("before-lookup", "exec", lookup),
                ev_completed("before-refresh"),
            ]),
            sse(vec![
                ev_assistant_message("missing-tool", "The Notes tool is not installed yet."),
                ev_completed("missing-tool-response"),
            ]),
            sse(vec![
                ev_response_created("after-refresh"),
                ev_custom_tool_call("after-lookup", "exec", lookup),
                ev_completed("after-refresh"),
            ]),
            sse(vec![
                ev_response_created("first-echo"),
                ev_custom_tool_call(
                    "first-echo-call",
                    "exec",
                    r#"const result = await tools.mcp__notes__echo({ message: "Mira owns the kickoff" }); text(result.structuredContent?.echo);"#,
                ),
                ev_completed("first-echo"),
            ]),
            sse(vec![
                ev_assistant_message("owner", "The Notes plugin confirms Mira owns the kickoff."),
                ev_completed("owner-response"),
            ]),
            sse(vec![
                ev_response_created("follow-up"),
                ev_custom_tool_call(
                    "second-echo-call",
                    "exec",
                    r#"const result = await tools.mcp__notes__echo({ message: "The kickoff is Friday" }); text(result.structuredContent?.echo);"#,
                ),
                ev_completed("follow-up"),
            ]),
            sse(vec![
                ev_assistant_message("deadline", "The Notes plugin confirms the kickoff is Friday."),
                ev_completed("deadline-response"),
            ]),
        ],
    )
    .await;
    let mut builder = test_codex()
        .with_model("gpt-6-astra")
        .with_home(Arc::clone(&home))
        .with_auth(CodexAuth::create_dummy_chatgpt_auth_for_testing())
        .with_extensions(skills_extensions())
        .with_config(configure_scenario_catalog);
    let test = builder.build(&server).await?;

    test.submit_turn("Check whether the Notes plugin echo tool is available.")
        .await?;

    let plugin_root = home.path().join("plugins/cache/test/notes/local");
    fs::create_dir_all(plugin_root.join(".codex-plugin"))?;
    fs::write(
        plugin_root.join(".codex-plugin/plugin.json"),
        json!({ "name": "notes", "description": "Look up and summarize team notes" }).to_string(),
    )?;
    fs::write(
        plugin_root.join(".mcp.json"),
        json!({ "mcpServers": { "notes": { "command": stdio_server_bin()?, "cwd": "." } } })
            .to_string(),
    )?;
    let skill = write_skill(
        &plugin_root.join("skills/summarize"),
        "summarize",
        "Summarize team notes",
        "State the owner and date from the notes.",
    )?;
    fs::write(
        home.path().join("config.toml"),
        format!("{config}\n[plugins.\"notes@test\"]\nenabled = true\n"),
    )?;
    test.codex.submit(Op::ReloadUserConfig).await?;
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![
            text("I installed Notes. Use $notes:summarize and the Notes tool to check that Mira owns the kickoff."),
            plugin("notes"),
            selected_skill("notes:summarize", &skill),
        ]))
        .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    test.submit_text_turn("Use the Notes tool again to check that the kickoff is Friday.")
        .await?;

    let snapshot_name = if simplified_selection_hints {
        "astra_plugin_refresh_simplified_selection_hints_on"
    } else {
        "astra_plugin_refresh_simplified_selection_hints_off"
    };
    insta::assert_snapshot!(
        snapshot_name,
        context_snapshot::format_request_history_snapshot(
            "Astra checks for Notes, refreshes its installed plugin without restarting, and uses the new skill and Code Mode MCP tool across turns.",
            &mock.requests(),
            &ContextSnapshotOptions::default().include_request_settings(),
        )
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guardian_nested_elicitation_request_history() -> Result<()> {
    skip_if_no_network!(Ok(()));
    skip_if_wine_exec!(Ok(()), "the MCP fixture requires a host Python interpreter");

    let requests = super::guardian_mcp_elicitation::nested_elicitation_request_history().await?;
    assert_eq!(requests.len(), 5);
    let snapshot = context_snapshot::format_request_history_snapshot(
        "A Node REPL invocation waits for two independent Guardian reviews. The follow-up review uses a transcript delta and receives the same live enclosing call.",
        &requests,
        &ContextSnapshotOptions::default().rewrite_known_segments(),
    );
    let snapshot = regex_lite::Regex::new(r"Wall time: [0-9]+(?:\.[0-9]+)? seconds")?
        .replace_all(&snapshot, "Wall time: <DURATION> seconds");
    let snapshot = regex_lite::Regex::new(
        r#"(The active permission profile for environment )"(?:local|remote)""#,
    )?
    .replace_all(&snapshot, "$1\"<ENVIRONMENT>\"");
    insta::assert_snapshot!("guardian_nested_elicitation", snapshot);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subagent_waits_for_its_inherited_environment_configuration() -> Result<()> {
    skip_if_no_network!(Ok(()));

    use super::remote_env::spawn_tests::PendingSpawnCase;
    use super::remote_env::spawn_tests::pending_subagent_scenario;

    let requests =
        pending_subagent_scenario(PendingSpawnCase::TurnSettings, configure_scenario_catalog)
            .await?;
    let labels = [
        "parent delegates while the environment is starting",
        "parent waits for the worker",
        "child sees the inherited starting environment",
        "child receives the ready result and updated environment",
        "parent receives the child result",
    ];
    assert_eq!(requests.len(), labels.len());
    let entries = requests
        .iter()
        .zip(labels)
        .map(|(request, label)| SnapshotEntry::body(request).labeled(label))
        .collect::<Vec<_>>();
    let snapshot = context_snapshot::format_context_snapshot(
        "A parent spawns a child before the selected environment is configured. The child sees it starting, waits, and receives the ready result after an ordinary parent turn-settings update.",
        &entries,
        &ContextSnapshotOptions::default(),
    );
    // Keep every request and its environment context; collapse unrelated guidance repeated per window.
    let mut snapshot = snapshot;
    for (pattern, replacement) in [
        (
            r"(?s)<permissions instructions>.*?</permissions instructions>",
            "<PERMISSIONS_INSTRUCTIONS>",
        ),
        (
            r"(?s)<environments_instructions>.*?</environments_instructions>",
            "<ENVIRONMENTS_INSTRUCTIONS>",
        ),
        (
            r"(?ms)(^\d+:message/developer:\n    You are (?:`/root`, the primary agent|an agent) in a team of agents[^\n]+)\n.*?(\n^\d+:message/developer:)",
            "${1}\n    <STANDARD_MULTI_AGENT_INSTRUCTIONS>${2}",
        ),
        (
            r#"(environment_id"\s*:\s*"|<environment id=")(?:local|remote)"#,
            "${1}<ENVIRONMENT>",
        ),
    ] {
        snapshot = regex_lite::Regex::new(pattern)?
            .replace_all(&snapshot, replacement)
            .into_owned();
    }
    // Windows guidance appears when the executor is ready, including under Wine.
    let snapshot_name =
        if core_test_support::test_target_os() == core_test_support::TestTargetOs::Windows {
            "subagent_inherits_pending_environment_windows"
        } else {
            "subagent_inherits_pending_environment"
        };
    insta::assert_snapshot!(snapshot_name, snapshot);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attached_side_chat_preserves_model_context_on_cold_resume() -> Result<()> {
    use codex_context_fragments::SideConversationPolicy;
    use codex_history::InitialHistory;
    use codex_history::ResumedHistory;
    use codex_protocol::protocol::ThreadHistoryMode;
    use codex_thread_store::ForkBoundary;
    use codex_thread_store::LoadThreadHistoryParams;
    use codex_thread_store::PrepareForkParams;

    skip_if_no_network!(Ok(()));
    let server = start_mock_server().await;
    let mock = mount_sse_sequence(
        &server,
        [
            ("parent", "The migration is scheduled for Thursday."),
            ("side", "Stage a rollback before migration."),
            ("resumed", "Keep the rollback script close."),
        ]
        .into_iter()
        .map(|(id, reply)| sse(vec![ev_assistant_message(id, reply), ev_completed(id)]))
        .collect(),
    )
    .await;
    let mut test = test_codex()
        .with_model("gpt-6-astra")
        .with_history_mode(ThreadHistoryMode::Paginated)
        .with_config(|config| {
            configure_scenario_catalog(config);
            config.workspace_roots = vec![config.cwd.clone()];
            config.developer_instructions = Some("Use plain language.".to_string());
        })
        .build_with_auto_env(&server)
        .await?;
    test.submit_text_turn("Plan a database migration.").await?;
    test.codex.flush_rollout().await?;

    let prepared = test
        .thread_store
        .prepare_fork(PrepareForkParams {
            thread_id: test.session_configured.thread_id,
            boundary: ForkBoundary::Latest,
        })
        .await?;
    let mut options = test.start_thread_options().await;
    options.attached_thread = true;
    options.config.developer_instructions = Some(
        SideConversationPolicy::Attached
            .instructions(options.config.developer_instructions.as_deref()),
    );
    let mut resume_options = StartThreadOptions::new(options.config.clone());
    resume_options.environments = options.environments.clone();
    let child = test
        .thread_manager
        .fork_prepared_thread(
            options,
            prepared,
            codex_thread_store::InheritedHistoryVisibility::Full,
        )
        .await?;
    child
        .thread
        .inject_response_items(SideConversationPolicy::Attached.fork_items())
        .await?;
    test.codex.shutdown_and_wait().await?;
    test.codex = child.thread;
    test.submit_text_turn("What is the safest rollback plan?")
        .await?;

    // Unload the child and resume from disk with the same policy as the TUI.
    test.codex.shutdown_and_wait().await?;
    test.thread_manager.remove_thread(&child.thread_id).await;
    let saved = test
        .thread_store
        .load_latest_model_context(LoadThreadHistoryParams {
            thread_id: child.thread_id,
            include_archived: false,
        })
        .await?;
    resume_options.initial_history = InitialHistory::Resumed(ResumedHistory {
        history_revision: saved.revision,
        conversation_id: saved.thread_id,
        history: Arc::new(saved.items),
        rollout_path: test.codex.rollout_path(),
    });
    resume_options.client_mcp_extensions = test.codex.client_mcp_extensions();
    test.codex = test
        .thread_manager
        .start_thread(resume_options)
        .await?
        .thread;
    test.submit_text_turn("What did you recommend before I closed the chat?")
        .await?;

    let requests = mock.requests();
    assert_eq!(requests.len(), 3);
    insta::assert_snapshot!(context_snapshot::format_request_history_snapshot(
        "A side chat receives the parent turn as reference, adds its own turn, and resumes from disk with the policy and prior answer intact.",
        &requests,
        &ContextSnapshotOptions::default(),
    ));
    Ok(())
}

fn compaction_chunk(
    gate: Option<oneshot::Receiver<()>>,
    events: Vec<serde_json::Value>,
) -> StreamingSseChunk {
    StreamingSseChunk {
        gate,
        body: sse(events),
    }
}

fn compaction_output(value: &str) -> serde_json::Value {
    json!({"type": "response.output_item.done", "item": {
        "type": "compaction", "encrypted_content": value
    }})
}

async fn async_compaction_test(
    builder: TestCodexBuilder,
    responses_uri: &str,
    features: Vec<Feature>,
    environment: TestEnv,
    configure: impl Fn(&mut Config) + Send + Sync + 'static,
) -> Result<(wiremock::MockServer, TestCodex)> {
    let config_server = start_mock_server().await;
    let base_url = format!("{responses_uri}/v1");
    let test = builder
        .with_auth(CodexAuth::create_dummy_chatgpt_auth_for_testing())
        .with_config(move |config| {
            configure_scenario_catalog(config);
            config.model_provider.base_url = Some(base_url);
            config.model_context_window = Some(100_000);
            for feature in features {
                config
                    .features
                    .enable(feature)
                    .expect("enable scenario feature");
            }
            config
                .features
                .enable(Feature::AsyncCompaction)
                .expect("enable async compaction");
            config
                .features
                .disable(Feature::EnableRequestCompression)
                .expect("capture raw requests");
            configure(config);
        })
        .with_model("gpt-6-astra")
        .build_with_environment(&config_server, environment)
        .await?;
    Ok((config_server, test))
}

/// Pending image checks for optional compaction do not delay foreground turn completion.
/// Its request still replaces files that expire after the sampling request.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn async_compaction_image_checks_do_not_block_turn_completion() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let barrier = Arc::new(tokio::sync::Barrier::new(/*n*/ 2));
    let image_store = Arc::new(RecordingFileAttachmentStore {
        resolve_barrier: Some(Arc::clone(&barrier)),
        ..Default::default()
    });
    let (release_sampling, sampling_gate) = oneshot::channel();
    let (streaming, _completions) = start_streaming_sse_server(vec![
        vec![compaction_chunk(
            Some(sampling_gate),
            vec![
                ev_assistant_message("answer", "I examined the image."),
                ev_completed_with_tokens("answer", /*total_tokens*/ 70_000),
            ],
        )],
        vec![compaction_chunk(
            /*gate*/ None,
            vec![
                compaction_output("IMAGE_CHECKPOINT"),
                ev_completed_with_tokens("compact", /*total_tokens*/ 3_000),
            ],
        )],
    ])
    .await;
    let config_server = start_mock_server().await;
    let base_url = format!("{}/v1", streaming.uri());
    let test = test_codex()
        .with_auth(CodexAuth::create_dummy_chatgpt_auth_for_testing())
        .with_image_store(image_store.clone())
        .with_request_country(RequestCountry::from_iso_code("DE")?)
        .with_config(move |config| {
            configure_scenario_catalog(config);
            config.model_provider.base_url = Some(base_url);
            config.model_context_window = Some(100_000);
            config.features.enable(Feature::AsyncCompaction).unwrap();
            config
                .features
                .disable(Feature::EnableRequestCompression)
                .unwrap();
        })
        .with_model("gpt-6-astra")
        .build_with_auto_env(&config_server)
        .await?;
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![
            text("Describe this image."),
            UserInput::Image {
                image: ImageReference::File {
                    file_id: "file_expiring".to_string(),
                },
                detail: Some(ImageDetail::High),
            },
        ]))
        .await?;
    let turn_id = wait_for_event_match(&test.codex, |event| match event {
        EventMsg::TurnStarted(event) => Some(event.turn_id.clone()),
        _ => None,
    })
    .await;
    // Release the sampling lookup, then expire the file before its response finishes.
    barrier.wait().await;
    streaming.wait_for_request_count(/*count*/ 1).await;
    let (reply, outcome) = oneshot::channel();
    test.codex
        .submit(Op::TurnSettings {
            turn_id,
            update: TurnSettingsUpdate {
                request_country: Some(RequestCountry::from_iso_code("US").unwrap()),
                ..Default::default()
            },
            reply,
        })
        .await?;
    assert_eq!(outcome.await?, TurnSettingsUpdateOutcome::Applied);
    image_store
        .resolve_errors
        .lock()
        .expect("resolve errors lock")
        .insert(
            "file_expiring".to_string(),
            AttachmentStoreErrorKind::NotFound,
        );
    release_sampling.send(()).expect("finish sampling");
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    assert_eq!(streaming.requests().await.len(), 1);

    test.codex
        .update_thread_settings(codex_protocol::protocol::ThreadSettingsOverrides {
            request_country: Some(RequestCountry::from_iso_code("JP").unwrap()),
            ..Default::default()
        })
        .await?;
    // The turn completed without releasing the optional compaction lookup.
    barrier.wait().await;
    tokio::time::timeout(
        Duration::from_secs(/*secs*/ 20),
        test.codex.wait_for_background_compaction_for_test(),
    )
    .await?;
    let requests = streaming
        .requests()
        .await
        .iter()
        .map(|body| serde_json::from_slice::<serde_json::Value>(body))
        .collect::<serde_json::Result<Vec<_>>>()?;
    assert_eq!(requests.len(), 2);
    // Compaction keeps the sampling step's country despite live and future settings updates.
    assert_eq!(
        requests
            .iter()
            .map(|request| request.get("request_country"))
            .collect::<Vec<_>>(),
        vec![
            Some(&serde_json::json!("DE")),
            Some(&serde_json::json!("DE"))
        ]
    );
    assert_eq!(
        *image_store.resolves.lock().expect("resolves lock"),
        HashMap::from([("file_expiring".to_string(), 2)])
    );
    assert!(requests[0]["input"].to_string().contains("file_expiring"));
    assert!(
        requests[1]["input"]
            .to_string()
            .contains("[image unavailable]")
    );
    assert!(!requests[1]["input"].to_string().contains("file_expiring"));
    test.codex.shutdown_and_wait().await?;
    Ok(())
}

#[derive(Clone, Copy)]
enum LiveSuffix {
    User,
    Tool,
    RetainedTool,
    OverBudgetRetainedTool,
}

#[derive(Clone, Copy)]
enum ExecCatalogRefresh {
    Hybrid,
    Metadata,
}

#[test_case::test_case(ExecCatalogRefresh::Hybrid; "hybrid mode")]
#[test_case::test_case(ExecCatalogRefresh::Metadata; "exec metadata refresh")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn astra_async_compaction_preserves_exec_catalog(change: ExecCatalogRefresh) -> Result<()> {
    use codex_models_manager::manager::RefreshStrategy;
    use core_test_support::responses::mount_models_once_with_etag;

    skip_if_no_network!(Ok(()));
    let (release_background, background_gate) = oneshot::channel();
    let (release_foreground, foreground_gate) = oneshot::channel();
    let (streaming, _completions) = start_streaming_sse_server(vec![
        vec![compaction_chunk(
            /*gate*/ None,
            vec![
                ev_assistant_message("draft", "I drafted the launch notes."),
                ev_completed_with_tokens("draft-response", /*total_tokens*/ 70_000),
            ],
        )],
        vec![compaction_chunk(
            Some(background_gate),
            vec![
                compaction_output("ASYNC_LAUNCH_CHECKPOINT"),
                ev_completed_with_tokens("compact-response", /*total_tokens*/ 3_000),
            ],
        )],
        vec![
            compaction_chunk(
                /*gate*/ None,
                vec![
                    ev_response_created("team-response"),
                    ev_assistant_message("team-check", "I will check who is available."),
                ],
            ),
            compaction_chunk(
                Some(foreground_gate),
                vec![
                    ev_function_call_with_namespace("team", "collaboration", "list_agents", "{}"),
                    ev_completed_with_tokens("team-response", /*total_tokens*/ 75_000),
                ],
            ),
        ],
        vec![compaction_chunk(
            /*gate*/ None,
            vec![
                ev_assistant_message("answer", "I can finish the notes myself."),
                ev_completed_with_tokens("answer-response", /*total_tokens*/ 3_000),
            ],
        )],
    ])
    .await;
    let model_server = wiremock::MockServer::start().await;
    let mut catalog = bundled_models_response()?;
    let model = catalog
        .models
        .iter_mut()
        .find(|model| model.slug == "gpt-6-astra")
        .expect("bundled Astra model");
    model.tool_mode = Some(ToolMode::CodeModeOnly);
    model.supports_image_detail_original = false;
    mount_models_once_with_etag(&model_server, catalog.clone(), "\"exec-mode-v1\"").await;
    let model_catalog_url = format!("{}/models", model_server.uri());
    let (_config_server, test) = async_compaction_test(
        test_codex(),
        streaming.uri(),
        vec![
            Feature::IncrementalTools,
            Feature::IncrementalExecTools,
            Feature::ViewImage,
        ],
        test_env().await?,
        move |config| {
            config
                .features
                .disable(Feature::UnifiedImageBudget)
                .expect("use the model's image detail capability");
            config.model_catalog = None;
            config.model_provider.model_catalog_url = Some(model_catalog_url.clone().into());
            config.code_mode.disable_in_process_fallback = true;
        },
    )
    .await?;
    test.codex.ensure_rollout_materialized().await;

    test.submit_text_turn("Draft the launch notes.").await?;
    let timeout = Duration::from_secs(/*secs*/ 20);
    tokio::time::timeout(timeout, streaming.wait_for_request_count(/*count*/ 2)).await?;
    // Refresh the same model while the prepared result is still pending.
    let model = catalog
        .models
        .iter_mut()
        .find(|model| model.slug == "gpt-6-astra")
        .expect("bundled Astra model");
    match change {
        ExecCatalogRefresh::Hybrid => model.tool_mode = Some(ToolMode::CodeMode),
        ExecCatalogRefresh::Metadata => {
            model.supports_image_detail_original = true;
            let messages = model
                .model_messages
                .get_or_insert_default()
                .tools
                .get_or_insert_default()
                .code_mode
                .get_or_insert_default();
            messages.exec.get_or_insert_default().description =
                Some("Previous exec instructions.".to_string());
            messages.mcp_typescript_preamble = Some("type PreviousResult = string;".to_string());
        }
    }
    mount_models_once_with_etag(&model_server, catalog.clone(), "\"exec-mode-v2\"").await;
    test.thread_manager
        .get_models_manager()
        .list_models(
            RefreshStrategy::Online,
            codex_core::test_support::default_http_client_factory(),
        )
        .await;
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![text(
            "Check who can finish the notes.",
        )]))
        .await?;
    tokio::time::timeout(timeout, streaming.wait_for_request_count(/*count*/ 3)).await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::ItemCompleted(event)
        if matches!(&event.item, TurnItem::AgentMessage(message) if message.id == "team-check"))
    })
    .await;
    release_background
        .send(())
        .expect("release background compaction");
    // Keep foreground work gated until the prepared result is available for optional adoption.
    tokio::time::timeout(
        timeout,
        test.codex.wait_for_background_compaction_for_test(),
    )
    .await?;
    release_foreground
        .send(())
        .expect("release foreground work");
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    tokio::time::timeout(
        timeout,
        test.codex.wait_for_background_compaction_for_test(),
    )
    .await?;
    let requests = streaming
        .requests()
        .await
        .iter()
        .map(|body| serde_json::from_slice::<serde_json::Value>(body))
        .collect::<serde_json::Result<Vec<_>>>()?;
    assert_eq!(requests.len(), 4);
    let history = test
        .codex
        .load_history(/*include_archived*/ false)
        .await?
        .items;
    let input = requests[3]["input"].as_array().expect("request input");
    let compaction = input
        .iter()
        .position(|item| item["encrypted_content"] == "ASYNC_LAUNCH_CHECKPOINT")
        .expect("prepared summary was adopted");
    let user = input
        .iter()
        .position(|item| {
            item["role"] == "user"
                && item["content"][0]["text"] == "Check who can finish the notes."
        })
        .expect("retained user message");
    let assistant = input
        .iter()
        .position(|item| {
            item["role"] == "assistant"
                && item["content"][0]["text"] == "I will check who is available."
        })
        .expect("retained assistant message");
    let call = input
        .iter()
        .position(|item| item["type"] == "function_call" && item["call_id"] == "team")
        .expect("retained tool call");
    let output = input
        .iter()
        .position(|item| item["type"] == "function_call_output" && item["call_id"] == "team")
        .expect("retained tool output");
    assert!(user < assistant && assistant < call && call < output);
    let [before_adoption, after_adoption] = [&requests[2], &requests[3]].map(|request| {
        request["input"]
            .as_array()
            .expect("request input")
            .iter()
            .filter(|item| item["type"] == "additional_tools")
            .map(|item| item["tools"].clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(after_adoption, before_adoption);
    let (exec_positions, exec_declarations): (Vec<_>, Vec<_>) = input
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            core_test_support::responses::namespace_child_tool(item, "functions", "exec")
                .map(|declaration| (index, declaration))
        })
        .unzip();
    let (adoption_index, adopted) = history
        .windows(2)
        .enumerate()
        .find_map(|(index, pair)| match pair {
            [RolloutItem::Compacted(_), RolloutItem::WorldState(baseline)] if baseline.full => {
                Some((index, &baseline.state["top_level_tools"]))
            }
            _ => None,
        })
        .expect("adopted full checkpoint");
    let changed_hashes: &[&str] = match change {
        ExecCatalogRefresh::Hybrid => &["/functions.exec"],
        ExecCatalogRefresh::Metadata => &["/tools/functions.exec", "/exec/Tool: tools.view_image"],
    };
    for path in changed_hashes {
        let live = history[..adoption_index]
            .iter()
            .rev()
            .find_map(|item| match item {
                RolloutItem::WorldState(patch) if !patch.full => {
                    patch.state.get("top_level_tools")?.pointer(path)
                }
                _ => None,
            })
            .and_then(serde_json::Value::as_str)
            .expect("live exec update before adoption");
        assert_eq!(
            adopted.pointer(path).and_then(serde_json::Value::as_str),
            Some(live),
            "{path}"
        );
    }
    if matches!(change, ExecCatalogRefresh::Metadata) {
        let initial_exec = requests[0]["input"]
            .as_array()
            .expect("initial input")
            .iter()
            .find_map(|item| {
                core_test_support::responses::namespace_child_tool(item, "functions", "exec")
            })
            .expect("initial exec declaration");
        assert_eq!(exec_declarations, vec![initial_exec]);
        assert!(exec_positions[0] < compaction && compaction < user);
        let is_view_image_update = |item: &&serde_json::Value| {
            item["role"] == "developer"
                && item["content"][0]["text"]
                    .as_str()
                    .is_some_and(|text| text.lines().any(|line| line == "### `view_image`"))
        };
        let live_update = requests[2]["input"]
            .as_array()
            .expect("input before adoption")
            .iter()
            .find(is_view_image_update)
            .expect("view_image redefinition during background compaction");
        assert!(
            live_update["content"][0]["text"]
                .as_str()
                .expect("view_image update text")
                .contains("detail?: \"high\" | \"original\"")
        );
        assert_eq!(
            input
                .iter()
                .filter(is_view_image_update)
                .collect::<Vec<_>>(),
            vec![live_update]
        );
        assert!(input[compaction + 1..].contains(live_update));
        return Ok(());
    }
    assert_eq!(exec_declarations.len(), 2);
    assert_ne!(exec_declarations[0], exec_declarations[1]);
    assert!(
        exec_positions[0] < compaction
            && compaction < exec_positions[1]
            && exec_positions[1] < user
    );
    Ok(())
}

#[derive(PartialEq)]
enum PendingCompactionOutcome {
    Absent,
    Success,
    SuccessOverBudget,
    StaleSuccess,
    Failure,
    ReadyFailure,
    Interrupted,
}

#[test_case::test_case(PendingCompactionOutcome::Success, CompactionTrigger::Auto; "auto uses pending result")]
#[test_case::test_case(PendingCompactionOutcome::Failure, CompactionTrigger::Auto; "auto falls back after failure")]
#[test_case::test_case(PendingCompactionOutcome::ReadyFailure, CompactionTrigger::Auto; "auto falls back after failure before checkpoint")]
#[test_case::test_case(PendingCompactionOutcome::StaleSuccess, CompactionTrigger::Auto; "auto falls back after account changes")]
#[test_case::test_case(PendingCompactionOutcome::Interrupted, CompactionTrigger::Auto; "auto can be interrupted")]
#[test_case::test_case(PendingCompactionOutcome::Absent, CompactionTrigger::Manual; "manual without background")]
#[test_case::test_case(PendingCompactionOutcome::Success, CompactionTrigger::Manual; "manual uses pending result")]
#[test_case::test_case(PendingCompactionOutcome::Failure, CompactionTrigger::Manual; "manual falls back after failure")]
#[test_case::test_case(PendingCompactionOutcome::StaleSuccess, CompactionTrigger::Manual; "manual falls back after account changes")]
#[test_case::test_case(PendingCompactionOutcome::Interrupted, CompactionTrigger::Manual; "manual can be interrupted")]
#[test_case::test_case(PendingCompactionOutcome::SuccessOverBudget, CompactionTrigger::Auto; "pending adoption still over budget")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn async_compaction_reuses_pending_work(
    outcome: PendingCompactionOutcome,
    trigger: CompactionTrigger,
) -> Result<()> {
    use super::rollout_budget::HostAccounting;

    const BACKGROUND_TOKENS: i64 = 3_001;

    skip_if_no_network!(Ok(()));
    let over_budget = match outcome {
        PendingCompactionOutcome::SuccessOverBudget => true,
        _ => false,
    };
    let retained_output = format!("LATE_OUTPUT {}", "x ".repeat(12_000));
    let manual = matches!(trigger, CompactionTrigger::Manual);
    let has_background = outcome != PendingCompactionOutcome::Absent;
    let seed_tokens = if over_budget {
        2_000
    } else if has_background {
        70_000
    } else {
        10_000
    };
    let valid_output = !matches!(
        outcome,
        PendingCompactionOutcome::Failure | PendingCompactionOutcome::ReadyFailure
    );
    let succeeds = !matches!(
        outcome,
        PendingCompactionOutcome::Absent
            | PendingCompactionOutcome::SuccessOverBudget
            | PendingCompactionOutcome::Failure
            | PendingCompactionOutcome::ReadyFailure
            | PendingCompactionOutcome::StaleSuccess
    );
    let response = |id, event, tokens| vec![event, ev_completed_with_tokens(id, tokens)];
    let immediate = |events| vec![compaction_chunk(/*gate*/ None, events)];
    let (release_background, background_gate) = oneshot::channel();
    let mut responses = vec![immediate(response(
        "seed",
        ev_assistant_message("seed", "seed"),
        seed_tokens,
    ))];
    if has_background {
        responses.push(vec![compaction_chunk(
            Some(background_gate),
            response(
                "background",
                if valid_output {
                    compaction_output("ASYNC")
                } else {
                    ev_assistant_message("invalid", "missing compaction output")
                },
                BACKGROUND_TOKENS,
            ),
        )]);
    }
    if !manual {
        responses.push(immediate(response(
            "second",
            ev_function_call_with_namespace("team", "collaboration", "list_agents", "{}"),
            /*tokens*/ if over_budget { 9_000 } else { 91_000 },
        )));
    }
    if !succeeds {
        responses.push(immediate(response(
            "sync",
            compaction_output("SYNC"),
            /*tokens*/ 3_000,
        )));
    }
    responses.push(immediate(response(
        "continuation",
        ev_assistant_message("continuation", "continuation"),
        /*tokens*/ if over_budget { 10 } else { 3_000 },
    )));
    let (streaming, mut completions) = start_streaming_sse_server(responses).await;
    let accounting =
        (outcome == PendingCompactionOutcome::Success).then(|| Arc::new(HostAccounting::default()));
    let mut builder = test_codex();
    let observe_attempts = outcome == PendingCompactionOutcome::Success && !manual;
    let timeout = Duration::from_secs(/*secs*/ 20);
    let post_started = Arc::new(tokio::sync::Notify::new());
    let (release_post, wait_for_release) = std::sync::mpsc::channel();
    let (record, recorded) = std::sync::mpsc::channel();
    // The harness owns the loopback MCP server. Hook inputs and waits do not
    // depend on a shell or filesystem shared with the tool environment.
    let _hook_server = if observe_attempts {
        let server = start_mock_server().await;
        core_test_support::hooks::mount_hook_mcp(
            &server,
            "compaction-observer",
            &["pre", "post"],
            {
                let started = Arc::clone(&post_started);
                let release = std::sync::Mutex::new(wait_for_release);
                move |params| {
                    record.send(params.clone()).expect("record hook call");
                    if observe_attempts
                        && params["name"] == "post"
                        && params["arguments"]["kind"] == "background_compaction"
                    {
                        started.notify_one();
                        release
                            .lock()
                            .expect("post-hook gate")
                            .recv_timeout(timeout)
                            .expect("release background post-hook");
                    }
                    json!({"content": []})
                }
            },
        )
        .await;
        let url = format!("{}/repl", server.uri());
        builder = builder.with_pre_build_hook(move |home| {
            let hook = |tool, input| json!([{"hooks": [{
                "type": "mcp_tool", "server": "compaction-observer", "tool": tool, "input": input
            }]}]);
            let hooks = json!({
                "PreModelRequest": hook("pre", json!({"kind": "${kind}", "request_id": "${request_id}"})),
                "PostModelRequest": hook("post", json!({"kind": "${kind}", "request_id": "${request_id}", "response_id": "${response_id}", "token_usage": "${token_usage}"})),
            });
            let config = json!({"hooks": hooks, "mcp_servers": {"compaction-observer": {
                "url": url, "environment_id": super::rmcp_client::remote_aware_environment_id()
            }}});
            std::fs::write(home.join("config.toml"), toml::to_string(&config).expect("serialize hook config"))
                .expect("install compaction hooks");
        });
        Some(server)
    } else {
        None
    };
    if let Some(accounting) = &accounting {
        let mut extensions = ExtensionRegistryBuilder::new();
        extensions.thread_lifecycle_contributor(accounting.clone());
        builder = builder.with_extensions(Arc::new(extensions.build()));
    }
    let env = test_env().await?;
    let (_config_server, test) = async_compaction_test(
        builder,
        streaming.uri(),
        if over_budget {
            vec![Feature::RetainClientToolOutputs]
        } else {
            vec![]
        },
        env,
        move |config| {
            if observe_attempts {
                config.response_process_mode =
                    codex_config::config_toml::ResponseProcessMode::AfterCompleteResponse;
                core_test_support::hooks::trust_discovered_hooks(config);
            }
            if over_budget {
                config.async_compaction_threshold_percent = 1;
                config.model_auto_compact_token_limit = Some(5_000);
            }
        },
    )
    .await?;
    test.codex.ensure_rollout_materialized().await;
    test.submit_text_turn("first").await?;
    if has_background {
        tokio::time::timeout(timeout, streaming.wait_for_request_count(/*count*/ 2)).await?;
    }
    let release_background = if outcome == PendingCompactionOutcome::ReadyFailure {
        release_background
            .send(())
            .expect("fail background compaction before the next checkpoint");
        // The client must observe the failure before take_ready polls the closed receiver.
        tokio::time::timeout(
            timeout,
            test.codex.wait_for_background_compaction_for_test(),
        )
        .await?;
        None
    } else {
        Some(release_background)
    };
    if manual {
        test.codex.submit(Op::Compact).await?;
    } else if over_budget {
        test.codex.start_or_steer_turn(TurnInputRequest::new(TurnInput::AnnotatedResponseItem(
            codex_protocol::turn_input::AnnotatedResponseItem {
                item: serde_json::from_value(json!({"type": "function_call_output", "name": "send_message_to_thread", "namespace": "codex_app", "output": retained_output}))?,
                annotations: codex_protocol::turn_input::ResponseItemAnnotations { retain: true },
            },
        ))).await?;
    } else {
        test.codex
            .start_or_steer_turn(TurnInputRequest::user_input(vec![text("newer")]))
            .await?;
    }
    let started = wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::ItemStarted(event)
            if matches!(event.item, TurnItem::ContextCompaction(_)))
    })
    .await;
    if has_background && outcome != PendingCompactionOutcome::ReadyFailure {
        assert_eq!(streaming.requests().await.len(), if manual { 2 } else { 3 });
    }
    if outcome == PendingCompactionOutcome::Interrupted {
        test.codex.submit(Op::Interrupt).await?;
        wait_for_event(&test.codex, |event| {
            matches!(event, EventMsg::TurnAborted(_))
        })
        .await;
        release_background
            .expect("pending background compaction")
            .send(())
            .expect("release unused result");
        completions.remove(1).await?;
        test.codex.shutdown_and_wait().await?;
        assert_eq!(streaming.requests().await.len(), if manual { 2 } else { 3 });
        return Ok(());
    }
    if outcome == PendingCompactionOutcome::StaleSuccess {
        let auth = test.thread_manager.auth_manager();
        let mut tokens = auth.auth_cached().expect("test auth").get_token_data()?;
        tokens.id_token.raw_jwt = "e30.e30.signature".to_string();
        tokens.account_id = Some("second-account".to_string());
        tokens.access_token = "second-account-token".to_string();
        std::fs::write(
            test.home.path().join("auth.json"),
            serde_json::to_vec(
                &json!({"auth_mode":"chatgpt", "tokens":tokens, "last_refresh":chrono::Utc::now()}),
            )?,
        )?;
        auth.reload().await;
    }
    if has_background && let Some(release_background) = release_background {
        release_background.send(()).expect("release pending result");
    }
    if observe_attempts {
        tokio::time::timeout(timeout, post_started.notified()).await?;
        // The collector bills a background terminal event before returning its
        // candidate. A blocked post-hook must still hold that event.
        let billed_while_held = accounting
            .as_ref()
            .expect("successful background attempt")
            .reports
            .lock()
            .expect("usage reports")
            .iter()
            .any(|report| {
                report
                    .usage
                    .as_ref()
                    .is_some_and(|usage| usage.total_tokens == BACKGROUND_TOKENS)
            });
        release_post.send(()).expect("release the compaction hook");
        assert!(
            !billed_while_held,
            "background usage reached the collector before its post-hook finished"
        );
    }
    let EventMsg::ItemStarted(started) = started else {
        unreachable!();
    };
    let mut starts = vec![started.item.id()];
    let mut completed = Vec::new();
    wait_for_event(&test.codex, |event| {
        if let EventMsg::ItemStarted(event) = event
            && matches!(event.item, TurnItem::ContextCompaction(_))
        {
            starts.push(event.item.id());
        }
        if let EventMsg::ItemCompleted(event) = event
            && matches!(event.item, TurnItem::ContextCompaction(_))
        {
            completed.push(event.item.id());
        }
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    assert_eq!(completed, starts);
    assert_eq!(starts.len(), if over_budget { 2 } else { 1 });
    assert_eq!(starts.iter().collect::<HashSet<_>>().len(), starts.len());
    tokio::time::timeout(
        timeout,
        test.codex.wait_for_background_compaction_for_test(),
    )
    .await?;
    if manual {
        test.submit_text_turn("after manual compaction").await?;
    }

    let history = test.codex.load_history(/*include_archived*/ false).await?;
    let usage = |id| {
        history
            .items
            .iter()
            .find_map(|item| match item {
                RolloutItem::TokenUsageRecord(record) if record.response_id == id => Some(record),
                _ => None,
            })
            .expect("persisted usage")
    };
    assert!(history.items.iter().all(|item| !matches!(item,
        RolloutItem::TokenUsageRecord(record) if record.response_id == "background"
    )));
    if let Some(accounting) = accounting {
        let reports = accounting.reports.lock().expect("usage reports");
        let charged_turns: Vec<_> = reports
            .iter()
            .filter(|report| {
                report
                    .usage
                    .as_ref()
                    .is_some_and(|usage| usage.total_tokens == BACKGROUND_TOKENS)
            })
            .map(|report| &report.turn_id)
            .collect();
        assert_eq!(charged_turns, [&usage("seed").turn_id]);
    }
    let continuation = usage("continuation");
    if observe_attempts {
        let calls: Vec<_> = recorded.try_iter().collect();
        let compactions = |tool: &str| {
            calls
                .iter()
                .filter(|call| call["name"] == tool)
                .map(|call| &call["arguments"])
                .filter(|input| input["kind"] != "sampling")
                .collect::<Vec<_>>()
        };
        let pre = compactions("pre");
        let post = compactions("post");
        assert_eq!(
            (pre.len(), post.len()),
            (1, 1),
            "adopting the candidate must not run a second attempt"
        );
        let background_id = post[0]["request_id"]
            .as_str()
            .expect("background request ID");
        assert_eq!(pre[0]["request_id"], background_id);
        assert_eq!(pre[0]["kind"], "background_compaction");
        assert_eq!(post[0]["kind"], pre[0]["kind"]);
        assert_eq!(post[0]["response_id"], "background");
        assert_eq!(post[0]["token_usage"]["total_tokens"], BACKGROUND_TOKENS);
        let seed_id = calls
            .iter()
            .map(|call| &call["arguments"])
            .find(|input| input["response_id"] == "seed")
            .and_then(|input| input["request_id"].as_str())
            .expect("ordinary seed request ID");
        assert_ne!(seed_id, background_id);
    }
    assert_eq!(
        (
            continuation.turn_token_usage.total_tokens,
            continuation.thread_token_usage.total_tokens,
        ),
        if over_budget {
            (12_010, 14_010)
        } else if manual {
            (3_000, seed_tokens + if succeeds { 3_000 } else { 6_000 })
        } else if succeeds {
            (94_000, 164_000)
        } else {
            (97_000, 167_000)
        },
    );

    let requests = streaming
        .requests()
        .await
        .iter()
        .map(|body| serde_json::from_slice::<serde_json::Value>(body))
        .collect::<serde_json::Result<Vec<_>>>()?;
    assert_eq!(
        requests.len(),
        (if manual { 2 } else { 3 }) + usize::from(has_background) + usize::from(!succeeds)
    );
    if over_budget {
        for request in &requests[3..] {
            assert!(
                request["input"]
                    .as_array()
                    .expect("compaction or continuation input")
                    .iter()
                    .any(|item| item["output"] == retained_output)
            );
        }
    }
    if manual && !succeeds {
        let foreground = &requests[requests.len() - 2];
        let metadata: serde_json::Value = serde_json::from_str(
            foreground["client_metadata"]["x-codex-turn-metadata"]
                .as_str()
                .expect("manual foreground metadata"),
        )?;
        assert_eq!(metadata["compaction"]["trigger"], "manual");
        assert_eq!(metadata["compaction"]["reason"], "user_requested");
        assert_eq!(metadata["compaction"]["phase"], "standalone_turn");
    }
    let summaries: Vec<_> = requests.last().expect("continuation")["input"]
        .as_array()
        .expect("foreground input")
        .iter()
        .filter(|item| item["type"] == "compaction")
        .map(|item| &item["encrypted_content"])
        .collect();
    assert_eq!(summaries, [&json!(if succeeds { "ASYNC" } else { "SYNC" })]);
    Ok(())
}

#[tokio::test]
async fn unshared_apps_catalog_across_turns() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let requests =
        super::mcp_tool_cache::apps_requests_during_peer_discovery(/*share*/ false).await?;
    let snapshot = context_snapshot::format_request_history_snapshot(
        "A peer finishes Apps discovery during submit. The thread uses its own catalog on its first and second model requests.",
        &requests,
        &ContextSnapshotOptions::default()
            .rewrite_known_segments()
            .include_request_settings(),
    );
    insta::assert_snapshot!("unshared_apps_catalog_across_turns", snapshot);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn realtime_handoff_preserves_accepted_context() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let requests = super::realtime_conversation::realtime_handoff_history_scenario(
        /*block_prompt*/ false,
    )
    .await?;
    insta::assert_snapshot!(
        "realtime_turn_input_accepted_context",
        context_snapshot::format_request_history_snapshot(
            "A realtime handoff consumes the exact sealed transcript referenced by its durable acceptance.",
            &requests,
            &ContextSnapshotOptions::default().rewrite_known_segments(),
        )
    );
    Ok(())
}
