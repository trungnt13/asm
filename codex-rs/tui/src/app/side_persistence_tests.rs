use super::*;
use crate::side_conversations::SideConversation;
use crate::side_conversations::SideConversationStore;
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
        let directory = app.config.codex_home.join("asm-side-conversations/local");
        std::fs::create_dir_all(&directory)?;
        let bytes = serde_json::to_vec(&serde_json::json!({
            "parent": pair.parent,
            "side": pair.side,
            "last_inherited_turn": pair.last_inherited_turn,
        }))?;
        std::fs::write(directory.join(format!("side-{}.json", pair.side)), &bytes)?;
        let selection = directory.join(format!("parent-{}.json", pair.parent));
        std::fs::write(&selection, &bytes)?;
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
                vec![thread],
                vec![thread]
            ),
        );
        assert_eq!(std::fs::read(&selection)?, bytes);
        assert_eq!(store.side(pair.side)?, Some(pair.clone()));
    }
    Ok(())
}
