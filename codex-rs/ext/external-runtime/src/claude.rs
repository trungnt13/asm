use crate::ExternalTurnFailure;
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
use std::io::Write;
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

/// Selects ASM root or child startup policy while preserving native side sessions.
pub enum ClaudeSessionKind {
    NativeDefault,
    ManagedRoot,
    ManagedChild,
}

pub struct ClaudeOptions {
    /// Counts host steering submissions not yet acknowledged by native replay echoes.
    /// Increment before enqueueing steering; the driver decrements after consumption.
    pub queued_input_count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub executable: PathBuf,
    pub cwd: PathBuf,
    pub model: String,
    pub effort: Option<String>,
    /// Native compaction window, not the model's maximum context capacity.
    pub auto_compact_window: Option<i64>,
    /// ASM system instructions, including the selected role, restored on resume.
    pub role_instructions: Option<String>,
    pub session_kind: ClaudeSessionKind,
    pub session: ClaudeSession,
    pub bridge_directory: Option<PathBuf>,
    /// Native tool names removed from this session, such as questions for headless children.
    pub disallowed_tools: Vec<String>,
    pub skill_overrides: serde_json::Map<String, Value>,
}

pub(crate) struct ClaudeDriver {
    pub(super) process: ExternalProcess,
    _system_prompt_file: tempfile::NamedTempFile,
    session_id: String,
    usable: bool,
    failure_result: Option<Value>,
    mid_turn_delivery: watch::Sender<bool>,
    queued_input_count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    // Native reports this level per process; live tasks can start another native turn.
    pub(super) background_work: bool,
    pub(super) first_frame: Option<Value>,
    active_agents: HashMap<String, Option<String>>,
    // Last top-level assistant message of the latest turn, for pinned native forks.
    last_assistant_uuid: Option<String>,
    per_turn_effort_active: Option<bool>,
    initialized: bool,
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

impl ClaudeDriver {
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
            "ANTHROPIC_PROFILE",
            "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CODE_USE_VERTEX",
            "CLAUDE_CODE_USE_FOUNDRY",
            "CLAUDE_CODE_USE_ANTHROPIC_AWS",
            "CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD",
            "CLAUDE_CODE_USE_MANTLE",
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
            .args([
                "--verbose",
                "--include-partial-messages",
                "--replay-user-messages",
                "--await-initialize",
            ])
            // Run in native bypass mode. Native deny rules and managed policy still apply;
            // no permission host is attached, so anything that still prompts is denied.
            .arg("--dangerously-skip-permissions")
            .args(["--permission-prompts", "none"])
            .arg("--model")
            .arg(options.model)
            .arg(session_flag)
            .arg(&session_id);
        let directory = options
            .bridge_directory
            .context("Claude requires the ASM tool bridge")?;
        command
            // An empty --tools also rejects plugin tools. Select, then deny, one built-in.
            .args([
                "--tools",
                "ToolSearch",
                "--disable-slash-commands",
                "--strict-mcp-config",
            ])
            .args([
                "--mcp-config",
                r#"{"mcpServers":{}}"#,
                "--setting-sources",
                "",
            ])
            .arg("--plugin-dir")
            .arg(&directory)
            .env("ASM_NATIVE_DESCRIPTOR", directory.join("descriptor.json"))
            .env("ASM_NATIVE_EXE", std::env::current_exe()?);
        let mut flag_env = serde_json::Map::new();
        for name in [
            "CLAUDE_CODE_DISABLE_CLAUDE_MDS",
            "CLAUDE_CODE_DISABLE_AUTO_MEMORY",
            "CLAUDE_CODE_DISABLE_BUNDLED_SKILLS",
            "CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS",
            "DISABLE_TELEMETRY",
        ] {
            command.env(name, "1");
            flag_env.insert(name.to_owned(), json!("1"));
        }
        let mut flag_settings = json!({
            "disableClaudeAiConnectors": true,
            "syncClaudeAiSkills": false,
            "syncClaudeAiPlugins": false,
            "autoMemoryEnabled": false,
            "spinnerTipsEnabled": false,
            "enabledPlugins": {
                "cc-plugin-agents-md@builtin": false,
                "cc-plugin-plugin-authoring@builtin": false,
                "cc-plugin-mods-guide@builtin": false,
                "cc-plugin-tips@builtin": false,
                "cc-plugin-you-should-know@builtin": false,
                "cc-plugin-privacy@builtin": false,
                "cc-plugin-telemetry@builtin": false,
            },
            "attribution": {"commit": "", "pr": ""},
        });
        if matches!(options.session_kind, ClaudeSessionKind::ManagedChild) {
            command
                .args(["--system-prompt-snapshot", "off"])
                .env("FORCE_PROMPT_CACHING_5M", "1");
            flag_env.insert("FORCE_PROMPT_CACHING_5M".to_owned(), json!("1"));
        }
        let system_prompt = options
            .role_instructions
            .context("Claude requires ASM system instructions")?;
        let mut system_prompt_file = tempfile::NamedTempFile::new_in(&directory)
            .context("creating Claude system prompt file")?;
        system_prompt_file
            .write_all(system_prompt.as_bytes())
            .context("writing Claude system prompt file")?;
        command
            .arg("--system-prompt-file")
            .arg(system_prompt_file.path());
        if let Some(window) = options.auto_compact_window {
            let window = window.to_string();
            command.env("CLAUDE_CODE_AUTO_COMPACT_WINDOW", &window);
            flag_env.insert("CLAUDE_CODE_AUTO_COMPACT_WINDOW".to_owned(), json!(window));
            command.arg("--autocompact").arg(window);
        }
        flag_settings["env"] = json!(flag_env);
        command.arg("--settings").arg(flag_settings.to_string());
        if let Some(effort) = options.effort {
            command.arg("--effort").arg(effort);
        }
        let mut disallowed_tools = options.disallowed_tools;
        disallowed_tools.push("ToolSearch".to_owned());
        command
            .arg("--disallowedTools")
            .arg(disallowed_tools.join(","));
        options
            .queued_input_count
            .store(/*val*/ 0, std::sync::atomic::Ordering::SeqCst);
        let mut driver = Self {
            process: ExternalProcess::spawn(command, frames::FramePolicy::ClaudeImages)
                .context("starting official Claude CLI")?,
            _system_prompt_file: system_prompt_file,
            session_id,
            usable: true,
            failure_result: None,
            mid_turn_delivery: watch::channel(false).0,
            queued_input_count: options.queued_input_count,
            background_work: false,
            first_frame: None,
            active_agents: HashMap::new(),
            last_assistant_uuid: None,
            per_turn_effort_active: None,
            initialized: false,
        };
        // Initialization runs bridge registration without submitting user input for inference.
        let initialized = timeout(Duration::from_secs(/*secs*/ 60), async {
            let id = Uuid::new_v4().to_string();
            driver
                .process
                .write_frame(json!({
                    "type": "control_request", "request_id": id,
                    "request": {"subtype": "initialize"},
                }))
                .await?;
            let frame = driver
                .process
                .frames
                .recv()
                .await
                .context("Claude closed during initialization")??;
            anyhow::ensure!(
                frame["type"] == "control_response"
                    && frame["response"]["request_id"] == id
                    && frame["response"]["subtype"] == "success",
                "Claude did not confirm isolated initialization"
            );
            anyhow::ensure!(
                frame["response"]["response"]["commands"]
                    .as_array()
                    .is_some_and(Vec::is_empty),
                "Claude exposed native commands or omitted its startup inventory"
            );
            driver
                .process
                .record_cli_version(&frame["response"]["response"]["claude_code_version"]);
            Ok::<(), anyhow::Error>(())
        })
        .await
        .context("Claude initialization timed out")
        .and_then(std::convert::identity);
        if let Err(error) = initialized {
            driver.process.kill().await;
            return Err(error);
        }
        Ok(driver)
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
            self.read_result(&events, cancel, &user_message_uuid, steering.map(crate::claude_steering::Steering::Foreground))
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

