use super::bridge::ClaudeToolBridge;
use codex_claude_runtime::ClaudeRuntime;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Checkpoint {
    pub(super) model: String,
    pub(super) effort: Option<String>,
    pub(super) cwd: PathBuf,
    pub(super) session_id: String,
    pub(super) instructions: String,
    pub(super) authority: Value,
    #[serde(default)]
    pub(super) system_policy: Option<SystemPolicy>,
    pub(super) in_flight: bool,
    pub(super) delivered_items: usize,
    pub(super) delivered_prefix: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct SystemPolicy {
    pub(super) stable_instructions: String,
    pub(super) approved_prefixes: std::collections::BTreeSet<Vec<String>>,
}

#[derive(Deserialize)]
pub(super) struct PermissionSnapshot {
    pub(super) instructions: String,
    pub(super) approved_command_prefixes: std::collections::BTreeSet<Vec<String>>,
}

impl SystemPolicy {
    pub(super) fn permits_refresh(&self, current: &Self) -> bool {
        self.stable_instructions == current.stable_instructions
            && self.approved_prefixes.is_subset(&current.approved_prefixes)
    }

    pub(super) fn verifies_receipt(&self, current: &Self, text: &str) -> bool {
        use crate::context::ContextualUserFragment;
        if !self.permits_refresh(current) {
            return false;
        }
        let prefixes = text
            .lines()
            .skip(1)
            .map(|line| {
                line.strip_prefix("- ")
                    .and_then(|json| serde_json::from_str::<Vec<String>>(json).ok())
            })
            .collect::<Option<std::collections::BTreeSet<_>>>();
        let Some(prefixes) = prefixes else {
            return false;
        };
        if prefixes.is_empty()
            || !prefixes.is_subset(&current.approved_prefixes)
            || !prefixes.is_disjoint(&self.approved_prefixes)
        {
            return false;
        }
        let Some(formatted) =
            codex_protocol::models::format_allow_prefixes(prefixes.into_iter().collect())
        else {
            return false;
        };
        crate::context::ApprovedCommandPrefixSaved::new(formatted).body() == text
    }
}

impl Checkpoint {
    pub(super) fn permits_system(
        &self,
        instructions: &str,
        policy: &SystemPolicy,
    ) -> anyhow::Result<()> {
        if self.instructions == instructions {
            return Ok(());
        }
        anyhow::ensure!(
            self.system_policy
                .as_ref()
                .is_some_and(|previous| previous.permits_refresh(policy)),
            "Claude system policy changed without a verified approved-prefix transition; start a new chat (older checkpoints cannot refresh changed instructions)"
        );
        Ok(())
    }
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
    pub(super) async fn spawn_runtime(
        bridge: &ClaudeToolBridge,
        checkpoint: &Checkpoint,
        session: codex_claude_runtime::ClaudeSession,
    ) -> anyhow::Result<ClaudeRuntime> {
        use anyhow::Context;
        let allowed_tools = bridge
            .tool_names()
            .await
            .into_iter()
            .map(|name| format!("mcp__asm__{name}"))
            .collect();
        ClaudeRuntime::spawn(codex_claude_runtime::ClaudeOptions {
            executable: PathBuf::from("claude"),
            cwd: checkpoint.cwd.clone(),
            model: checkpoint
                .model
                .strip_prefix("claude-code/")
                .context("not a Claude model")?
                .to_owned(),
            effort: checkpoint.effort.clone(),
            system_prompt: checkpoint.instructions.clone(),
            session,
            mcp_config: serde_json::json!({"mcpServers":{"asm":{"type":"http","url":bridge.url(),
                "headers":{"Authorization":format!("Bearer {}", bridge.bearer_token())}}}}),
            allowed_tools,
            turn_timeout: std::time::Duration::from_secs(/*secs*/ 3600),
        })
        .await
    }

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
    pub(super) fn checkpoint(&self) -> anyhow::Result<Option<Checkpoint>> {
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Claude session lock was poisoned"))?;
        Ok(state
            .runtime
            .as_ref()
            .map(|runtime| runtime.checkpoint.clone()))
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
