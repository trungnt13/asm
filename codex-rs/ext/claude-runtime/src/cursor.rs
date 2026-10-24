//! Run persistent official Cursor Agent sessions over ACP: JSON-RPC frames on `agent acp` stdio.
//! Cursor owns its tools, permissions and history; ASM grants no approvals.

use super::ClaudeTurnOutcome;
use super::NativeProcess;
use super::STOP_TIMEOUT;
use super::TurnGuard;
use super::kill_group;
use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use codex_utils_pty::process_group;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio::time::sleep_until;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

/// Cursor login and session setup can stall on its backend; fail instead of hanging the turn.
const SETUP_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 60);

pub enum CursorSession {
    New,
    Load { id: String },
}

pub struct CursorOptions {
    pub executable: PathBuf,
    pub cwd: PathBuf,
    /// A listed ACP model value, or a base name matching exactly one listed value.
    pub model: String,
    pub effort: Option<String>,
    /// Selected role text appended through ACP, including when loading a saved session.
    pub role_instructions: Option<String>,
    pub session: CursorSession,
    /// Private `CURSOR_CONFIG_DIR`. Cursor persists session model selections there, so a
    /// shared directory would silently change the user's default model.
    pub config_dir: PathBuf,
    pub bridge_directory: Option<PathBuf>,
}

pub struct CursorRuntime {
    process: NativeProcess,
    session_id: String,
    next_request_id: u64,
    usable: bool,
    failure_result: Option<Value>,
    bridge_instructions: Option<String>,
}

impl CursorRuntime {
    #[tracing::instrument(skip_all)]
    pub async fn spawn(options: CursorOptions) -> Result<Self> {
        if !cfg!(any(target_os = "linux", target_os = "macos")) {
            bail!("Cursor runtime currently supports Linux and macOS only");
        }
        copy_user_config(&options.config_dir)?;
        let mut command = Command::new(&options.executable);
        // Keep login in the official CLI, without accidental API-key billing.
        command
            .env_remove("CURSOR_API_KEY")
            .env("CURSOR_CONFIG_DIR", &options.config_dir)
            .current_dir(&options.cwd)
            // Native run-everything mode; explicit denies and team auto-run controls still apply.
            .args(["--force", "acp"]);
        if let Some(directory) = &options.bridge_directory {
            command
                .env("ASM_NATIVE_DESCRIPTOR", directory.join("descriptor.json"))
                .env("ASM_NATIVE_EXE", std::env::current_exe()?);
        }
        let mut runtime = Self {
            process: NativeProcess::spawn(command).context("starting official Cursor agent")?,
            session_id: String::new(),
            next_request_id: 0,
            usable: false,
            failure_result: None,
            bridge_instructions: options.bridge_directory.as_ref().map(|_| "ASM communication: use native shell to run \"$ASM_NATIVE_EXE\" --asm-native-tools list for available tools and JSON schemas. Call a listed tool with JSON arguments on stdin: printf '%s' '<JSON>' | \"$ASM_NATIVE_EXE\" --asm-native-tools call <name>. Do not print the private descriptor or retry side-effecting calls automatically. Long calls may background; await their completion.".to_owned()),
        };
        let setup = runtime.start_session(options).await;
        if setup.is_err() {
            runtime.process.kill(kill_tree).await;
        }
        setup.map(|()| runtime)
    }

