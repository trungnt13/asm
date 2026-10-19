//! Run persistent official Claude CLI sessions.
//! Claude owns tools and configuration. Callers serialize turns.

mod frames;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use codex_utils_pty::process_group;
use serde_json::Value;
use serde_json::json;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;
use tokio::process::Child;
use tokio::process::ChildStdin;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const STOP_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 5);

pub enum ClaudeSession {
    New { id: String },
    Resume { id: String },
}

pub struct ClaudeOptions {
    pub executable: PathBuf,
    pub cwd: PathBuf,
    pub model: String,
    pub effort: Option<String>,
    pub session: ClaudeSession,
    pub mcp_config: Option<Value>,
}

pub struct ClaudeRuntime {
    child: Child,
    stdin: Option<ChildStdin>,
    frames: mpsc::Receiver<Result<Value>>,
    stdout_reader: JoinHandle<()>,
    stderr_reader: Option<JoinHandle<()>>,
    stderr: Arc<Mutex<VecDeque<u8>>>,
    session_id: String,
    mcp_config: Option<NamedTempFile>,
    usable: bool,
    failure_result: Option<Value>,
    exit_status: Option<String>,
}

// A dropped turn future must not leave a live agent executing outside ASM's task lifetime.
struct TurnGuard {
    pid: Option<u32>,
    armed: bool,
}

impl Drop for TurnGuard {
    fn drop(&mut self) {
        if self.armed {
            kill_group(self.pid);
        }
    }
}

fn kill_group(pid: Option<u32>) {
    if let Some(pid) = pid
        && let Err(error) = process_group::kill_process_group(pid)
    {
        tracing::warn!(pid, %error, "failed to kill Claude process group");
    }
}

