use super::bridge::ExternalMessageBridge;
use codex_external_runtime::ClaudeRuntime;
use codex_external_runtime::CursorRuntime;
use codex_protocol::openai_models::NativeRuntime;
use codex_protocol::protocol::TokenUsage;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
pub(super) struct Checkpoint {
    pub(super) format_version: u8,
    pub(super) model: String,
    pub(super) effort: Option<String>,
    #[serde(default)]
    pub(super) auto_compact_window: Option<i64>,
    pub(super) cwd: PathBuf,
    pub(super) session_id: String,
    pub(super) messaging: bool,
    pub(super) in_flight: bool,
    pub(super) input_count: usize,
    pub(super) input_prefix: String,
    /// Final native message of the last successful turn; older checkpoints lack it.
    #[serde(default)]
    pub(super) fork_point: Option<String>,
}

impl Checkpoint {
    pub(super) fn validate(
        &self,
        model: &str,
        auto_compact_window: Option<i64>,
        cwd: &std::path::Path,
        messaging: bool,
        messages: &[serde_json::Value],
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.format_version == 2,
            "incompatible native session linkage; start a new chat"
        );
        anyhow::ensure!(
            !self.in_flight,
            "ASM could not confirm that the native runtime finished the previous turn. This chat is blocked to prevent duplicate execution; start a new chat."
        );
        anyhow::ensure!(
            self.model == model
                && self.auto_compact_window == auto_compact_window
                && self.cwd == cwd
                && self.messaging == messaging,
            "saved native model, compaction window, directory, or messaging configuration changed; start a new chat"
        );
        anyhow::ensure!(
            self.input_count <= messages.len()
                && self.input_prefix == super::input_prefix(&messages[..self.input_count])?,
            "saved user input does not match the native session checkpoint; refusing replay"
        );
        Ok(())
    }
    pub(super) fn save(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(&serde_json::to_vec(self)?)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        if let Some(parent) = path.parent() {
            File::open(parent)?.sync_all()?;
        }
        Ok(())
    }
}

/// The official CLI process serving one loaded native session.
pub(super) enum RuntimeProcess {
    Claude(ClaudeRuntime),
    Cursor(CursorRuntime),
}

impl RuntimeProcess {
    pub(super) async fn shutdown(&mut self) -> anyhow::Result<()> {
        match self {
            Self::Claude(runtime) => runtime.shutdown().await,
            Self::Cursor(runtime) => runtime.shutdown().await,
        }
    }

    pub(super) fn diagnostic_snapshot(&self) -> serde_json::Value {
        match self {
            Self::Claude(runtime) => runtime.diagnostic_snapshot(),
            Self::Cursor(runtime) => runtime.diagnostic_snapshot(),
        }
    }
}

/// Where a loaded session keeps its native linkage.
pub(super) enum Linkage {
    Saved,
    /// Temporary forks are unsaved natively, so a saved checkpoint could only mislead cold resume.
    Temporary,
}

/// A clean Claude parent captured when a temporary side fork was created.
pub(crate) struct ForkSeed {
    pub(super) parent: Checkpoint,
    pub(super) fork_point: String,
}

enum TemporaryFork {
    Pending(ForkSeed),
    Started,
}

pub(crate) struct ExternalSession {
    pub(super) runtime: RuntimeProcess,
    pub(super) bridge: Option<ExternalMessageBridge>,
    pub(super) checkpoint: Checkpoint,
    pub(super) checkpoint_path: PathBuf,
    pub(super) linkage: Linkage,
    pub(super) _lock: File,
    pub(super) diagnostic_capture: bool,
    pub(super) usage_baseline: Option<BTreeMap<String, ModelUsage>>,
}

#[derive(Clone, Copy)]
pub(super) struct ModelUsage {
    billing: [i64; 4],
    thinking: Option<i64>,
}