    async fn start_session(&mut self, options: CursorOptions) -> Result<()> {
        let initialized = self
            .request(
                "initialize",
                // Subagent support publishes native child sessions and makes prompts wait for
                // background children and cancel them; Cursor runs its Task tool either way.
                json!({"protocolVersion": 1, "clientCapabilities": {
                    "fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false,
                    "_meta": {"subagents": {}}}}),
            )
            .await?;
        if initialized["protocolVersion"] != 1 {
            bail!("Cursor agent does not support ACP protocol version 1");
        }
        if !initialized["agentCapabilities"]["sessionCapabilities"]["subagents"].is_object() {
            bail!("Cursor agent does not support ACP subagents; upgrade the Cursor CLI");
        }
        if options.role_instructions.is_some()
            && !initialized["agentCapabilities"]["sessionCapabilities"]["systemPrompt"].is_object()
        {
            bail!(
                "Cursor agent does not support ACP systemPrompt; cannot forward selected role instructions"
            );
        }
        self.request("authenticate", json!({"methodId": "cursor_login"}))
            .await
            .context("Cursor login is required; run `agent login`")?;
        let mut params = json!({"cwd": options.cwd, "mcpServers": []});
        if let Some(instructions) = options.role_instructions {
            params["systemPrompt"] = json!({
                "mode": "append",
                "content": [{"type": "text", "text": instructions}],
            });
        }
        let session = match options.session {
            CursorSession::New => {
                let session = self.request("session/new", params).await?;
                self.session_id = session["sessionId"]
                    .as_str()
                    .context("Cursor did not return a session ID")?
                    .to_owned();
                session
            }
            CursorSession::Load { id } => {
                self.session_id = id;
                // Native history replay arrives before this response and is discarded.
                params["sessionId"] = json!(self.session_id);
                self.request("session/load", params).await?
            }
        };
        let values = session["configOptions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|option| option["id"] == "model")
            .flat_map(|option| option["options"].as_array().into_iter().flatten())
            .filter_map(|option| option["value"].as_str())
            .collect::<Vec<_>>();
        let matches = if values.contains(&options.model.as_str()) {
            vec![options.model.as_str()]
        } else {
            values
                .iter()
                .copied()
                .filter(|value| value.split('[').next() == Some(options.model.as_str()))
                .collect()
        };
        let matches = matches
            .into_iter()
            .filter(|value| {
                options.effort.as_deref().is_none_or(|effort| {
                    if effort == "none" && value.starts_with("auto-smart[") {
                        return true;
                    }
                    value.split_once('[').is_some_and(|(_, parameters)| {
                        parameters
                            .trim_end_matches(']')
                            .split(',')
                            .any(|parameter| {
                                parameter.split_once('=').is_some_and(|(key, value)| {
                                    matches!(key, "effort" | "reasoning_effort" | "reasoning")
                                        && value == effort
                                })
                            })
                    })
                })
            })
            .collect::<Vec<_>>();
        let [model] = matches[..] else {
            bail!(
                "Cursor model `{}` with effort {:?} is not one available variant; use one of: {}",
                options.model,
                options.effort,
                values.join(", ")
            );
        };
        let model = model.to_owned();
        let selected = self
            .request(
                "session/set_config_option",
                json!({"sessionId": self.session_id, "configId": "model", "value": model}),
            )
            .await?;
        anyhow::ensure!(
            selected["configOptions"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|option| option["id"] == "model" && option["currentValue"] == model),
            "Cursor did not confirm requested model variant `{model}`"
        );
        self.usable = true;
        Ok(())
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// These bounded diagnostics can contain private information.
    /// Save them only when private diagnostic capture is enabled.
    pub fn diagnostic_snapshot(&self) -> Value {
        self.process.diagnostic_snapshot(&self.failure_result)
    }

    /// Sends one setup request and waits for its response, discarding notifications.
    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.send_request(method, params).await?;
        timeout(SETUP_TIMEOUT, async {
            loop {
                let frame = self
                    .process
                    .frames
                    .recv()
                    .await
                    .context("Cursor stream reader stopped")??;
                if frame.get("method").is_some() {
                    self.refuse_request(&frame).await?;
                } else if frame["id"] == id {
                    if !frame["error"].is_null() {
                        self.failure_result = Some(frame);
                        bail!("Cursor rejected {method}");
                    }
                    return Ok(frame["result"].clone());
                }
            }
        })
        .await
        .with_context(|| format!("Cursor {method} timed out"))?
    }

    async fn send_request(&mut self, method: &str, params: Value) -> Result<u64> {
        self.next_request_id += 1;
        let id = self.next_request_id;
        self.process
            .write_frame(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await?;
        Ok(id)
    }

    /// Answers agent-initiated requests without granting anything, so Cursor cannot hang:
    /// permission prompts get a reject option, plans and questions get typed refusals, and
    /// other methods get an error.
    async fn refuse_request(&mut self, frame: &Value) -> Result<()> {
        let Some(id) = frame.get("id") else {
            return Ok(());
        };
        let refusal = |outcome: &str, reason: &str| {
            json!({"jsonrpc": "2.0", "id": id, "result": {"outcome": {
                "outcome": outcome, "reason": reason}}})
        };
        // Cursor treats an unsupported plan or question method as success with a local
        // fallback, so these need typed refusals.
        let response = if frame["method"] == "cursor/create_plan" {
            refusal("rejected", "ASM cannot review Cursor plans")
        } else if frame["method"] == "cursor/ask_question" {
            refusal("skipped", "ASM cannot answer Cursor questions")
        } else if frame["method"] == "session/request_permission" {
            let rejection = frame["params"]["options"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|option| option["kind"] == "reject_once")
                .and_then(|option| option["optionId"].as_str());
            let outcome = match rejection {
                Some(option_id) => json!({"outcome": "selected", "optionId": option_id}),
                None => json!({"outcome": "cancelled"}),
            };
            json!({"jsonrpc": "2.0", "id": id, "result": {"outcome": outcome}})
        } else {
            json!({"jsonrpc": "2.0", "id": id, "error": {
                "code": -32601, "message": "ASM does not support this Cursor request"}})
        };
        self.process.write_frame(response).await
    }

    #[tracing::instrument(skip_all)]
    pub async fn run_turn(
        &mut self,
        prompt: &str,
        events: mpsc::Sender<Value>,
        cancel: CancellationToken,
    ) -> Result<ClaudeTurnOutcome> {
        if !self.usable {
            bail!("Cursor session requires explicit recovery after an unfinished turn");
        }
        if cancel.is_cancelled() {
            bail!("Cursor turn cancelled before submission");
        }
        self.usable = false;
        self.failure_result = None;
        let mut guard = TurnGuard {
            pid: self.process.child.id(),
            armed: true,
            kill: kill_tree,
        };
        let prompt = match self.bridge_instructions.take() {
            Some(instructions) => format!("{instructions}\n\n{prompt}"),
            None => prompt.to_owned(),
        };
        let mut result = self.read_result(&prompt, &events, cancel).await;
        match &result {
            Ok(outcome) if !outcome.interrupted => self.usable = true,
            // Cancelled tools can outlive Cursor's acknowledgement in their own process
            // groups; stop the tree and let the next turn load the clean native session.
            // Uncertain descendant cleanup must not be reported as a clean cancellation.
            Ok(_) => {
                let cleanup = try_kill_tree(self.process.child.id());
                self.process.kill(kill_group).await;
                if let Err(error) = cleanup {
                    result =
                        Err(error.context("Cursor tool cleanup after cancellation is uncertain"));
                }
            }
            Err(_) => self.process.kill(kill_tree).await,
        }
        guard.armed = false;
        result
    }

    async fn read_result(
        &mut self,
        prompt: &str,
        events: &mpsc::Sender<Value>,
        cancel: CancellationToken,
    ) -> Result<ClaudeTurnOutcome> {
        let prompt_id = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                bail!("Cursor turn cancelled during submission; explicit recovery is required");
            }
            id = self.send_request("session/prompt", json!({"sessionId": self.session_id,
                "prompt": [{"type": "text", "text": prompt}]})) => id?,
        };
        let mut cancel_deadline = None;
        // Native child sessions announced this turn, with their last reported state.
        let mut children: HashMap<String, Option<String>> = HashMap::new();
        loop {
            let frame = tokio::select! {
                biased;
                _ = cancel.cancelled(), if cancel_deadline.is_none() => {
                    let deadline = Instant::now() + STOP_TIMEOUT;
                    timeout(STOP_TIMEOUT, self.process.write_frame(json!({"jsonrpc": "2.0",
                        "method": "session/cancel", "params": {"sessionId": self.session_id}})))
                        .await
                        .context("Cursor cancel submission timed out")??;
                    cancel_deadline = Some(deadline);
                    continue;
                }
                _ = async {
                    match cancel_deadline {
                        Some(deadline) => sleep_until(deadline).await,
                        None => std::future::pending::<()>().await,
                    }
                } => bail!("Cursor cancel acknowledgement timed out"),
                frame = self.process.frames.recv() => frame.context("Cursor stream reader stopped")??,
            };
            if frame.get("method").is_some() {
                if frame["method"] == "session/update" {
                    let session = frame["params"]["sessionId"].as_str().unwrap_or_default();
                    if session != self.session_id && !children.contains_key(session) {
                        bail!("Cursor returned an update for a different session");
                    }
                    let update = &frame["params"]["update"];
                    let child = update["subagentSessionId"].as_str();
                    match update["sessionUpdate"].as_str() {
                        Some("subagent_spawned") => {
                            let child = child.context("Cursor subagent has no session ID")?;
                            children.insert(child.to_owned(), /*v*/ None);
                        }
                        Some("subagent_state_update") => {
                            let state = children
                                .get_mut(child.unwrap_or_default())
                                .context("Cursor updated an unknown subagent")?;
                            *state = update["state"].as_str().map(str::to_owned);
                        }
                        _ => {}
                    }
                }
                self.refuse_request(&frame).await?;
            } else if frame["id"] != prompt_id {
                continue;
            }
            let send = events.send(frame.clone());
            match cancel_deadline {
                Some(deadline) => tokio::time::timeout_at(deadline, send)
                    .await
                    .context("Cursor cancel event delivery timed out")?
                    .context("Cursor event consumer closed")?,
                None => send.await.context("Cursor event consumer closed")?,
            }
            // Agent request IDs are a separate namespace; only responses complete the prompt.
            if frame.get("method").is_some() || frame["id"] != prompt_id {
                continue;
            }
            let stop_reason = frame["result"]["stopReason"].as_str();
            let interrupted = cancel_deadline.is_some();
            if !interrupted && stop_reason != Some("end_turn") {
                // Only known categories enter the user error. Provider prose stays in
                // opt-in private diagnostics.
                let category = match stop_reason {
                    Some("max_tokens") => "maximum tokens reached",
                    Some("max_turn_requests") => "maximum model requests reached",
                    Some("refusal") => "refusal",
                    Some("cancelled") => "cancelled by Cursor",
                    Some(_) => "unknown stop reason",
                    None => "provider/API error",
                };
                self.failure_result = Some(frame);
                bail!("Cursor returned an unsuccessful turn result: {category}");
            }
            if interrupted && !matches!(stop_reason, Some("cancelled" | "end_turn")) {
                self.failure_result = Some(frame);
                bail!("Cursor did not confirm cancellation");
            }
            // Cursor waits for this turn's children before answering. It also answers after
            // its cancel wait times out, marking children `disconnected`: that is not a stop.
            if children.values().any(|state| {
                !matches!(state.as_deref(), Some("completed" | "failed" | "cancelled"))
            }) {
                self.failure_result = Some(frame);
                bail!("Cursor did not confirm that its native subagents stopped");
            }
            return Ok(ClaudeTurnOutcome {
                result: Some(frame["result"].clone()),
                interrupted,
                submitted_steering_count: 0,
            });
        }
    }

