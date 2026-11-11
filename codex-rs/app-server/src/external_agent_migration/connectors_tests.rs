use super::*;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
#[test]
fn connector_candidates_accumulate_imports_for_each_source() {
    let root = TempDir::new().expect("tempdir");
    let codex_home = root.path().join("codex-home");
    let first_source = root.path().join("first.jsonl");
    let second_source = root.path().join("second.jsonl");

    record_completed_session_imports(
        &codex_home,
        vec![
            ImportedExternalAgentSessionRecord {
                source_path: first_source.clone(),
                content_sha256: "first-version".to_string(),
                imported_thread_id: ThreadId::new(),
                connector_names: vec!["Gmail".to_string()],
                title: None,
                imported_at: 0,
                source_modified_at: None,
            },
            ImportedExternalAgentSessionRecord {
                source_path: first_source,
                content_sha256: "second-version".to_string(),
                imported_thread_id: ThreadId::new(),
                connector_names: vec!["Slack".to_string()],
                title: None,
                imported_at: 0,
                source_modified_at: None,
            },
            ImportedExternalAgentSessionRecord {
                source_path: second_source,
                content_sha256: "only-version".to_string(),
                imported_thread_id: ThreadId::new(),
                connector_names: vec!["Gmail".to_string(), "Slack".to_string()],
                title: None,
                imported_at: 0,
                source_modified_at: None,
            },
        ],
    )
    .expect("record imports");

    assert_eq!(
        read_imported_connector_candidates(&codex_home).expect("read connector candidates"),
        vec![
            ImportedConnectorCandidate {
                name: "Gmail".to_string(),
                session_count: 2,
            },
            ImportedConnectorCandidate {
                name: "Slack".to_string(),
                session_count: 2,
            },
        ]
    );
}

#[test]
fn connector_candidates_accumulate_detected_and_imported_session_clues() {
    let root = TempDir::new().expect("tempdir");
    let codex_home = root.path().join("codex-home");
    let source_path = root.path().join("session.jsonl");
    std::fs::write(&source_path, "session contents").expect("source");
    let source_path = std::fs::canonicalize(source_path).expect("canonical source");

    record_detected_session_connectors(
        &codex_home,
        BTreeMap::from([(source_path.clone(), vec!["Figma".to_string()])]),
    )
    .expect("record detected connectors");
    assert_eq!(
        read_imported_connector_candidates(&codex_home).expect("read detected connectors"),
        vec![ImportedConnectorCandidate {
            name: "Figma".to_string(),
            session_count: 1,
        }]
    );

    record_completed_session_imports(
        &codex_home,
        vec![ImportedExternalAgentSessionRecord {
            source_path: source_path.clone(),
            content_sha256: "content-sha".to_string(),
            imported_thread_id: ThreadId::new(),
            connector_names: vec!["Slack".to_string()],
            title: None,
            imported_at: 0,
            source_modified_at: None,
        }],
    )
    .expect("record completed import");
    assert_eq!(
        read_imported_connector_candidates(&codex_home).expect("read imported connectors"),
        vec![
            ImportedConnectorCandidate {
                name: "Figma".to_string(),
                session_count: 1,
            },
            ImportedConnectorCandidate {
                name: "Slack".to_string(),
                session_count: 1,
            },
        ]
    );

    append_imported_session_connector_names(
        &codex_home,
        BTreeMap::from([(source_path, vec!["Gmail".to_string(), "FIGMA".to_string()])]),
    )
    .expect("append imported connector names");
    assert_eq!(
        read_imported_connector_candidates(&codex_home).expect("read appended connectors"),
        vec![
            ImportedConnectorCandidate {
                name: "Figma".to_string(),
                session_count: 1,
            },
            ImportedConnectorCandidate {
                name: "Gmail".to_string(),
                session_count: 1,
            },
            ImportedConnectorCandidate {
                name: "Slack".to_string(),
                session_count: 1,
            },
        ]
    );
}

fn save(home: &Path, ledger: &ImportedExternalAgentSessionLedger) -> io::Result<()> {
    fs::create_dir_all(home)?;
    fs::write(import_ledger_path(home), serde_json::to_vec(ledger)?)
}
fn record_completed_session_imports(
    home: &Path,
    records: Vec<ImportedExternalAgentSessionRecord>,
) -> io::Result<()> {
    let mut ledger = load_import_ledger(home)?;
    ledger.records.extend(records);
    save(home, &ledger)
}
fn record_detected_session_connectors(
    home: &Path,
    records: BTreeMap<PathBuf, Vec<String>>,
) -> io::Result<()> {
    let mut ledger = load_import_ledger(home)?;
    ledger.detected_connector_records = records
        .into_iter()
        .map(
            |(source_path, connector_names)| DetectedExternalAgentSessionConnectorRecord {
                source_path,
                connector_names,
            },
        )
        .collect();
    save(home, &ledger)
}
fn append_imported_session_connector_names(
    home: &Path,
    records: BTreeMap<PathBuf, Vec<String>>,
) -> io::Result<()> {
    let mut ledger = load_import_ledger(home)?;
    for record in &mut ledger.records {
        if let Some(names) = records.get(&record.source_path) {
            record.connector_names.extend(names.iter().cloned());
        }
    }
    save(home, &ledger)
}