fn decode_model_usage(result: &serde_json::Value) -> Option<BTreeMap<String, ModelUsage>> {
    result["modelUsage"]
        .as_object()
        .filter(|models| !models.is_empty())
        .and_then(|models| {
            models
                .iter()
                .map(|(model, usage)| {
                    let mut counters = [0; 4];
                    for (index, field) in [
                        "inputTokens",
                        "cacheReadInputTokens",
                        "cacheCreationInputTokens",
                        "outputTokens",
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        counters[index] = usage[field].as_i64().filter(|value| *value >= 0)?;
                    }
                    let thinking = match usage.get("thinkingTokens") {
                        Some(tokens) => Some(tokens.as_i64().filter(|tokens| *tokens >= 0)?),
                        None => None,
                    };
                    if thinking.is_some_and(|thinking| thinking > counters[3]) {
                        return None;
                    }
                    Some((
                        model.clone(),
                        ModelUsage {
                            billing: counters,
                            thinking,
                        },
                    ))
                })
                .collect::<Option<BTreeMap<_, _>>>()
        })
}

impl Linkage {
    pub(super) fn save(
        &self,
        checkpoint: &Checkpoint,
        path: &std::path::Path,
    ) -> anyhow::Result<()> {
        match self {
            Self::Saved => checkpoint.save(path),
            Self::Temporary => Ok(()),
        }
    }
}

pub(super) enum SessionStart {
    New {
        id: String,
    },
    Resume {
        id: String,
    },
    Fork {
        parent: String,
        at: String,
        id: String,
    },
}

impl ExternalSession {
    #[tracing::instrument(skip_all)]
    pub(super) async fn spawn_runtime(
        bridge: Option<&ExternalMessageBridge>,
        disallowed_tools: Vec<String>,
        role_instructions: Option<String>,
        session_kind: codex_external_runtime::ClaudeSessionKind,
        checkpoint: &mut Checkpoint,
        checkpoint_path: &std::path::Path,
        session: SessionStart,
    ) -> anyhow::Result<RuntimeProcess> {
        use anyhow::Context;
        let (runtime, model) =
            NativeRuntime::for_model(&checkpoint.model).context("not a native runtime model")?;
        if runtime == NativeRuntime::CursorAgent {
            let session = match session {
                SessionStart::New { .. } => codex_external_runtime::CursorSession::New,
                SessionStart::Resume { id } => codex_external_runtime::CursorSession::Load { id },
                SessionStart::Fork { .. } => {
                    anyhow::bail!("Cursor Agent has no native session fork")
                }
            };
            let runtime = CursorRuntime::spawn(codex_external_runtime::CursorOptions {
                executable: PathBuf::from("agent"),
                cwd: checkpoint.cwd.clone(),
                model: model.to_owned(),
                effort: checkpoint.effort.clone(),
                role_instructions,
                session,
                config_dir: checkpoint_path.with_extension("cursor-config"),
                bridge_directory: bridge.map(|bridge| bridge.directory().to_owned()),
            })
            .await?;
            // Cursor assigns new session IDs; link before the first prompt is submitted.
            checkpoint.session_id = runtime.session_id().to_owned();
            return Ok(RuntimeProcess::Cursor(runtime));
        }
        let model = if model.starts_with("sonnet-") || model.starts_with("opus-") {
            format!("claude-{model}")
        } else {
            model.to_owned()
        };
        let session = match session {
            SessionStart::New { id } => codex_external_runtime::ClaudeSession::New { id },
            SessionStart::Resume { id } => codex_external_runtime::ClaudeSession::Resume { id },
            SessionStart::Fork { parent, at, id } => {
                codex_external_runtime::ClaudeSession::Fork { parent, at, id }
            }
        };
        ClaudeRuntime::spawn(codex_external_runtime::ClaudeOptions {
            executable: PathBuf::from("claude"),
            cwd: checkpoint.cwd.clone(),
            model,
            effort: checkpoint.effort.clone(),
            auto_compact_window: checkpoint.auto_compact_window,
            session_kind,
            role_instructions,
            session,
            disallowed_tools,
            bridge_directory: bridge.map(|bridge| bridge.directory().to_owned()),
        })
        .await
        .map(RuntimeProcess::Claude)
    }