    #[tracing::instrument(skip_all)]
    pub async fn shutdown(&mut self) -> Result<()> {
        self.usable = false;
        // `agent acp` does not exit when stdin closes; native history is already saved.
        self.process.kill(kill_tree).await;
        self.process.wait_for_stderr().await;
        Ok(())
    }
}

/// Refreshes the private config directory from the user's Cursor settings, read-only.
/// Native permission and approval settings live there; login and other native resources
/// do not, so they stay shared without copying credentials.
fn copy_user_config(config_dir: &std::path::Path) -> Result<()> {
    // Match Cursor's precedence: a variable counts only when it is not blank.
    let variable = |name| {
        std::env::var_os(name)
            .filter(|value: &std::ffi::OsString| !value.to_string_lossy().trim().is_empty())
    };
    let user_dir = match (variable("CURSOR_CONFIG_DIR"), variable("XDG_CONFIG_HOME")) {
        (Some(dir), _) => PathBuf::from(dir),
        (None, Some(dir)) => PathBuf::from(dir).join("cursor"),
        (None, None) => std::env::home_dir()
            .context("home directory is unknown")?
            .join(".cursor"),
    };
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(config_dir)?;
    for name in ["cli-config.json", "permissions.json"] {
        let target = config_dir.join(name);
        // Replace the previous private copy; a missing source must not leave stale settings.
        match std::fs::remove_file(&target) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        match std::fs::copy(user_dir.join(name), &target) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(anyhow::Error::from(error).context(format!("copying Cursor {name}")));
            }
        }
        // The copy inherits the source mode; Cursor must be able to write its private copy.
        #[cfg(unix)]
        std::fs::set_permissions(&target, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    }
    Ok(())
}