    #[tracing::instrument(skip_all)]
    pub(super) async fn read_result(
        &mut self,
        events: &mpsc::Sender<Value>,
        cancel: CancellationToken,
        user_message_uuid: &str,
        mut steering: Option<crate::claude_steering::Steering>,
    ) -> Result<ExternalTurnOutcome> {
        let mut pending_result = None;
        let mut latest_command = user_message_uuid.to_owned();
        let mut commands = HashSet::new();
        if !user_message_uuid.is_empty() {
            commands.insert(latest_command.clone());
        }
        let mut submitted_commands = commands.clone();
        let mut latest_command_succeeded = false;
        let mut cancelled_commands = HashSet::new();
        let mut submitted_steering_count = 0;
        let mut queued_steering = HashSet::new();
        let mut interrupt_id = None;
        let mut interrupt_deadline = None;
        let mut interrupt_acknowledged = false;
        let mut interrupted_commands = HashSet::new();
        let mut authentication_error = false;
        let mut tool_work_observed = false;
        let mut pending_failure = None;
        let mut failure_deadline = None;
        let mut idle = false;

        loop {
            if interrupt_id.is_none()
                && idle
                && (submitted_steering_count == 0
                    || (latest_command_succeeded && commands.is_empty()))
                && (pending_failure.is_none()
                    || (commands.is_empty()
                        && self.active_agents.is_empty()
                        && !self.background_work
                        && (pending_failure == Some(ExternalTurnFailure::UsageLimit)
                            || cancelled_commands.contains(user_message_uuid))))
                && (pending_result.is_some() || user_message_uuid.is_empty())
                && steering
                    .as_mut()
                    .is_none_or(crate::claude_steering::Steering::close_at_idle)
            {
                return Ok(ExternalTurnOutcome {
                    result: pending_result.take(),
                    interrupted: false,
                    submitted_steering_count,
                    failure: pending_failure,
                });
            }

            let frame = tokio::select! {
                biased;
                _ = cancel.cancelled(), if interrupt_id.is_none() => {
                    if pending_failure.is_some() {
                        bail!("Claude provider failure cleanup was cancelled before idle; explicit recovery is required");
                    }
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
                    if let Some(deadline) = interrupt_deadline.or(failure_deadline) {
                        sleep_until(deadline).await;
                    } else {
                        std::future::pending::<()>().await;
                    }
                } => {
                    if pending_failure.is_some() {
                        bail!("Claude provider failure did not reach a confirmed idle boundary");
                    }
                    bail!("Claude interrupt acknowledgement and idle timed out");
                },
                prompt = async {
                    match steering.as_mut() {
                        Some(receiver) => receiver.recv().await,
                        None => std::future::pending().await,
                    }
                }, if interrupt_id.is_none() && pending_failure.is_none() => {
                    let prompt = match prompt {
                        crate::claude_steering::SteeringInput::Prompt(prompt) => prompt,
                        crate::claude_steering::SteeringInput::Cancelled => continue,
                        crate::claude_steering::SteeringInput::Closed => { steering = None; continue; },
                    };
                    if !*self.mid_turn_delivery.borrow() {
                        bail!("Claude steering was submitted without command lifecycle support");
                    }
                    let command_uuid = Uuid::new_v4().to_string();
                    queued_steering.insert(command_uuid.clone());
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
                frame = async {
                    match self.first_frame.take() {
                        Some(frame) => Ok(frame),
                        None => self.process.frames.recv().await.context("Claude stream reader stopped")?,
                    }
                } => frame?,
            };
            self.observe_session_frame(&frame)?;
            if frame["type"] == "user"
                && frame["uuid"]
                    .as_str()
                    .is_some_and(|id| queued_steering.remove(id))
            {
                self.queued_input_count
                    .fetch_sub(/*val*/ 1, std::sync::atomic::Ordering::SeqCst);
            }
            let tool_block = |block: &Value| {
                matches!(
                    block.get("type").and_then(Value::as_str),
                    Some("tool_use" | "server_tool_use" | "tool_result")
                )
            };
            let tool_work = frame
                .pointer("/message/content")
                .and_then(Value::as_array)
                .is_some_and(|blocks| blocks.iter().any(tool_block))
                || frame
                    .pointer("/event/content_block")
                    .is_some_and(tool_block)
                || matches!(
                    frame.get("type").and_then(Value::as_str),
                    Some("tool_progress" | "tool_use_summary")
                )
                || frame
                    .get("parent_tool_use_id")
                    .is_some_and(|id| !id.is_null())
                || (frame.get("type").and_then(Value::as_str) == Some("system")
                    && frame.get("subtype").and_then(Value::as_str) == Some("task_started"));
            tool_work_observed |= tool_work;
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
                    let known = self.active_agents.get(task_id).cloned().flatten();
                    if known.is_none() || run.is_some_and(|run| Some(run) >= known.as_deref()) {
                        self.active_agents
                            .insert(task_id.to_owned(), run.map(str::to_owned));
                    }
                }
                if subtype == Some("task_notification")
                    && matches!(
                        frame.get("status").and_then(Value::as_str),
                        Some("stopped" | "completed" | "failed" | "killed")
                    )
                    && let Some(task_id) = frame.get("task_id").and_then(Value::as_str)
                    && let Some(known) = self.active_agents.get(task_id)
                {
                    // A run-less or older-run terminal cannot prove that a known newer run stopped.
                    let run = frame.get("run_id").and_then(Value::as_str);
                    if known.is_none() || run.is_some_and(|run| Some(run) >= known.as_deref()) {
                        self.active_agents.remove(task_id);
                    }
                }
                if subtype == Some("session_state_changed") {
                    // Native idle already accounts for blocking tasks. The task snapshot also
                    // includes background shells, which must not veto this completion boundary.
                    idle = frame.get("state").and_then(Value::as_str) == Some("idle");
                }
            }
            if pending_failure.is_some()
                && (tool_work
                    || matches!(
                        frame.get("type").and_then(Value::as_str),
                        Some("assistant" | "stream_event" | "result")
                    ))
            {
                bail!(
                    "Claude emitted new activity after its provider failure; explicit recovery is required"
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
                idle = false;
                if frame.get("type").and_then(Value::as_str) == Some("assistant") {
                    authentication_error = frame.get("error").and_then(Value::as_str)
                        == Some("authentication_failed")
                        && frame.get("user_message_uuid").and_then(Value::as_str)
                            == Some(user_message_uuid);
                    self.last_assistant_uuid =
                        frame.get("uuid").and_then(Value::as_str).map(str::to_owned);
                }
            }
            let send = events.send(frame.clone());
            if let Some(deadline) = interrupt_deadline.or(failure_deadline) {
                tokio::time::timeout_at(deadline, send)
                    .await
                    .context("Claude bounded cleanup event delivery timed out")?
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
                    && (frame
                        .get("user_message_uuid")
                        .and_then(Value::as_str)
                        .is_some_and(|id| {
                            commands.contains(id) || cancelled_commands.contains(id)
                        })
                        || (user_message_uuid.is_empty()
                            && frame.get("user_message_uuid").is_none_or(Value::is_null)));

                let result_command = frame.get("user_message_uuid").and_then(Value::as_str);
                if interrupted_execution && let Some(id) = result_command {
                    interrupted_commands.insert(id.to_owned());
                }
                // Native reports provider failures as an error result with the `success` subtype.
                // A usage limit can end a turn after tool work; authentication fails before any.
                let failure = if interrupt_id.is_some()
                    || frame.get("is_error").and_then(Value::as_bool) != Some(true)
                    || frame.get("subtype").and_then(Value::as_str) != Some("success")
                {
                    None
                } else if authentication_error
                    && !tool_work_observed
                    && submitted_steering_count == 0
                    && result_command == Some(user_message_uuid)
                {
                    Some(ExternalTurnFailure::Authentication)
                } else if frame.get("api_error").and_then(Value::as_str)
                    == Some("usage_limit_reached")
                    && result_command.is_some_and(|id| submitted_commands.contains(id))
                {
                    Some(ExternalTurnFailure::UsageLimit)
                } else {
                    None
                };
                if !interrupted_execution
                    && failure.is_none()
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
                if failure.is_some() {
                    self.failure_result = Some(frame.clone());
                    pending_failure = failure;
                    failure_deadline = Some(Instant::now() + STOP_TIMEOUT);
                    idle = false;
                }
                if result_command == Some(latest_command.as_str()) && !interrupted_execution {
                    latest_command_succeeded = true;
                }
                // A notification turn can run while ASM is between turns, for example after
                // an interrupt leaves a background shell live. Accept one only after this turn's
                // own input succeeded, so a stale notification never answers new input.
                let notification = frame.pointer("/origin/kind").and_then(Value::as_str)
                    == Some("task-notification");
                if (user_message_uuid.is_empty()
                    || (submitted_steering_count == 0 && !notification && result_command.is_none()))
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
                if is_idle && !self.active_agents.is_empty() {
                    bail!("Claude became idle before interrupted local agents stopped");
                }
                if interrupt_acknowledged
                    && idle
                    && self.active_agents.is_empty()
                    && interrupted_commands.is_subset(&cancelled_commands)
                {
                    return Ok(ExternalTurnOutcome {
                        result: pending_result,
                        interrupted: true,
                        submitted_steering_count,
                        failure: None,
                    });
                }
            }
        }
    }

