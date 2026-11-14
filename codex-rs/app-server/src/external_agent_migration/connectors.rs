use codex_protocol::ThreadId;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
const SESSION_IMPORT_LEDGER_FILE: &str = "external_agent_session_imports.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct ImportedExternalAgentSessionLedger {
    records: Vec<ImportedExternalAgentSessionRecord>,
    #[serde(default)]
    detected_connector_records: Vec<DetectedExternalAgentSessionConnectorRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DetectedExternalAgentSessionConnectorRecord {
    source_path: PathBuf,
    connector_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ImportedExternalAgentSessionRecord {
    source_path: PathBuf,
    content_sha256: String,
    imported_thread_id: ThreadId,
    imported_at: i64,
    #[serde(default)]
    source_modified_at: Option<i64>,
    #[serde(default)]
    connector_names: Vec<String>,
    #[serde(default)]
    title: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ImportedConnectorCandidate {
    pub name: String,
    pub session_count: u32,
}

pub(super) fn read_imported_connector_candidates(
    codex_home: &Path,
) -> io::Result<Vec<ImportedConnectorCandidate>> {
    let ledger = load_import_ledger(codex_home)?;
    let mut connector_names_by_source = BTreeMap::<PathBuf, Vec<String>>::new();
    for record in ledger.detected_connector_records {
        connector_names_by_source
            .entry(record.source_path)
            .or_default()
            .extend(record.connector_names);
    }
    for record in ledger.records {
        connector_names_by_source
            .entry(record.source_path)
            .or_default()
            .extend(record.connector_names);
    }
    let mut candidates_by_name = BTreeMap::<String, ImportedConnectorCandidate>::new();
    for connector_names in connector_names_by_source.into_values() {
        let mut connector_names_by_key = BTreeMap::new();
        for name in connector_names
            .into_iter()
            .filter_map(|name| normalized_connector_display_name(Some(&name)))
        {
            connector_names_by_key
                .entry(name.to_lowercase())
                .or_insert(name);
        }
        for (key, name) in connector_names_by_key {
            let candidate = candidates_by_name
                .entry(key)
                .or_insert(ImportedConnectorCandidate {
                    name,
                    session_count: 0,
                });
            candidate.session_count = candidate.session_count.saturating_add(1);
        }
    }
    let mut candidates = candidates_by_name.into_values().collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(candidates)
}

fn load_import_ledger(codex_home: &Path) -> io::Result<ImportedExternalAgentSessionLedger> {
    let path = import_ledger_path(codex_home);
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Ok(ImportedExternalAgentSessionLedger::default());
        }
        Err(err) => return Err(err),
    };
    serde_json::from_str(&raw).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid external agent session import ledger: {err}"),
        )
    })
}

fn import_ledger_path(codex_home: &Path) -> PathBuf {
    codex_home.join(SESSION_IMPORT_LEDGER_FILE)
}

fn normalized_connector_display_name(name: Option<&str>) -> Option<String> {
    name.map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
#[path = "connectors_tests.rs"]
mod tests;
