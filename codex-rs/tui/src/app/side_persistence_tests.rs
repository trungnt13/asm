use super::*;
use crate::side_conversations::SideConversation;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn restoring_either_end_keeps_parent_lazy_and_unrelated_threads_ordinary() -> Result<()> {
    let mut app = Box::pin(super::super::test_support::make_test_app()).await;
    let pair = SideConversation {
        parent: ThreadId::new(),
        side: ThreadId::new(),
        last_inherited_turn: Some("inherited".into()),
    };
    SideConversationStore::new(&app.config.codex_home, &app.app_server_target).save(&pair)?;
    // No server or parent rollout exists: local pairing must not require either one.
    for thread in [pair.parent, pair.side] {
        app.side_threads.clear();
        app.primary_thread_id = Some(thread);
        app.restore_side_conversation(thread);
        assert_eq!(app.primary_thread_id, Some(pair.parent));
        assert_eq!(
            app.side_threads
                .get(&pair.side)
                .map(|state| state.parent_thread_id),
            Some(pair.parent)
        );
        assert!(app.thread_event_channels.is_empty());
        app.agent_navigation.mark_closed(pair.parent);
        app.agent_navigation.mark_closed(pair.side);
        assert!(app.should_attach_live_thread_for_selection(pair.parent));
        assert!(app.should_attach_live_thread_for_selection(pair.side));
    }
    app.side_threads.clear();
    let unrelated = ThreadId::new();
    app.primary_thread_id = Some(unrelated);
    app.restore_side_conversation(unrelated);
    assert_eq!(app.primary_thread_id, Some(unrelated));
    assert!(app.side_threads.is_empty());
    Ok(())
}