    pub(super) fn observe_session_frame(&mut self, frame: &Value) -> Result<()> {
        if !frame.is_object() || frame.get("type").and_then(Value::as_str).is_none() {
            bail!("Claude emitted a JSON frame without an event type");
        }
        if matches!(
            frame["type"].as_str(),
            Some("assistant" | "stream_event" | "result" | "tool_progress")
        ) {
            anyhow::ensure!(
                self.initialized,
                "Claude emitted model activity before its tool inventory was verified"
            );
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
            let tools = frame["tools"]
                .as_array()
                .context("Claude omitted its tool inventory")?;
            anyhow::ensure!(
                !tools.is_empty()
                    && tools.iter().all(|tool| tool
                        .as_str()
                        .is_some_and(|name| name.starts_with("mcp__asm__"))),
                "Claude exposed tools outside the ASM bridge or failed to register the bridge"
            );
            for name in ["skills", "slash_commands"] {
                anyhow::ensure!(
                    frame[name].as_array().is_some_and(Vec::is_empty),
                    "Claude exposed native {name} or omitted its inventory"
                );
            }
            for name in ["mcp_servers", "plugins"] {
                anyhow::ensure!(
                    frame[name]
                        .as_array()
                        .is_some_and(|items| items.len() == 1 && items[0]["name"] == "asm"),
                    "Claude exposed non-ASM {name} or failed to register the bridge"
                );
            }
            anyhow::ensure!(
                frame["mcp_servers"][0]["source"] == "plugin"
                    && frame["mcp_servers"][0]["status"] == "connected"
                    && frame["plugins"][0]["source"] == "asm@inline",
                "Claude did not load the explicit ASM bridge plugin"
            );
            self.initialized = true;
            self.process
                .record_cli_version(&frame["claude_code_version"]);
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
        if frame["type"] == "system" && frame["subtype"] == "background_tasks_changed" {
            self.background_work = frame
                .get("tasks")
                .and_then(Value::as_array)
                .context("Claude reported background tasks without a task list")?
                .iter()
                .any(|task| task.get("ambient").and_then(Value::as_bool) != Some(true));
        }
        Ok(())
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