impl ClaudeRuntime {
    #[tracing::instrument(skip_all)]
    pub async fn spawn(options: ClaudeOptions) -> Result<Self> {
        if !cfg!(any(target_os = "linux", target_os = "macos")) {
            bail!("Claude runtime currently supports Linux and macOS only");
        }
        let (session_id, session_flag) = match options.session {
            ClaudeSession::New { id } => (id, "--session-id"),
            ClaudeSession::Resume { id } => (id, "--resume"),
        };
        Uuid::parse_str(&session_id).context("Claude session ID must be a UUID")?;
        let mut command = Command::new(options.executable);
        // Keep login in the official CLI, without accidental API-key or cloud-provider billing.
        for name in [
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_BASE_URL",
            "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CODE_USE_VERTEX",
            "CLAUDE_CODE_USE_FOUNDRY",
        ] {
            command.env_remove(name);
        }
        command
            // Official SDK opt-in: idle follows held-back results and native background agents.
            .env("CLAUDE_CODE_SDK_READS_SESSION_STATE", "1")
            .current_dir(options.cwd)
            .args([
                "-p",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
            ])
            .args(["--verbose", "--include-partial-messages"])
            // No permission host is attached. Native allow/deny rules still apply,
            // but a tool that requires a new approval must be denied, not granted.
            .args(["--permission-prompts", "none"])
            .arg("--model")
            .arg(options.model)
            .arg(session_flag)
            .arg(&session_id)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(/*kill_on_drop*/ true);
        // Keep credentials out of argv. The owner-only file is removed on spawn
        // failure or runtime teardown, after the child no longer needs it.
        let mcp_config = options
            .mcp_config
            .map(|config| -> Result<NamedTempFile> {
                let mut file =
                    NamedTempFile::new().context("creating private Claude MCP config")?;
                serde_json::to_writer(file.as_file_mut(), &config)
                    .context("writing private Claude MCP config")?;
                Ok(file)
            })
            .transpose()?;
        if let Some(config) = &mcp_config {
            command.arg("--mcp-config").arg(config.path());
        }
        if let Some(effort) = options.effort {
            command.arg("--effort").arg(effort);
        }
        #[cfg(unix)]
        {
            let parent_pid = std::process::id() as i32;
            // SAFETY: only async-signal-safe system calls run in the child.
            unsafe {
                command.pre_exec(move || {
                    process_group::set_process_group()?;
                    process_group::set_parent_death_signal(parent_pid)
                });
            }
        }
        let mut child = command.spawn().context("starting official Claude CLI")?;
        let stdin = child.stdin.take().context("Claude stdin unavailable")?;
        let stdout = child.stdout.take().context("Claude stdout unavailable")?;
        let stderr_pipe = child.stderr.take().context("Claude stderr unavailable")?;
        let (sender, frames) = mpsc::channel(/*buffer*/ 32);
        let stderr = Arc::new(Mutex::new(VecDeque::new()));
        let stdout_reader = tokio::spawn(frames::read_frames(stdout, sender));
        let stderr_reader = Some(tokio::spawn(frames::drain_stderr(
            stderr_pipe,
            Arc::clone(&stderr),
        )));
        Ok(Self {
            child,
            stdin: Some(stdin),
            frames,
            stdout_reader,
            stderr_reader,
            stderr,
            session_id,
            mcp_config,
            usable: true,
            failure_result: None,
            exit_status: None,
        })
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// These bounded diagnostics can contain private information.
    /// Save them only when private diagnostic capture is enabled.
    pub fn diagnostic_snapshot(&self) -> Value {
        let stderr = self
            .stderr
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .copied()
            .collect::<Vec<_>>();
        json!({
            "stderr_tail": String::from_utf8_lossy(&stderr),
            "failure_result": self.failure_result,
            "exit_status": self.exit_status,
        })
    }

    #[tracing::instrument(skip_all)]
    pub async fn run_turn(
        &mut self,
        prompt: &str,
        events: mpsc::Sender<Value>,
        cancel: CancellationToken,
    ) -> Result<Value> {
        if !self.usable {
            bail!("Claude session requires explicit recovery after an unfinished turn");
        }
        if cancel.is_cancelled() {
            bail!("Claude turn cancelled before submission");
        }
        self.usable = false;
        self.failure_result = None;
        let mut guard = TurnGuard {
            pid: self.child.id(),
            armed: true,
        };
        let result = async {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    let interrupt = json!({"type":"control_request", "request_id":Uuid::new_v4().to_string(),
                        "request":{"subtype":"interrupt"}});
                    timeout(STOP_TIMEOUT, async {
                        self.write_frame(interrupt).await?;
                        self.read_result(&events).await
                    }).await.context("Claude interrupt acknowledgement timed out")??;
                    bail!("Claude turn interrupted; explicit recovery is required");
                }
                result = async {
                    self.write_frame(json!({"type":"user",
                        "message":{"role":"user","content":prompt}})).await?;
                    self.read_result(&events).await
                } => result,
            }
        }.await;
        if result.is_ok() {
            self.usable = true;
        }
        if self.usable {
            guard.armed = false;
        } else {
            // Preserve the original turn error even if cleanup fails.
            let pid = self.child.id();
            let exited = self.child.try_wait();
            // try_wait can reap the leader and clear child.id(), while helpers
            // still hold its process group. Keep the original group ID for cleanup.
            kill_group(pid);
            if let Ok(Some(status)) = exited {
                self.exit_status = Some(status.to_string());
            } else if let Ok(Ok(status)) = timeout(STOP_TIMEOUT, self.child.wait()).await {
                self.exit_status = Some(status.to_string());
            }
            guard.armed = false;
        }
        result
    }

    async fn write_frame(&mut self, frame: Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(&frame)?;
        if bytes.len() > frames::FRAME_LIMIT {
            bail!("Claude input frame exceeded size limit");
        }
        bytes.push(b'\n');
        let stdin = self.stdin.as_mut().context("Claude stdin is closed")?;
        stdin
            .write_all(&bytes)
            .await
            .context("writing Claude input")?;
        stdin.flush().await.context("flushing Claude input")
    }

    async fn read_result(&mut self, events: &mpsc::Sender<Value>) -> Result<Value> {
        let mut pending_result = None;
        loop {
            let frame = self
                .frames
                .recv()
                .await
                .context("Claude stream reader stopped")??;
            if !frame.is_object() || frame.get("type").and_then(Value::as_str).is_none() {
                bail!("Claude emitted a JSON frame without an event type");
            }
            if let Some(id) = frame.get("session_id").and_then(Value::as_str)
                && id != self.session_id
            {
                bail!("Claude returned a different session ID than requested");
            }
            if frame.get("type").and_then(Value::as_str) == Some("control_request") {
                self.failure_result = Some(frame.clone());
                if let Some(request_id) = frame.get("request_id").and_then(Value::as_str) {
                    self.write_frame(json!({
                        "type": "control_response",
                        "response": {
                            "subtype": "error",
                            "request_id": request_id,
                            "error": "ASM does not provide a Claude permission or control callback",
                        }
                    }))
                    .await
                    .context("refusing unsupported Claude control request")?;
                }
                bail!(
                    "Claude requested an unsupported approval or control callback; no permission was granted"
                );
            }
            let is_result = frame.get("type").and_then(Value::as_str) == Some("result");
            let is_idle = frame.get("type").and_then(Value::as_str) == Some("system")
                && frame.get("subtype").and_then(Value::as_str) == Some("session_state_changed")
                && frame.get("state").and_then(Value::as_str) == Some("idle");
            if matches!(
                frame.get("type").and_then(Value::as_str),
                Some("assistant" | "stream_event")
            ) && frame.get("parent_tool_use_id").is_none_or(Value::is_null)
            {
                pending_result = None;
            }
            events
                .send(frame.clone())
                .await
                .context("Claude event consumer closed")?;
            if is_result {
                if frame.get("session_id").and_then(Value::as_str) != Some(self.session_id.as_str())
                {
                    bail!("Claude result did not confirm the requested session ID");
                }
                if frame.get("is_error").and_then(Value::as_bool) != Some(false)
                    || frame.get("subtype").and_then(Value::as_str) != Some("success")
                {
                    // Only known categories enter the user error. Arbitrary provider prose can
                    // contain prompts or credentials and remains in opt-in private diagnostics.
                    let category = match frame.get("subtype").and_then(Value::as_str) {
                        Some("success") => "provider/API error",
                        Some("error_max_turns") => "maximum turns exceeded",
                        Some("error_max_budget_usd") => "budget exceeded",
                        Some("error_during_execution") => "execution error",
                        Some("error_max_structured_output_retries") => "structured output failed",
                        Some(_) | None => "unknown result category",
                    };
                    self.failure_result = Some(frame);
                    bail!("Claude returned an unsuccessful turn result: {category}");
                }
                pending_result = Some(frame);
            }
            // A result ends one generation, not necessarily the native agent's run.
            // Require explicit idle before the next ASM turn can consume this stream.
            if is_idle && let Some(result) = pending_result.take() {
                return Ok(result);
            }
        }
    }

    #[tracing::instrument(skip_all)]
    pub async fn shutdown(&mut self) -> Result<()> {
        self.usable = false;
        self.stdin.take();
        let pid = self.child.id();
        let result = timeout(STOP_TIMEOUT, self.child.wait()).await;
        // Also stop group descendants after an apparently clean leader exit.
        kill_group(pid);
        match result {
            Ok(status) => {
                self.exit_status = Some(status.context("waiting for Claude exit")?.to_string());
            }
            Err(_) => {
                let status = timeout(STOP_TIMEOUT, self.child.wait())
                    .await
                    .context("Claude did not exit after termination")??;
                self.exit_status = Some(status.to_string());
            }
        }
        self.mcp_config.take();
        // Let stderr finish before callers capture diagnostics. Keep the task owned
        // while awaiting, so cancellation still lets Drop stop it.
        if let Some(reader) = self.stderr_reader.as_mut() {
            if timeout(STOP_TIMEOUT, &mut *reader).await.is_err() {
                reader.abort();
            }
            self.stderr_reader.take();
        }
        Ok(())
    }
}

impl Drop for ClaudeRuntime {
    fn drop(&mut self) {
        kill_group(self.child.id());
        self.stdout_reader.abort();
        if let Some(reader) = &self.stderr_reader {
            reader.abort();
        }
    }
}
