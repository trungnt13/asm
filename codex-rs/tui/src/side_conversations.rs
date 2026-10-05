//! Local TUI navigation metadata; saved threads remain ordinary app-server threads.

use crate::AppServerTarget;
use crate::RemoteAppServerEndpoint;
use codex_app_server_protocol::Turn;
use codex_protocol::ThreadId;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

const MAX_RECORD_BYTES: u64 = 4096;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct SideConversation {
    pub(crate) parent: ThreadId,
    pub(crate) side: ThreadId,
    pub(crate) last_inherited_turn: Option<String>,
}

impl SideConversation {
    pub(crate) fn trim_turns(&self, turns: &mut Vec<Turn>) {
        if let Some(boundary) = &self.last_inherited_turn
            && let Some(index) = turns.iter().position(|turn| &turn.id == boundary)
        {
            turns.drain(..=index);
        }
    }
}

pub(crate) struct SideConversationStore {
    directory: PathBuf,
}

impl SideConversationStore {
    pub(crate) fn new(codex_home: &Path, target: &AppServerTarget) -> Self {
        let scope = match target {
            // The implicit local daemon uses this invocation's CODEX_HOME. Explicit daemon
            // endpoints are Remote targets, so unrelated servers never share this namespace.
            AppServerTarget::Embedded | AppServerTarget::LocalDaemon { .. } => "local".into(),
            AppServerTarget::Remote { endpoint } => {
                let endpoint = match endpoint {
                    RemoteAppServerEndpoint::WebSocket { websocket_url, .. } => {
                        websocket_url.clone()
                    }
                    RemoteAppServerEndpoint::UnixSocket { socket_path } => {
                        format!("unix:{}", socket_path.display())
                    }
                };
                format!("{:x}", sha2::Sha256::digest(endpoint.as_bytes()))
            }
        };
        Self {
            directory: codex_home.join("asm-side-conversations").join(scope),
        }
    }

    pub(crate) fn side(&self, side: ThreadId) -> std::io::Result<Option<SideConversation>> {
        let record = self.read(&self.directory.join(format!("side-{side}.json")))?;
        Ok(record.filter(|record| record.side == side && record.parent != side))
    }

    pub(crate) fn save(&self, record: &SideConversation) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.directory)?;
        let _lock = self.lock_parent(record.parent)?;
        // Write the immutable child record first. A failed parent selection must not lose the
        // boundary of a successfully saved child, including when another client selects a side.
        self.write(&format!("side-{}.json", record.side), record)?;
        self.write(&format!("parent-{}.json", record.parent), record)
    }

    pub(crate) fn close_selection(&self, parent: ThreadId, side: ThreadId) -> std::io::Result<()> {
        if !self.directory.exists() {
            return Ok(());
        }
        let _lock = self.lock_parent(parent)?;
        let path = self.directory.join(format!("parent-{parent}.json"));
        if self.read(&path)?.is_some_and(|record| record.side == side) {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn lock_parent(&self, parent: ThreadId) -> std::io::Result<std::fs::File> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.directory.join(format!("parent-{parent}.lock")))?;
        file.lock()?;
        Ok(file)
    }

    fn read(&self, path: &Path) -> std::io::Result<Option<SideConversation>> {
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        file.take(MAX_RECORD_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(std::io::Error::other(
                "side conversation record exceeds size limit",
            ));
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(std::io::Error::other)
    }

    fn write(&self, name: &str, record: &SideConversation) -> std::io::Result<()> {
        let bytes = serde_json::to_vec(record)?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(std::io::Error::other(
                "side conversation record exceeds size limit",
            ));
        }
        let mut temporary = tempfile::NamedTempFile::new_in(&self.directory)?;
        temporary.write_all(&bytes)?;
        temporary.as_file().sync_all()?;
        temporary.persist(self.directory.join(name))?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "side_conversations_tests.rs"]
mod tests;
