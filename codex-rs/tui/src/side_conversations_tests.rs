use super::*;
use pretty_assertions::assert_eq;

#[test]
fn legacy_child_records_remain_readable_without_pairing() -> std::io::Result<()> {
    let home = tempfile::tempdir()?;
    let store = SideConversationStore::new(home.path(), &AppServerTarget::Embedded);
    let record = SideConversation {
        parent: ThreadId::new(),
        side: ThreadId::new(),
        last_inherited_turn: Some("inherited".into()),
    };
    std::fs::create_dir_all(&store.directory)?;
    std::fs::write(
        store.directory.join(format!("side-{}.json", record.side)),
        serde_json::to_vec(&serde_json::json!({
            "parent": record.parent,
            "side": record.side,
            "last_inherited_turn": record.last_inherited_turn,
        }))?,
    )?;
    assert_eq!(store.side(record.side)?, Some(record.clone()));
    assert_eq!(store.side(record.parent)?, None);
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
    std::fs::create_dir_all(&first.directory)?;
    std::fs::write(
        first.directory.join(format!("side-{}.json", record.side)),
        serde_json::to_vec(&serde_json::json!({
            "parent": record.parent,
            "side": record.side,
            "last_inherited_turn": record.last_inherited_turn,
        }))?,
    )?;
    assert_eq!(second.side(record.side)?, None);
    assert_eq!(local.side(record.side)?, None);
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
    std::fs::create_dir_all(&store.directory)?;
    std::fs::write(
        store.directory.join(format!("side-{}.json", record.side)),
        serde_json::to_vec(&serde_json::json!({
            "parent": record.parent,
            "side": record.side,
            "last_inherited_turn": record.last_inherited_turn,
        }))?,
    )?;
    std::fs::write(
        store.directory.join(format!("side-{}.json", record.side)),
        vec![b' '; MAX_RECORD_BYTES as usize + 1],
    )?;
    assert!(store.side(record.side).is_err());
    Ok(())
}