    pub(super) fn measure_usage_delta(&mut self, result: &serde_json::Value) -> Option<TokenUsage> {
        let current = decode_model_usage(result);
        // Native resume restores cost-state only when present; its first baseline is unknown.
        let previous = std::mem::replace(&mut self.usage_baseline, current);
        let (Some(previous), Some(current)) = (previous, &self.usage_baseline) else {
            return None;
        };
        let mut reasoning_available = true;
        let delta = (|| {
            if previous.keys().any(|model| !current.contains_key(model)) {
                return None;
            }
            let mut totals = [0i64; 5];
            for (model, counters) in current {
                let baseline = previous.get(model).copied().unwrap_or(ModelUsage {
                    billing: [0; 4],
                    thinking: Some(0),
                });
                let mut billing = [0i64; 4];
                for index in 0..4 {
                    billing[index] = counters.billing[index]
                        .checked_sub(baseline.billing[index])
                        .filter(|delta| *delta >= 0)?;
                    totals[index] = totals[index].checked_add(billing[index])?;
                }
                let thinking = match (counters.thinking, baseline.thinking) {
                    (Some(current), Some(previous)) => current
                        .checked_sub(previous)
                        .filter(|thinking| *thinking >= 0 && *thinking <= billing[3])?,
                    // An inactive legacy model cannot add thinking without adding output.
                    _ if billing[3] == 0 => 0,
                    _ => {
                        reasoning_available = false;
                        0
                    }
                };
                totals[4] = totals[4].checked_add(thinking)?;
            }
            let [uncached, cached, written, output, thinking] = totals;
            let input = uncached.checked_add(cached)?.checked_add(written)?;
            Some(TokenUsage {
                input_tokens: input,
                cached_input_tokens: cached,
                cache_write_input_tokens: written,
                output_tokens: output,
                // Thinking is a subset of output, not additional billable tokens.
                reasoning_output_tokens: thinking,
                total_tokens: input.checked_add(output)?,
                codex_rollout_budget_units: None,
            })
        })();
        if delta.is_none() {
            self.usage_baseline = None;
        }
        // Keep a valid snapshot even when legacy reasoning data needs a new baseline.
        delta.filter(|_| reasoning_available)
    }
}

#[derive(Default)]
pub(crate) struct RuntimeSlot {
    pub(crate) authentication_required: std::sync::atomic::AtomicBool,
    state: std::sync::Mutex<SlotState>,
    returned: tokio::sync::Notify,
}

#[derive(Default)]
struct SlotState {
    runtime: Option<ExternalSession>,
    leased: bool,
    discard_on_return: bool,
    fork: Option<TemporaryFork>,
}

impl RuntimeSlot {
    pub(crate) fn seed_fork(&self, seed: ForkSeed) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .fork = Some(TemporaryFork::Pending(seed));
    }

    /// Reports a parent busy with an ASM-owned turn or live native background work.
    pub(super) fn is_busy(&self) -> bool {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.leased
            || state.runtime.as_ref().is_some_and(|session| {
                session.checkpoint.in_flight
                    || match &session.runtime {
                        RuntimeProcess::Claude(runtime) => runtime.has_background_work(),
                        RuntimeProcess::Cursor(_) => false,
                    }
            })
    }

    pub(super) async fn take_for_shutdown(&self) -> anyhow::Result<Option<ExternalSession>> {
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
                    "native runtime did not return from its aborted turn within five seconds"
                )
            }
        }
    }
}

/// Returns the process on success, error, or forced destruction of its turn future.
pub(super) struct RuntimeLease<'a> {
    slot: &'a RuntimeSlot,
    pub(super) runtime: Option<ExternalSession>,
}

impl<'a> RuntimeLease<'a> {
    /// Claims the seed of a temporary fork once; its native session cannot be started again.
    pub(super) fn start_fork(&self) -> anyhow::Result<Option<ForkSeed>> {
        let mut state = self
            .slot
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match state.fork.replace(TemporaryFork::Started) {
            Some(TemporaryFork::Pending(seed)) => Ok(Some(seed)),
            Some(TemporaryFork::Started) => anyhow::bail!(
                "the temporary native side process ended and cannot be resumed; close it and start a new side conversation"
            ),
            None => {
                state.fork = None;
                Ok(None)
            }
        }
    }

    pub(super) fn take(slot: &'a RuntimeSlot) -> anyhow::Result<Self> {
        let mut state = slot
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("native session lock was poisoned"))?;
        anyhow::ensure!(!state.leased, "native runtime is already in use");
        anyhow::ensure!(
            !state.discard_on_return,
            "native runtime shutdown timed out; start a new chat"
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