impl Drop for CursorRuntime {
    fn drop(&mut self) {
        // The process's own Drop stops only its group; tools run in separate groups.
        kill_tree(self.process.child.id());
    }
}

/// Bounds the process-table snapshot used during cleanup.
const PROCESS_LIST_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 2);

fn kill_tree(pid: Option<u32>) {
    if let Err(error) = try_kill_tree(pid) {
        tracing::warn!(?pid, %error, "Cursor descendant cleanup is incomplete");
    }
}

/// Kills the process groups of `pid`'s descendants, then its own group.
/// Cursor runs shell tools in separate process groups under the ACP process, so the
/// owned group alone does not stop them. Never signals ASM's own process group.
/// The owned group is killed even when descendants cannot be listed; that case errors.
fn try_kill_tree(pid: Option<u32>) -> Result<()> {
    let Some(pid) = pid else {
        return Ok(());
    };
    let groups = list_processes()
        .and_then(|table| descendant_groups(&table, pid))
        .context("listing Cursor descendants failed");
    // Signal every known group, including the owned one, but report the first failure.
    let mut result = groups
        .as_ref()
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!("{error:#}"));
    for group in groups.iter().flatten().copied().chain([pid]) {
        if let Err(error) = process_group::kill_process_group(group)
            && result.is_ok()
        {
            result =
                Err(anyhow::Error::from(error)
                    .context(format!("killing Cursor process group {group}")));
        }
    }
    result?;
    let groups = groups?;
    // Signals are asynchronous; report success only after the groups are observed gone.
    // A dead leader stays a zombie until its owner reaps it. This covers groups seen in the
    // snapshot, not processes that left them, for example with `setsid`.
    let deadline = std::time::Instant::now() + PROCESS_LIST_TIMEOUT;
    loop {
        let survivors = list_processes()?
            .into_iter()
            .any(|row| !row.zombie && (row.group == pid || groups.contains(&row.group)));
        if !survivors {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            bail!("Cursor processes survived SIGKILL");
        }
        std::thread::sleep(Duration::from_millis(/*millis*/ 20));
    }
}

