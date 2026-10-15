use super::bridge::ClaudeToolBridge;
use codex_claude_runtime::ClaudeRuntime;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
pub(super) struct Checkpoint {
    pub(super) model: String,
    pub(super) effort: Option<String>,
    pub(super) cwd: PathBuf,
    pub(super) session_id: String,
    pub(super) instructions: String,
    pub(super) authority: Value,
    pub(super) in_flight: bool,
    pub(super) delivered_items: usize,
    pub(super) delivered_prefix: String,
}

pub(crate) struct ClaudeSession {
    pub(super) runtime: ClaudeRuntime,
    pub(super) bridge: ClaudeToolBridge,
    pub(super) checkpoint: Checkpoint,
    pub(super) checkpoint_path: PathBuf,
    pub(super) _lock: File,
    pub(super) diagnostic_capture: bool,
}

impl ClaudeSession {
    pub(super) fn save_checkpoint(&self) -> anyhow::Result<()> {
        let temporary = self
            .checkpoint_path
            .with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(&serde_json::to_vec(&self.checkpoint)?)?;
        file.sync_all()?;
        std::fs::rename(&temporary, &self.checkpoint_path)?;
        if let Some(parent) = self.checkpoint_path.parent() {
            File::open(parent)?.sync_all()?;
        }
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct RuntimeSlot {
    state: std::sync::Mutex<SlotState>,
    returned: tokio::sync::Notify,
}

#[derive(Default)]
struct SlotState {
    runtime: Option<ClaudeSession>,
    leased: bool,
    discard_on_return: bool,
}

impl RuntimeSlot {
    pub(super) fn checkpoint(&self) -> anyhow::Result<Option<(usize, String)>> {
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Claude session lock was poisoned"))?;
        Ok(state.runtime.as_ref().map(|runtime| {
            (
                runtime.checkpoint.delivered_items,
                runtime.checkpoint.delivered_prefix.clone(),
            )
        }))
    }

    pub(super) async fn take_for_shutdown(&self) -> anyhow::Result<Option<ClaudeSession>> {
        let returned = async {
            loop {
                // Register before inspecting state so all concurrent shutdown waiters wake.
                let notification = self.returned.notified();
                tokio::pin!(notification);
                notification.as_mut().enable();
                let ready = {
                    let mut state = self
                        .state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if state.leased {
                        None
                    } else {
                        Some(state.runtime.take())
                    }
                };
                if let Some(runtime) = ready {
                    return runtime;
                }
                notification.await;
            }
        };
        match tokio::time::timeout(std::time::Duration::from_secs(/*secs*/ 5), returned).await {
            Ok(runtime) => Ok(runtime),
            Err(_) => {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state.discard_on_return = true;
                // A late return is killed by Drop rather than becoming an unowned idle process.
                drop(state.runtime.take());
                anyhow::bail!(
                    "Claude runtime did not return from its aborted turn within five seconds"
                )
            }
        }
    }
}

/// Returns the process on success, error, or forced destruction of its turn future.
pub(super) struct RuntimeLease<'a> {
    slot: &'a RuntimeSlot,
    pub(super) runtime: Option<ClaudeSession>,
}

impl<'a> RuntimeLease<'a> {
    pub(super) fn take(slot: &'a RuntimeSlot) -> anyhow::Result<Self> {
        let mut state = slot
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Claude session lock was poisoned"))?;
        anyhow::ensure!(!state.leased, "Claude runtime is already in use");
        anyhow::ensure!(
            !state.discard_on_return,
            "Claude runtime shutdown timed out; start a new chat"
        );
        state.leased = true;
        let runtime = state.runtime.take();
        Ok(Self { slot, runtime })
    }
}

impl Drop for RuntimeLease<'_> {
    fn drop(&mut self) {
        let mut state = self
            .slot
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.discard_on_return {
            state.runtime = self.runtime.take();
        }
        state.leased = false;
        self.slot.returned.notify_waiters();
    }
}
