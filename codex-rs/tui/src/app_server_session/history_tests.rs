use super::HistoryHydrationScope;
use super::HistoryLoadBudget;
use super::advancing_cursor;
use crate::legacy_core::config::ConfigBuilder;
use crate::legacy_core::config::TerminalResizeReflowMaxRows;
use crate::local_settings::LocalSettings;
use codex_config::types::AltScreenMode;
use pretty_assertions::assert_eq;
use std::collections::HashSet;

#[test]
fn advancing_cursor_rejects_repeated_cursors() {
    let mut seen_cursors = HashSet::new();
    assert_eq!(
        advancing_cursor(
            /*current*/ None,
            Some("first".to_string()),
            &mut seen_cursors,
        ),
        Some("first".to_string())
    );
    assert_eq!(
        advancing_cursor(Some("first"), Some("second".to_string()), &mut seen_cursors,),
        Some("second".to_string())
    );
    assert_eq!(
        advancing_cursor(Some("second"), Some("first".to_string()), &mut seen_cursors,),
        None
    );
    assert_eq!(
        advancing_cursor(Some("second"), /*next*/ None, &mut seen_cursors),
        None
    );
}

#[tokio::test]
async fn owned_initial_history_stops_after_viewport_or_scan_budget() {
    let codex_home = tempfile::tempdir().expect("temporary codex home");
    let mut config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .build()
        .await
        .expect("config");
    config.tui_fullscreen_transcript = true;
    config.tui_alternate_screen = AltScreenMode::Always;
    config.terminal_resize_reflow.max_rows = TerminalResizeReflowMaxRows::Disabled;
    let local_settings = LocalSettings::from(&config);
    let budget = HistoryLoadBudget::new(
        HistoryHydrationScope::Initial,
        Some(&config),
        Some(&local_settings),
        /*terminal_height*/ 10,
    );

    assert_eq!(
        [
            budget.next_page_size(/*rendered_rows*/ 0, /*scanned_items*/ 0),
            budget.next_page_size(/*rendered_rows*/ 29, /*scanned_items*/ 10),
            budget.next_page_size(/*rendered_rows*/ 30, /*scanned_items*/ 11),
            budget.next_page_size(/*rendered_rows*/ 0, /*scanned_items*/ 399),
            budget.next_page_size(/*rendered_rows*/ 0, /*scanned_items*/ 400),
        ],
        [Some(30), Some(100), None, Some(1), None],
    );

    let complete = HistoryLoadBudget::new(
        HistoryHydrationScope::Complete,
        Some(&config),
        /*local_settings*/ None,
        /*terminal_height*/ 10,
    );
    assert_eq!(
        complete.next_page_size(/*rendered_rows*/ 10_000, /*scanned_items*/ 10_000),
        Some(100),
    );

    config.tui_fullscreen_transcript = false;
    // The mode selected at launch wins even if thread config changes or is unavailable.
    for config in [Some(&config), None] {
        let budget = HistoryLoadBudget::new(
            HistoryHydrationScope::Initial,
            config,
            Some(&local_settings),
            /*terminal_height*/ 10,
        );
        assert_eq!(
            [
                budget.next_page_size(/*rendered_rows*/ 0, /*scanned_items*/ 0),
                budget.next_page_size(/*rendered_rows*/ 30, /*scanned_items*/ 11),
            ],
            [Some(30), None],
        );
    }
}

#[tokio::test]
async fn saved_fork_preserves_daybreak_and_legacy_history_on_resume() -> color_eyre::Result<()> {
    use super::super::*;
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
    server.thread_read(parent, /*include_turns*/ false).await?;
    let request_id = server.next_request_id();
    let _: ThreadMetadataUpdateResponse = server
        .request_handle
        .request_typed(ClientRequest::ThreadMetadataUpdate {
            request_id,
            params: ThreadMetadataUpdateParams {
                thread_id: parent.to_string(),
                project_id: None,
                git_info: None,
                daybreak_enabled: Some(true),
            },
        })
        .await?;
    let fork = server
        .fork_thread(&settings, config.clone(), parent)
        .await?;
    let side = fork.session.thread_id;
    assert_eq!(
        (
            server
                .thread_read(side, /*include_turns*/ false)
                .await?
                .daybreak_enabled,
            fork.session.daybreak_enabled,
        ),
        (Some(true), true),
    );
    let mut temporary_config = config.clone();
    temporary_config.ephemeral = true;
    let temporary = server
        .fork_side_thread(
            &settings,
            temporary_config,
            parent,
            /*selected_profile*/ None,
            crate::app_server_session::SideChatStorage::Temporary,
        )
        .await?;
    assert!(!temporary.session.daybreak_enabled);
    let inherited = server.thread_read(side, /*include_turns*/ true).await?;
    assert_eq!(inherited.history_mode, ThreadHistoryMode::Legacy);
    let inherited_turns = inherited.turns;
    assert!(!inherited_turns.is_empty());
    server.shutdown().await?;

    let mut server = crate::start_embedded_app_server_for_picker(&config).await?;
    let resumed = server
        .resume_thread(
            &settings,
            config.clone(),
            side,
            ResumeModelSettings::PreserveExistingThread,
        )
        .await?;
    assert_eq!(resumed.turns, inherited_turns);
    assert!(resumed.session.daybreak_enabled);
    let parent = server.thread_read(parent, /*include_turns*/ true).await?;
    assert!(!parent.turns.is_empty());
    server.shutdown().await?;
    Ok(())
}
