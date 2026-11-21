//! Model-visible instructions and tools from catalog routing, refresh, and compaction.

use super::*;
use codex_protocol::config_types::ModeKind;
use codex_protocol::openai_models::CollaborationModeMessages;
use codex_protocol::protocol::ThreadSettingsOverrides;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initial_collaboration_guidance_survives_resume_until_plan() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let builder = || {
        test_codex()
            .with_model("gpt-5.5")
            .with_config(|config| {
                configure_scenario_catalog(config);
                config.include_environment_context = false;
                config.include_permissions_instructions = false;
            })
            .with_model_info_override("gpt-5.5", |model| {
                let messages = model.model_messages.get_or_insert_default();
                messages.instructions_template = Some("Help with the migration.".into());
                messages.collaboration_modes = Some(CollaborationModeMessages {
                    initial: Some("Work directly on the user's request.".into()),
                    plan: Some("Prepare a plan before editing.".into()),
                    default: Some("Resume implementation.".into()),
                });
            })
    };
    let mock = mount_sse_sequence(
        &server,
        (1..=4)
            .map(|i| sse(vec![ev_completed(&format!("response-{i}"))]))
            .collect(),
    )
    .await;
    let test = builder().build_with_auto_env(&server).await?;
    test.submit_text_turn("Inspect the migration.").await?;
    let resumed = builder().restart_with_auto_env(&server, &test).await?;
    for (mode, prompt) in [
        (ModeKind::Default, "Check the rollback path too."),
        (ModeKind::Plan, "Plan the migration."),
        (ModeKind::Default, "Implement the plan."),
    ] {
        let mut collaboration_mode = resumed.codex.config_snapshot().await.collaboration_mode;
        collaboration_mode.mode = mode;
        resumed
            .codex
            .update_thread_settings(ThreadSettingsOverrides {
                collaboration_mode: Some(collaboration_mode),
                ..Default::default()
            })
            .await?;
        resumed.submit_text_turn(prompt).await?;
    }
    insta::assert_snapshot!(context_snapshot::format_request_history_snapshot(
        "A catalog supplies initial guidance. After a restart, the client selects Default, enters Plan, and returns to Default.",
        &mock.requests(),
        &ContextSnapshotOptions::default(),
    ));
    Ok(())
}
