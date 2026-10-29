use crate::ExternalTurnOutcome;
use crate::InputBlock;
use crate::Prompt;
use crate::frames;
use crate::process::ExternalProcess;
use crate::process::STOP_TIMEOUT;
use crate::process::TurnGuard;
use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::sync::watch;
use tokio::time::Instant;
use tokio::time::sleep_until;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

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

/// Applies ASM child startup policy without changing native root or side sessions.
pub enum ClaudeSessionKind {
    NativeDefault,
    ManagedChild,
}

pub struct ClaudeOptions {
    pub executable: PathBuf,
    pub cwd: PathBuf,
    pub model: String,
    pub effort: Option<String>,
    /// Native compaction window, not the model's maximum context capacity.
    pub auto_compact_window: Option<i64>,
    /// Selected role instructions appended to the native system prompt, including on resume.
    pub role_instructions: Option<String>,
    pub session_kind: ClaudeSessionKind,
    pub session: ClaudeSession,
    pub bridge_directory: Option<PathBuf>,
    /// Native tool names removed from this session, such as questions for headless children.
    pub disallowed_tools: Vec<String>,
}

pub struct ClaudeRuntime {
    process: ExternalProcess,
    session_id: String,
    usable: bool,
    failure_result: Option<Value>,
    mid_turn_delivery: watch::Sender<bool>,
    // Native reports this level per process; live tasks can start another native turn.
    background_work: bool,
    // Last top-level assistant message of the latest turn, for pinned native forks.
    last_assistant_uuid: Option<String>,
    per_turn_effort_active: Option<bool>,
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
                .env("ASM_NATIVE_DESCRIPTOR", directory.join("descriptor.json"))
                .env("ASM_NATIVE_EXE", std::env::current_exe()?);
        }
        let mut flag_env = serde_json::Map::new();
        if matches!(options.session_kind, ClaudeSessionKind::ManagedChild) {
            command
                .args(["--system-prompt-snapshot", "off"])
                .env("FORCE_PROMPT_CACHING_5M", "1");
            flag_env.insert("FORCE_PROMPT_CACHING_5M".to_owned(), json!("1"));
        }
        if let Some(instructions) = options.role_instructions {
            command.arg("--append-system-prompt").arg(instructions);
        }
        if let Some(window) = options.auto_compact_window {
            let window = window.to_string();
            command.env("CLAUDE_CODE_AUTO_COMPACT_WINDOW", &window);
            flag_env.insert("CLAUDE_CODE_AUTO_COMPACT_WINDOW".to_owned(), json!(window));
            command.arg("--autocompact").arg(window);
        }
        if !flag_env.is_empty() {
            // Settings can reapply env after startup; flag settings beat user/project settings.
            // Native managed policy still has precedence.
            command
                .arg("--settings")
                .arg(json!({"env": flag_env}).to_string());
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
            process: ExternalProcess::spawn(command, frames::FramePolicy::ClaudeImages)
                .context("starting official Claude CLI")?,
            session_id,
            usable: true,
            failure_result: None,
            mid_turn_delivery: watch::channel(false).0,
            background_work: false,
            last_assistant_uuid: None,
            per_turn_effort_active: None,
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
        let mut snapshot = self.process.diagnostic_snapshot(&self.failure_result);
        snapshot["per_turn_effort_active"] = json!(self.per_turn_effort_active);
        snapshot
    }

    /// Change effort without replacing the native process or conversation.
    #[tracing::instrument(skip_all)]
    pub async fn set_effort(&mut self, effort: Option<&str>) -> Result<()> {
        anyhow::ensure!(
            self.usable && !self.background_work,
            "Claude effort can change only after a clean idle turn"
        );
        self.usable = false;
        let mut guard = TurnGuard {
            pid: self.process.child.id(),
            armed: true,
            marker: self.process.marker.clone(),
            birth: self.process.birth,
        };
        let result = timeout(Duration::from_secs(/*secs*/ 15), async {
            for request in [
                json!({"subtype": "apply_flag_settings", "settings": {"effortLevel": effort}}),
                json!({"subtype": "get_settings"}),
            ] {
                let confirm = request["subtype"] == "get_settings";
                let id = Uuid::new_v4().to_string();
                self.process
                    .write_frame(json!({
                        "type": "control_request", "request_id": id, "request": request,
                    }))
                    .await?;
                let frame = self
                    .process
                    .frames
                    .recv()
                    .await
                    .context("Claude closed during effort change")??;
                self.failure_result = Some(frame.clone());
                anyhow::ensure!(
                    frame["type"] == "control_response" && frame["response"]["request_id"] == id,
                    "Claude emitted unexpected activity during effort change"
                );
                anyhow::ensure!(
                    frame["response"]["subtype"] == "success",
                    "Claude rejected effort change: {}",
                    frame["response"]["error"]
                );
                if confirm && let Some(effort) = effort {
                    anyhow::ensure!(
                        frame["response"]["response"]["applied"]["effort"] == effort,
                        "Claude did not confirm requested effort `{effort}`"
                    );
                }
            }
            Ok::<(), anyhow::Error>(())
        })
        .await
        .context("Claude effort change timed out")
        .and_then(std::convert::identity);
        if result.is_ok() {
            self.usable = true;
            self.failure_result = None;
        } else {
            self.process.kill().await;
        }
        guard.armed = false;
        result
    }

    #[tracing::instrument(skip_all)]
    pub async fn run_turn(
        &mut self,
        prompt: &Prompt,
        events: mpsc::Sender<Value>,
        cancel: CancellationToken,
        steering: Option<mpsc::Receiver<Prompt>>,
    ) -> Result<ExternalTurnOutcome> {
        if !self.usable {
            bail!("Claude session requires explicit recovery after an unfinished turn");
        }
        if cancel.is_cancelled() {
            bail!("Claude turn cancelled before submission");
        }
        validate_prompt(prompt)?;
        self.usable = false;
        self.failure_result = None;
        self.last_assistant_uuid = None;
        let mut guard = TurnGuard {
            pid: self.process.child.id(),
            armed: true,
            marker: self.process.marker.clone(),
            birth: self.process.birth,
        };
        let user_message_uuid = Uuid::new_v4().to_string();
        let result = async {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    bail!("Claude turn cancelled during submission; explicit recovery is required");
                }
                result = self.process.write_frame(user_frame(prompt, &user_message_uuid)) => result?,
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
            self.process.kill().await;
        }
        guard.armed = false;
        result
    }

    async fn read_result(
        &mut self,
        events: &mpsc::Sender<Value>,
        cancel: CancellationToken,
        user_message_uuid: &str,
        mut steering: Option<mpsc::Receiver<Prompt>>,
    ) -> Result<ExternalTurnOutcome> {
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
                    timeout(STOP_TIMEOUT, self.process.write_frame(user_frame(&prompt, &command_uuid)))
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
            if frame["type"] == "system"
                && matches!(
                    frame["subtype"].as_str(),
                    Some("init" | "per_turn_effort_changed")
                )
                && let Some(active) = frame["per_turn_effort_active"].as_bool()
            {
                self.per_turn_effort_active = Some(active);
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
                    return Ok(ExternalTurnOutcome {
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
                return Ok(ExternalTurnOutcome {
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
        self.process.shutdown().await
    }
}

pub fn validate_prompt(prompt: &Prompt) -> Result<()> {
    // UUIDs emitted by the adapter always have this fixed width.
    let frame = user_frame(prompt, "00000000-0000-0000-0000-000000000000");
    let limit = frames::FramePolicy::ClaudeImages.input_limit(&frame);
    anyhow::ensure!(
        serde_json::to_vec(&frame)?.len() < limit,
        "Claude input exceeds the native stream-json frame limit ({limit} bytes); reduce the image or prompt size"
    );
    Ok(())
}

fn user_frame(prompt: &Prompt, uuid: &str) -> Value {
    let content = match prompt {
        Prompt::Text(text) => json!(text),
        Prompt::Blocks(blocks) => Value::Array(blocks.iter().map(|block| match block {
            InputBlock::Text { text } => json!({"type": "text", "text": text}),
            InputBlock::Image { mime_type, data } => json!({
                "type": "image", "source": {"type": "base64", "media_type": mime_type, "data": data}
            }),
        }).collect()),
    };
    json!({"type":"user", "uuid":uuid, "message":{"role":"user","content":content}})
}
