use super::*;
use crate::legacy_core::config::ConfigBuilder;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn legacy_side_captures_its_boundary_without_requesting_paginated_turns() -> Result<()> {
    let home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .build()
        .await?;
    let parent = ThreadId::from_string(
        &app_test_support::create_fake_rollout(
            home.path(),
            "2025-01-05T12-00-00",
            "2025-01-05T12:00:00Z",
            "Inherited user message",
            Some(config.model_provider_id.as_str()),
            /*git_info*/ None,
        )
        .expect("create legacy rollout"),
    )?;
    let settings = LocalSettings::from(&config);
    let mut server = crate::start_embedded_app_server_for_picker(&config).await?;
    let fork = server
        .fork_side_thread(
            &settings,
            config.clone(),
            parent,
            /*selected_profile*/ None,
        )
        .await?;
    let side = fork.session.thread_id;
    assert_eq!(
        server
            .history_pagination
            .get(&side)
            .map(|state| state.history_mode),
        Some(ThreadHistoryMode::Legacy),
    );
    server.save_side_conversation(parent, side).await?;
    let stored = server
        .side_conversation(side)?
        .expect("saved side boundary");
    assert_eq!((stored.parent, stored.side), (parent, side));
    assert!(stored.last_inherited_turn.is_some());
    assert_eq!(
        server
            .history_pagination
            .get(&side)
            .map(|state| (state.history_mode, state.side_boundary.clone())),
        Some((ThreadHistoryMode::Legacy, stored.last_inherited_turn)),
    );
    server.shutdown().await?;

    let mut server = crate::start_embedded_app_server_for_picker(&config).await?;
    let resumed = server
        .resume_thread(
            &settings,
            config,
            side,
            ResumeModelSettings::PreserveExistingThread,
        )
        .await?;
    assert!(resumed.turns.is_empty());
    let parent = server.thread_read(parent, /*include_turns*/ true).await?;
    assert!(!parent.turns.is_empty());
    server.shutdown().await?;
    Ok(())
}
