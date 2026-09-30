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

#[test]
fn side_boundary_stops_initial_and_older_turn_pages_in_server_order() {
    use codex_app_server_protocol::ThreadTurnsListResponse;
    use codex_app_server_protocol::Turn;
    use codex_app_server_protocol::TurnItemsView;
    use codex_app_server_protocol::TurnStatus;
    let turn = |id: &str| Turn {
        id: id.into(),
        items: Vec::new(),
        items_view: TurnItemsView::NotLoaded,
        status: TurnStatus::Completed,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    };
    let mut recent = ThreadTurnsListResponse {
        data: vec![turn("a-newest"), turn("z-newer")],
        next_cursor: Some("older".into()),
        backwards_cursor: None,
    };
    super::trim_side_turn_page(&mut recent, Some("m-parent"));
    assert_eq!(
        recent
            .data
            .iter()
            .map(|turn| turn.id.as_str())
            .collect::<Vec<_>>(),
        ["a-newest", "z-newer"]
    );
    assert_eq!(recent.next_cursor.as_deref(), Some("older"));
    let mut older = ThreadTurnsListResponse {
        data: vec![
            turn("b-side-first"),
            turn("m-parent"),
            turn("zz-parent-older"),
        ],
        next_cursor: Some("inherited".into()),
        backwards_cursor: None,
    };
    super::trim_side_turn_page(&mut older, Some("m-parent"));
    assert_eq!(
        older
            .data
            .iter()
            .map(|turn| turn.id.as_str())
            .collect::<Vec<_>>(),
        ["b-side-first"]
    );
    assert_eq!(older.next_cursor, None);
}

#[tokio::test]
async fn side_item_paging_stops_before_inherited_items_even_with_an_empty_boundary_turn()
-> color_eyre::Result<()> {
    use codex_app_server_protocol::ThreadItem;
    use codex_app_server_protocol::ThreadItemEntry;
    use codex_app_server_protocol::ThreadItemsListResponse;
    use codex_app_server_protocol::Turn;
    use codex_app_server_protocol::TurnItemsView;
    use codex_app_server_protocol::TurnStatus;
    let home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .build()
        .await?;
    let mut server = crate::start_embedded_app_server_for_picker(&config).await?;
    let visible = ThreadItem::ContextCompaction {
        id: "side-item".into(),
    };
    let mut turns = vec![Turn {
        id: "side-turn".into(),
        items: Vec::new(),
        items_view: TurnItemsView::NotLoaded,
        status: TurnStatus::Completed,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    }];
    let mut state = super::ThreadHistoryPagination {
        side_boundary: Some("empty-inherited-turn".into()),
        next_item_cursor: Some("page".into()),
        ..Default::default()
    };
    let page = ThreadItemsListResponse {
        data: vec![
            ThreadItemEntry {
                turn_id: "side-turn".into(),
                item: visible.clone(),
                started_at_ms: None,
                completed_at_ms: None,
            },
            ThreadItemEntry {
                turn_id: "older-parent-turn".into(),
                item: ThreadItem::ContextCompaction {
                    id: "parent-item".into(),
                },
                started_at_ms: None,
                completed_at_ms: None,
            },
        ],
        next_cursor: Some("parent-items".into()),
        backwards_cursor: None,
    };
    let items = server
        .merge_thread_item_page(
            codex_protocol::ThreadId::new(),
            page,
            &mut state,
            &mut turns,
        )
        .await?;
    assert_eq!(items, vec![visible.clone()]);
    assert_eq!(turns[0].items, vec![visible]);
    assert_eq!(state.next_item_cursor, None);
    server.shutdown().await?;
    Ok(())
}
