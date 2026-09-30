use super::*;
use pretty_assertions::assert_eq;

#[test]
fn saved_sides_restore_from_either_end_and_close_only_the_selected_side() -> std::io::Result<()> {
    let home = tempfile::tempdir()?;
    let store = SideConversationStore::new(home.path(), &AppServerTarget::Embedded);
    let first = SideConversation {
        parent: ThreadId::new(),
        side: ThreadId::new(),
        last_inherited_turn: Some("inherited".into()),
    };
    store.save(&first)?;
    assert_eq!(store.pair(first.parent)?, Some(first.clone()));
    assert_eq!(store.pair(first.side)?, Some(first.clone()));
    let second = SideConversation {
        side: ThreadId::new(),
        ..first.clone()
    };
    store.save(&second)?;
    store.close_selection(first.parent, first.side)?;
    assert_eq!(store.pair(first.parent)?, Some(second.clone()));
    assert_eq!(store.pair(first.side)?, Some(first));
    store.close_selection(second.parent, second.side)?;
    assert_eq!(store.pair(second.parent)?, None);
    assert_eq!(store.pair(second.side)?, Some(second));
    Ok(())
}

#[test]
fn endpoint_namespaces_do_not_expose_credentials_or_mix_servers() -> std::io::Result<()> {
    let home = tempfile::tempdir()?;
    let remote = |url: &str| AppServerTarget::Remote {
        endpoint: RemoteAppServerEndpoint::WebSocket {
            websocket_url: url.into(),
            auth_token: Some("secret-token".into()),
        },
    };
    let first = SideConversationStore::new(home.path(), &remote("wss://first.example"));
    let second = SideConversationStore::new(home.path(), &remote("wss://second.example"));
    let local = SideConversationStore::new(home.path(), &AppServerTarget::Embedded);
    let record = SideConversation {
        parent: ThreadId::new(),
        side: ThreadId::new(),
        last_inherited_turn: None,
    };
    first.save(&record)?;
    assert_eq!(second.pair(record.side)?, None);
    assert_eq!(local.pair(record.side)?, None);
    assert!(!first.directory.to_string_lossy().contains("secret-token"));
    assert!(!first.directory.to_string_lossy().contains("first.example"));
    Ok(())
}

#[test]
fn oversized_metadata_is_rejected_without_unbounded_reads() -> std::io::Result<()> {
    let home = tempfile::tempdir()?;
    let store = SideConversationStore::new(home.path(), &AppServerTarget::Embedded);
    let record = SideConversation {
        parent: ThreadId::new(),
        side: ThreadId::new(),
        last_inherited_turn: None,
    };
    store.save(&record)?;
    std::fs::write(
        store.directory.join(format!("side-{}.json", record.side)),
        vec![b' '; MAX_RECORD_BYTES as usize + 1],
    )?;
    assert!(store.side(record.side).is_err());
    Ok(())
}