/// Returns the process groups of `pid`'s descendants other than `pid`'s and ASM's.
fn descendant_groups(table: &[ProcessRow], pid: u32) -> Result<HashSet<u32>> {
    let own_group = table
        .iter()
        .find(|row| row.pid == std::process::id())
        .map(|row| row.group)
        .context("ASM is missing from the process list")?;
    let mut tree = HashSet::from([pid]);
    let mut groups = HashSet::new();
    // The table is a snapshot, so repeat until no new descendants are found.
    let mut changed = true;
    while changed {
        changed = false;
        for row in table {
            if tree.contains(&row.parent) && tree.insert(row.pid) {
                changed = true;
                if row.group != pid && row.group != own_group {
                    groups.insert(row.group);
                }
            }
        }
    }
    Ok(groups)
}

struct ProcessRow {
    pid: u32,
    parent: u32,
    group: u32,
    zombie: bool,
}

/// Lists all processes. Output goes to a file so a full pipe cannot stall `ps`.
fn list_processes() -> Result<Vec<ProcessRow>> {
    use std::io::Read;
    use std::io::Seek;
    let mut output = tempfile::tempfile()?;
    let mut child = std::process::Command::new("ps")
        .args(["-A", "-o", "pid=,ppid=,pgid=,stat="])
        .stdin(std::process::Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let deadline = std::time::Instant::now() + PROCESS_LIST_TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("ps timed out");
        }
        std::thread::sleep(Duration::from_millis(/*millis*/ 10));
    };
    anyhow::ensure!(status.success(), "ps failed: {status}");
    let mut text = String::new();
    output.rewind()?;
    output.read_to_string(&mut text)?;
    Ok(text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let mut number = || fields.next()?.parse::<u32>().ok();
            let (pid, parent, group) = (number()?, number()?, number()?);
            let zombie = fields.next()?.starts_with('Z');
            Some(ProcessRow {
                pid,
                parent,
                group,
                zombie,
            })
        })
        .collect())
}
