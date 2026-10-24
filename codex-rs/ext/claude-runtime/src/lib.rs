//! Run persistent official Claude and Cursor CLI sessions.
//! The native CLI owns tools and configuration. Callers serialize turns.

mod cursor;
mod frames;

pub use cursor::CursorOptions;
pub use cursor::CursorRuntime;
pub use cursor::CursorSession;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use codex_utils_pty::process_group;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Child;
use tokio::process::ChildStdin;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio::time::sleep_until;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const STOP_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 5);

/// `Fork` is an unsaved native copy of `parent` up to and including message `at`, under `id`;
/// it cannot resume after its process ends.
pub enum ClaudeSession {
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

pub struct ClaudeOptions {
    pub executable: PathBuf,
    pub cwd: PathBuf,
    pub model: String,
    pub effort: Option<String>,
    /// Native compaction window, not the model's maximum context capacity.
    pub auto_compact_window: Option<i64>,
    pub session: ClaudeSession,
    pub bridge_directory: Option<PathBuf>,
    /// Native tool names removed from this session, such as questions for headless children.
    pub disallowed_tools: Vec<String>,
}

pub struct ClaudeTurnOutcome {
    pub result: Option<Value>,
    pub interrupted: bool,
    pub submitted_steering_count: usize,
}

pub struct ClaudeRuntime {
    process: NativeProcess,
    session_id: String,
    usable: bool,
    failure_result: Option<Value>,
    mid_turn_delivery: watch::Sender<bool>,
    // Native reports this level per process; live tasks can start another native turn.
    background_work: bool,
    // Last top-level assistant message of the latest turn, for pinned native forks.
    last_assistant_uuid: Option<String>,
}

/// Native CLI process leading its own process group, with framed stdout and a bounded stderr tail.
struct NativeProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    frames: mpsc::Receiver<Result<Value>>,
    stdout_reader: JoinHandle<()>,
    stderr_reader: Option<JoinHandle<()>>,
    stderr: Arc<Mutex<VecDeque<u8>>>,
    exit_status: Option<String>,
}

// A dropped turn future must not leave a live agent executing outside ASM's task lifetime.
struct TurnGuard {
    pid: Option<u32>,
    armed: bool,
    kill: fn(Option<u32>),
}

impl Drop for TurnGuard {
    fn drop(&mut self) {
        if self.armed {
            (self.kill)(self.pid);
        }
    }
}

fn kill_group(pid: Option<u32>) {
    if let Some(pid) = pid
        && let Err(error) = process_group::kill_process_group(pid)
    {
        tracing::warn!(pid, %error, "failed to kill native CLI process group");
    }
}

fn execution_abort_reason(result: &Value) -> Option<&str> {
    if result.get("is_error").and_then(Value::as_bool) == Some(true)
        && result.get("subtype").and_then(Value::as_str) == Some("error_during_execution")
    {
        result.get("terminal_reason").and_then(Value::as_str)
    } else {
        None
    }
}

impl ClaudeRuntime {
    #[tracing::instrument(skip_all)]
    pub async fn spawn(options: ClaudeOptions) -> Result<Self> {
        if !cfg!(any(target_os = "linux", target_os = "macos")) {
            bail!("Claude runtime currently supports Linux and macOS only");
        }
        if let Some(window) = options.auto_compact_window {
            anyhow::ensure!(
                (100_000..=1_000_000).contains(&window),
                "Claude auto-compaction window must be between 100000 and 1000000 tokens"
            );
        }
        let (session_id, session_flag, fork_parent) = match options.session {
            ClaudeSession::New { id } => (id, "--session-id", None),
            ClaudeSession::Resume { id } => (id, "--resume", None),
            ClaudeSession::Fork { parent, at, id } => (id, "--session-id", Some((parent, at))),
        };
        Uuid::parse_str(&session_id).context("Claude session ID must be a UUID")?;
        let mut command = Command::new(options.executable);
        if let Some((parent, at)) = fork_parent {
            Uuid::parse_str(&parent).context("Claude parent session ID must be a UUID")?;
            Uuid::parse_str(&at).context("Claude fork message ID must be a UUID")?;
            // The native cutoff pins the fork; Claude refuses to start if `at` is not in the chain.
            command
                .arg("--resume")
                .arg(parent)
                .arg("--resume-session-at")
                .arg(at)
                .args(["--fork-session", "--no-session-persistence"]);
        }
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
            // Run in native bypass mode. Native deny rules and managed policy still apply;
            // no permission host is attached, so anything that still prompts is denied.
            .arg("--dangerously-skip-permissions")
            .args(["--permission-prompts", "none"])
            .arg("--model")
            .arg(options.model)
            .arg(session_flag)
            .arg(&session_id);
        if let Some(directory) = &options.bridge_directory {
            command
                .arg("--plugin-dir")
                .arg(directory)
                .env("ASM_NATIVE_DESCRIPTOR", directory.join("descriptor.json"));
        }
        if let Some(window) = options.auto_compact_window {
            // Settings can reapply env after startup; flag settings beat user/project settings.
            // Native managed policy still has precedence.
            let window = window.to_string();
            command.env("CLAUDE_CODE_AUTO_COMPACT_WINDOW", &window);
            command
                .arg("--settings")
                .arg(json!({"env":{"CLAUDE_CODE_AUTO_COMPACT_WINDOW":window}}).to_string());
            command.arg("--autocompact").arg(window);
        }
        if let Some(effort) = options.effort {
            command.arg("--effort").arg(effort);
        }
        if !options.disallowed_tools.is_empty() {
            command
                .arg("--disallowedTools")
                .arg(options.disallowed_tools.join(","));
        }
        Ok(Self {
            process: NativeProcess::spawn(command).context("starting official Claude CLI")?,
            session_id,
            usable: true,
            failure_result: None,
            mid_turn_delivery: watch::channel(false).0,
            background_work: false,
            last_assistant_uuid: None,
        })
    }

