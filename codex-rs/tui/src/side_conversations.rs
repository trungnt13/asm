//! Read-only display boundaries for saved conversations created by the removed /parallel command.

use crate::AppServerTarget;
use crate::RemoteAppServerEndpoint;
use codex_app_server_protocol::Turn;
use codex_protocol::ThreadId;
use serde::Deserialize;
use sha2::Digest;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;

const MAX_RECORD_BYTES: u64 = 4096;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
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

/// Namespace client metadata without including server authentication.
pub(crate) fn app_server_scope(target: &AppServerTarget) -> String {
    match target {
        // The implicit local daemon uses this invocation's CODEX_HOME. Explicit daemon
        // endpoints are Remote targets, so unrelated servers never share this namespace.
        AppServerTarget::Embedded | AppServerTarget::LocalDaemon { .. } => "local".into(),
        AppServerTarget::Cloud => "cloud".into(),
        AppServerTarget::Remote { endpoint } => {
            let endpoint = match endpoint {
                RemoteAppServerEndpoint::WebSocket { websocket_url, .. } => websocket_url.clone(),
                RemoteAppServerEndpoint::UnixSocket { socket_path } => {
                    format!("unix:{}", socket_path.display())
                }
            };
            format!("{:x}", sha2::Sha256::digest(endpoint.as_bytes()))
        }
    }
}

impl SideConversationStore {
    pub(crate) fn new(codex_home: &Path, target: &AppServerTarget) -> Self {
        let scope = app_server_scope(target);
        Self {
            directory: codex_home.join("asm-side-conversations").join(scope),
        }
    }

    pub(crate) fn side(&self, side: ThreadId) -> std::io::Result<Option<SideConversation>> {
        let record = self.read(&self.directory.join(format!("side-{side}.json")))?;
        Ok(record.filter(|record| record.side == side && record.parent != side))
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
}

#[cfg(test)]
#[path = "side_conversations_tests.rs"]
mod tests;
