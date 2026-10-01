use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn failed_side_setup_cleanup_clears_selection_but_preserves_saved_child() -> Result<()> {
    use crate::side_conversations::SideConversation;
    use crate::side_conversations::SideConversationStore;

    let mut app = Box::pin(super::super::test_support::make_test_app()).await;
    let mut app_server = crate::start_embedded_app_server_for_picker(&app.config).await?;
    let started = app_server.start_thread(&app.config).await?;
    let pair = SideConversation {
        parent: ThreadId::new(),
        side: started.session.thread_id,
        last_inherited_turn: None,
    };
    let store = SideConversationStore::new(&app.config.codex_home, &app.app_server_target);
    store.save(&pair)?;
    app.side_threads
        .insert(pair.side, SideThreadState::parallel(pair.parent));
    let mut tui = crate::tui::test_support::make_test_tui()?;

    assert!(
        Box::pin(app.discard_side_thread_or_keep_visible(&mut tui, &mut app_server, pair.side))
            .await
    );
    assert_eq!(store.pair(pair.parent)?, None);
    assert_eq!(store.side(pair.side)?, Some(pair));
    app_server.shutdown().await?;
    Ok(())
}
