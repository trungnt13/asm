use super::*;
use crate::side_conversations::SideConversation;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn resuming_either_end_and_unrelated_threads_keeps_only_selected_session() -> Result<()> {
    let seed_app = Box::pin(super::super::test_support::make_test_app()).await;
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(
        &seed_app.config,
    ))
    .await?;
    let started = server.start_thread(&seed_app.config).await?;
    let pair = SideConversation {
        parent: ThreadId::new(),
        side: ThreadId::new(),
        last_inherited_turn: Some("inherited".into()),
    };
    for thread in [pair.parent, pair.side, ThreadId::new()] {
        let mut app = Box::pin(super::super::test_support::make_test_app()).await;
        let store = SideConversationStore::new(&app.config.codex_home, &app.app_server_target);
        store.save(&pair)?;
        let mut session = started.session.clone();
        session.thread_id = thread;
        app.enqueue_primary_thread_session(session, Vec::new())
            .await?;
        assert_eq!(
            (
                app.primary_thread_id,
                app.current_displayed_thread_id(),
                app.primary_session_configured
                    .as_ref()
                    .map(|session| session.thread_id),
                app.side_threads.is_empty(),
                app.chat_widget.parallel_conversation_active(),
                app.chat_widget.side_conversation_active(),
                app.agent_navigation.tracked_thread_ids(),
                app.thread_event_channels
                    .keys()
                    .copied()
                    .collect::<Vec<_>>(),
            ),
            (
                Some(thread),
                Some(thread),
                Some(thread),
                true,
                false,
                false,
                vec![thread],
                vec![thread]
            ),
        );
        assert_eq!(store.pair(pair.parent)?, Some(pair.clone()));
        assert_eq!(store.side(pair.side)?, Some(pair.clone()));
    }
    Ok(())
}