    /// Whether native background tasks were live at the last observed frame.
    pub fn has_background_work(&self) -> bool {
        self.background_work
    }

    /// Final top-level assistant message of the latest turn; a fork at it excludes later turns.
    pub fn fork_point(&self) -> Option<&str> {
        self.last_assistant_uuid.as_deref()
    }

    pub fn subscribe_mid_turn_delivery(&self) -> watch::Receiver<bool> {
        self.mid_turn_delivery.subscribe()
    }

    /// These bounded diagnostics can contain private information.
    /// Save them only when private diagnostic capture is enabled.
    pub fn diagnostic_snapshot(&self) -> Value {
        self.process.diagnostic_snapshot(&self.failure_result)
    }

    #[tracing::instrument(skip_all)]
    pub async fn run_turn(
        &mut self,
        prompt: &str,
        events: mpsc::Sender<Value>,
        cancel: CancellationToken,
        steering: Option<mpsc::Receiver<String>>,
    ) -> Result<ClaudeTurnOutcome> {
        if !self.usable {
            bail!("Claude session requires explicit recovery after an unfinished turn");
        }
        if cancel.is_cancelled() {
            bail!("Claude turn cancelled before submission");
        }
        self.usable = false;
        self.failure_result = None;
        self.last_assistant_uuid = None;
        let mut guard = TurnGuard {
            pid: self.process.child.id(),
            armed: true,
            kill: kill_group,
        };
        let user_message_uuid = Uuid::new_v4().to_string();
        let result = async {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    bail!("Claude turn cancelled during submission; explicit recovery is required");
                }
                result = self.process.write_frame(json!({"type":"user", "uuid":user_message_uuid,
                    "message":{"role":"user","content":prompt}})) => result?,
            }
            self.read_result(&events, cancel, &user_message_uuid, steering)
                .await
        }
        .await;
        if result.is_ok() {
            self.usable = true;
        }
        if !self.usable {
            // Preserve the original turn error even if cleanup fails.
            self.process.kill(kill_group).await;
        }
        guard.armed = false;
        result
    }

    async fn read_result(
        &mut self,
        events: &mpsc::Sender<Value>,
        cancel: CancellationToken,
        user_message_uuid: &str,
        mut steering: Option<mpsc::Receiver<String>>,
    ) -> Result<ClaudeTurnOutcome> {
        let mut pending_result = None;
        let mut latest_command = user_message_uuid.to_owned();
        let mut commands = HashSet::from([latest_command.clone()]);
        let mut submitted_commands = commands.clone();
        let mut latest_command_succeeded = false;
        let mut cancelled_commands = HashSet::new();
        let mut submitted_steering_count = 0;
        let mut interrupt_id = None;
        let mut interrupt_deadline = None;
        let mut interrupt_acknowledged = false;
        let mut interrupted_commands = HashSet::new();
        let mut idle = false;
        // Local agents by task ID, with the latest run ID when Claude reports one.
        let mut active_agents: HashMap<String, Option<String>> = HashMap::new();
        loop {
            let frame = tokio::select! {
                biased;
                _ = cancel.cancelled(), if interrupt_id.is_none() => {
                    let request_id = Uuid::new_v4().to_string();
                    let deadline = Instant::now() + STOP_TIMEOUT;
                    timeout(STOP_TIMEOUT, self.process.write_frame(json!({
                        "type":"control_request", "request_id":request_id,
                        "request":{"subtype":"interrupt"}
                    }))).await.context("Claude interrupt submission timed out")??;
                    interrupt_id = Some(request_id);
                    interrupt_deadline = Some(deadline);
                    idle = false;
                    continue;
                }
                _ = async {
                    if let Some(deadline) = interrupt_deadline {
                        sleep_until(deadline).await;
                    } else {
                        std::future::pending::<()>().await;
                    }
                } => bail!("Claude interrupt acknowledgement and idle timed out"),
                prompt = async {
                    match steering.as_mut() {
                        Some(receiver) => receiver.recv().await,
                        None => std::future::pending().await,
                    }
                }, if interrupt_id.is_none() => {
                    let Some(prompt) = prompt else {
                        steering = None;
                        continue;
                    };
                    if !*self.mid_turn_delivery.borrow() {
                        bail!("Claude steering was submitted without command lifecycle support");
                    }
                    let command_uuid = Uuid::new_v4().to_string();
                    // No Send now or interrupt: native queues the message and folds it in at
                    // its next tool boundary, so running tools finish instead of being rejected.
                    // Persisted input is never replayed if the write fails partway through.
                    timeout(STOP_TIMEOUT, self.process.write_frame(json!({"type":"user",
                        "uuid":command_uuid, "message":{"role":"user","content":prompt}})))
                    .await
                    .context("Claude steering submission timed out")??;
                    commands.insert(command_uuid.clone());
                    submitted_commands.insert(command_uuid.clone());
                    latest_command = command_uuid;
                    latest_command_succeeded = false;
                    submitted_steering_count += 1;
                    pending_result = None;
                    idle = false;
                    continue;
                }
                frame = self.process.frames.recv() => frame.context("Claude stream reader stopped")??,
            };
            if !frame.is_object() || frame.get("type").and_then(Value::as_str).is_none() {
                bail!("Claude emitted a JSON frame without an event type");
            }
            if let Some(id) = frame.get("session_id").and_then(Value::as_str)
                && id != self.session_id
            {
                bail!("Claude returned a different session ID than requested");
            }
            if frame.get("type").and_then(Value::as_str) == Some("system")
                && frame.get("subtype").and_then(Value::as_str) == Some("init")
            {
                // Send now is never requested. This capability only identifies the verified
                // CLI generation whose UUID command lifecycle lets queued input be reconciled.
                self.mid_turn_delivery.send_replace(
                    frame
                        .get("capabilities")
                        .and_then(Value::as_array)
                        .is_some_and(|items| {
                            items
                                .iter()
                                .any(|item| item.as_str() == Some("interrupt_send_now_v1"))
                        }),
                );
            }
            if frame.get("type").and_then(Value::as_str) == Some("command_lifecycle")
                && let Some(id) = frame.get("command_uuid").and_then(Value::as_str)
                && submitted_commands.contains(id)
            {
                match frame.get("state").and_then(Value::as_str) {
                    Some("cancelled") => {
                        cancelled_commands.insert(id.to_owned());
                        commands.remove(id);
                    }
                    Some("completed") => {
                        commands.remove(id);
                        if id == latest_command {
                            latest_command_succeeded = true;
                        }
                    }
                    _ => {}
                }
            }
            if frame.get("type").and_then(Value::as_str) == Some("control_request") {
                self.failure_result = Some(frame.clone());
                if let Some(request_id) = frame.get("request_id").and_then(Value::as_str) {
                    self.process.write_frame(json!({
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
            if frame.get("type").and_then(Value::as_str) == Some("control_response")
                && let Some(response) = frame.get("response")
                && interrupt_id.as_deref().is_some_and(|id| {
                    response.get("request_id").and_then(Value::as_str) == Some(id)
                })
            {
                if response.get("subtype").and_then(Value::as_str) != Some("success") {
                    self.failure_result = Some(frame);
                    bail!("Claude refused the interrupt request");
                }
                // Queued commands survive interrupt. UUID-stamped inputs make the
                // native acknowledgement report them instead of silently omitting them.
                if !response
                    .pointer("/response/still_queued")
                    .and_then(Value::as_array)
                    .is_some_and(Vec::is_empty)
                {
                    self.failure_result = Some(frame);
                    bail!("Claude interrupt did not confirm an empty input queue");
                }
                interrupt_acknowledged = true;
            }
            if frame.get("type").and_then(Value::as_str) == Some("system") {
                let subtype = frame.get("subtype").and_then(Value::as_str);
                if subtype == Some("task_started")
                    && frame.get("task_type").and_then(Value::as_str) == Some("local_agent")
                {
                    let task_id = frame
                        .get("task_id")
                        .and_then(Value::as_str)
                        .context("Claude local agent started without a task ID")?;
                    let run = frame.get("run_id").and_then(Value::as_str);
                    // A resumed task keeps its ID with a new, later-sorting run ID. Late or
                    // run-less starts must not replace a known newer run.
                    let known = active_agents.get(task_id).cloned().flatten();
                    if known.is_none() || run.is_some_and(|run| Some(run) >= known.as_deref()) {
                        active_agents.insert(task_id.to_owned(), run.map(str::to_owned));
                    }
                }
                if subtype == Some("task_notification")
                    && matches!(
                        frame.get("status").and_then(Value::as_str),
                        Some("stopped" | "completed" | "failed" | "killed")
                    )
                    && let Some(task_id) = frame.get("task_id").and_then(Value::as_str)
                    && let Some(known) = active_agents.get(task_id)
                {
                    // A run-less or older-run terminal cannot prove that a known newer run stopped.
                    let run = frame.get("run_id").and_then(Value::as_str);
                    if known.is_none() || run.is_some_and(|run| Some(run) >= known.as_deref()) {
                        active_agents.remove(task_id);
                    }
                }
                if subtype == Some("background_tasks_changed") {
                    self.background_work = frame
                        .get("tasks")
                        .and_then(Value::as_array)
                        .context("Claude reported background tasks without a task list")?
                        .iter()
                        .any(|task| task.get("ambient").and_then(Value::as_bool) != Some(true));
                }
                if subtype == Some("session_state_changed") {
                    // Idle with live background work is not final: its notification starts
                    // another native turn. Interrupts keep their separate idle confirmation.
                    idle = frame.get("state").and_then(Value::as_str) == Some("idle")
                        && (interrupt_id.is_some() || !self.background_work);
                }
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
                idle = false;
                if frame.get("type").and_then(Value::as_str) == Some("assistant") {
                    self.last_assistant_uuid =
                        frame.get("uuid").and_then(Value::as_str).map(str::to_owned);
                }
            }
            let send = events.send(frame.clone());
            if let Some(deadline) = interrupt_deadline {
                tokio::time::timeout_at(deadline, send)
                    .await
                    .context("Claude interrupt event delivery timed out")?
                    .context("Claude event consumer closed")?;
            } else {
                send.await.context("Claude event consumer closed")?;
            }
            if is_result {
                if frame.get("session_id").and_then(Value::as_str) != Some(self.session_id.as_str())
                {
                    bail!("Claude result did not confirm the requested session ID");
                }
                let abort_reason = execution_abort_reason(&frame);
                let interrupted_execution = interrupt_id.is_some()
                    && matches!(abort_reason, Some("aborted_streaming" | "aborted_tools"))
                    && frame
                        .get("user_message_uuid")
                        .and_then(Value::as_str)
                        .is_some_and(|id| commands.contains(id) || cancelled_commands.contains(id));
                let result_command = frame.get("user_message_uuid").and_then(Value::as_str);
                if interrupted_execution && let Some(id) = result_command {
                    interrupted_commands.insert(id.to_owned());
                }
                if !interrupted_execution
                    && (frame.get("is_error").and_then(Value::as_bool) != Some(false)
                        || frame.get("subtype").and_then(Value::as_str) != Some("success"))
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
                if result_command == Some(latest_command.as_str()) && !interrupted_execution {
                    latest_command_succeeded = true;
                }
                // A notification turn can run while ASM is between turns, for example after
                // an interrupt leaves a background shell live. Accept one only after this turn's
                // own input succeeded, so a stale notification never answers new input.
                let notification = frame.pointer("/origin/kind").and_then(Value::as_str)
                    == Some("task-notification");
                if (submitted_steering_count == 0 && !notification)
                    || (result_command.is_some_and(|id| submitted_commands.contains(id))
                        && !interrupted_execution)
                    || (result_command.is_none()
                        && notification
                        && latest_command_succeeded
                        && commands.is_empty())
                {
                    pending_result = Some(frame.clone());
                }
                if let Some(id) = result_command {
                    commands.remove(id);
                }
            }
            // A result ends one generation, not necessarily the native agent's run.
            // Require explicit idle before the next ASM turn can consume this stream.
            if interrupt_id.is_some() {
                if is_idle && !active_agents.is_empty() {
                    bail!("Claude became idle before interrupted local agents stopped");
                }
                if interrupt_acknowledged
                    && idle
                    && active_agents.is_empty()
                    && interrupted_commands.is_subset(&cancelled_commands)
                {
                    return Ok(ClaudeTurnOutcome {
                        result: pending_result,
                        interrupted: true,
                        submitted_steering_count,
                    });
                }
            } else if idle
                && (submitted_steering_count == 0
                    || (latest_command_succeeded && commands.is_empty()))
                && let Some(result) = pending_result.take()
            {
                return Ok(ClaudeTurnOutcome {
                    result: Some(result),
                    interrupted: false,
                    submitted_steering_count,
                });
            }
        }
    }

    #[tracing::instrument(skip_all)]
    pub async fn shutdown(&mut self) -> Result<()> {
        self.usable = false;
        let result = self.process.shutdown().await;
        result
    }
}

impl NativeProcess {
    /// Starts `command` as the leader of a new process group that dies with ASM.
    fn spawn(mut command: Command) -> Result<Self> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(/*kill_on_drop*/ true);
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
        let mut child = command.spawn()?;
        let stdin = child.stdin.take().context("native CLI stdin unavailable")?;
        let stdout = child
            .stdout
            .take()
            .context("native CLI stdout unavailable")?;
        let stderr_pipe = child
            .stderr
            .take()
            .context("native CLI stderr unavailable")?;
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
            exit_status: None,
        })
    }

    fn diagnostic_snapshot(&self, failure_result: &Option<Value>) -> Value {
        let stderr = self
            .stderr
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .copied()
            .collect::<Vec<_>>();
        json!({
            "stderr_tail": String::from_utf8_lossy(&stderr),
            "failure_result": failure_result,
            "exit_status": self.exit_status,
        })
    }

    async fn write_frame(&mut self, frame: Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(&frame)?;
        if bytes.len() > frames::FRAME_LIMIT {
            bail!("native CLI input frame exceeded size limit");
        }
        bytes.push(b'\n');
        let stdin = self.stdin.as_mut().context("native CLI stdin is closed")?;
        stdin
            .write_all(&bytes)
            .await
            .context("writing native CLI input")?;
        stdin.flush().await.context("flushing native CLI input")
    }

    /// Kills the owned process tree with `kill` and records the leader's exit status.
    async fn kill(&mut self, kill: fn(Option<u32>)) {
        let pid = self.child.id();
        let exited = self.child.try_wait();
        // try_wait can reap the leader and clear child.id(), while helpers
        // still hold its process group. Keep the original group ID for cleanup.
        kill(pid);
        if let Ok(Some(status)) = exited {
            self.exit_status = Some(status.to_string());
        } else if let Ok(Ok(status)) = timeout(STOP_TIMEOUT, self.child.wait()).await {
            self.exit_status = Some(status.to_string());
        }
    }

    async fn shutdown(&mut self) -> Result<()> {
        self.stdin.take();
        let pid = self.child.id();
        let result = timeout(STOP_TIMEOUT, self.child.wait()).await;
        // Also stop group descendants after an apparently clean leader exit.
        kill_group(pid);
        match result {
            Ok(status) => {
                self.exit_status = Some(status.context("waiting for native CLI exit")?.to_string());
            }
            Err(_) => {
                let status = timeout(STOP_TIMEOUT, self.child.wait())
                    .await
                    .context("native CLI did not exit after termination")??;
                self.exit_status = Some(status.to_string());
            }
        }
        self.wait_for_stderr().await;
        Ok(())
    }

    /// Lets stderr finish before callers capture diagnostics. Keeps the task owned
    /// while awaiting, so cancellation still lets Drop stop it.
    async fn wait_for_stderr(&mut self) {
        if let Some(reader) = self.stderr_reader.as_mut() {
            if timeout(STOP_TIMEOUT, &mut *reader).await.is_err() {
                reader.abort();
            }
            self.stderr_reader.take();
        }
    }
}

impl Drop for NativeProcess {
    fn drop(&mut self) {
        kill_group(self.child.id());
        self.stdout_reader.abort();
        if let Some(reader) = &self.stderr_reader {
            reader.abort();
        }
    }
}
