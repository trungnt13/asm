//! Run persistent official Cursor Agent sessions over ACP: JSON-RPC frames on `agent acp` stdio.
//! Cursor owns its tools, permissions and history; ASM grants no approvals.

mod images;

use crate::ExternalTurnOutcome;
use crate::Prompt;
use crate::process::ExternalProcess;
use crate::process::STOP_TIMEOUT;
use crate::process::TurnGuard;
use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
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
    process: ExternalProcess,
    session_id: String,
    next_request_id: u64,
    config_options: Value,
    supports_images: bool,
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
            process: ExternalProcess::spawn(command, crate::frames::FramePolicy::CursorImages)
                .context("starting official Cursor agent")?,
            session_id: String::new(),
            next_request_id: 0,
            config_options: Value::Null,
            supports_images: false,
            usable: false,
            failure_result: None,
            bridge_instructions: options.bridge_directory.as_ref().map(|_| "ASM communication: use native shell to run \"$ASM_NATIVE_EXE\" --asm-native-tools list for available tools and JSON schemas. Call a listed tool with JSON arguments on stdin: printf '%s' '<JSON>' | \"$ASM_NATIVE_EXE\" --asm-native-tools call <name>. Do not print the private descriptor or retry side-effecting calls automatically. Long calls may background; await their completion.".to_owned()),
        };
        let setup = runtime.start_session(options).await;
        if setup.is_err() {
            runtime.process.kill().await;
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
                    "_meta": {"subagents": {}, "parameterizedModelPicker": true}}}),
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
        self.supports_images = initialized["agentCapabilities"]["promptCapabilities"]["image"]
            .as_bool()
            .unwrap_or(false);
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
        let (model, encoded) = options
            .model
            .split_once('[')
            .map_or((options.model.as_str(), ""), |(model, parameters)| {
                (model, parameters.trim_end_matches(']'))
            });
        let model_options = session["configOptions"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|option| option["id"] == "model")
            .and_then(|option| option["options"].as_array())
            .context("Cursor did not advertise model choices")?;
        // Saved Auto IDs and the CLI display ID differ from ACP's canonical value.
        let model = if model_options.iter().any(|option| option["value"] == model) {
            model
        } else if matches!(model, "auto" | "auto-smart")
            && model_options
                .iter()
                .any(|option| option["value"] == "default")
        {
            "default"
        } else {
            bail!("Cursor does not advertise model `{model}`");
        };
        let mut selected = self
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
            "Cursor did not confirm model `{model}`"
        );
        let mut parameters = encoded
            .split(',')
            .filter(|parameter| !parameter.is_empty())
            .map(|parameter| {
                parameter
                    .split_once('=')
                    .context("Invalid saved Cursor model parameter")
                    .map(|(id, value)| (id.to_owned(), value.to_owned()))
            })
            .collect::<Result<Vec<_>>>()?;
        if let Some(effort) = options.effort {
            let thought = selected["configOptions"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|option| {
                    option["category"] == "thought_level"
                        && option["options"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .any(|value| value["value"] == effort)
                });
            if let Some(thought) = thought {
                let id = thought["id"]
                    .as_str()
                    .context("Cursor effort parameter has no ID")?;
                parameters.retain(|(key, _)| key != id);
                parameters.push((id.to_owned(), effort));
            } else if effort != "none" || !matches!(model, "default" | "auto" | "auto-smart") {
                bail!("Cursor model `{model}` does not advertise adjustable effort");
            }
        }
        for (id, value) in parameters {
            anyhow::ensure!(
                selected["configOptions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|option| option["id"] == id)
                    .flat_map(|option| option["options"].as_array().into_iter().flatten())
                    .any(|option| option["value"] == value),
                "Cursor does not advertise `{id}={value}` for `{model}`"
            );
            selected = self
                .request(
                    "session/set_config_option",
                    json!({
                "sessionId": self.session_id, "configId": id, "value": value}),
                )
                .await?;
            anyhow::ensure!(
                selected["configOptions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|option| option["id"] == id && option["currentValue"] == value),
                "Cursor did not confirm `{id}={value}`"
            );
        }
        self.config_options = selected["configOptions"].clone();
        self.usable = true;
        Ok(())
    }

    #[tracing::instrument(skip_all)]
    pub async fn set_effort(&mut self, effort: Option<&str>) -> Result<()> {
        anyhow::ensure!(
            self.usable,
            "Cursor session requires recovery before changing effort"
        );
        let effort = effort.context("Cursor requires an explicit effort when changing it")?;
        let id = self
            .config_options
            .as_array()
            .into_iter()
            .flatten()
            .find(|option| {
                option["category"] == "thought_level"
                    && option["options"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|value| value["value"] == effort)
            })
            .and_then(|option| option["id"].as_str())
            .context("Cursor does not advertise the requested effort")?
            .to_owned();
        self.usable = false;
        let mut guard = TurnGuard {
            pid: self.process.child.id(),
            armed: true,
            marker: self.process.marker.clone(),
            birth: self.process.birth,
        };
        let result = async {
            let selected = self
                .request(
                    "session/set_config_option",
                    json!({
                        "sessionId": self.session_id, "configId": id, "value": effort
                    }),
                )
                .await?;
            anyhow::ensure!(
                selected["configOptions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|option| option["id"] == id && option["currentValue"] == effort),
                "Cursor did not confirm requested effort"
            );
            self.config_options = selected["configOptions"].clone();
            Ok(())
        }
        .await;
        if result.is_ok() {
            self.usable = true;
        } else {
            self.process.kill().await;
        }
        guard.armed = false;
        result
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
        timeout(SETUP_TIMEOUT, async {
            let id = self.send_request(method, params).await?;
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
        prompt: &Prompt,
        events: mpsc::Sender<Value>,
        cancel: CancellationToken,
    ) -> Result<ExternalTurnOutcome> {
        if !self.usable {
            bail!("Cursor session requires explicit recovery after an unfinished turn");
        }
        if cancel.is_cancelled() {
            bail!("Cursor turn cancelled before submission");
        }
        let frame = self.prompt_frame(prompt)?;
        self.usable = false;
        self.failure_result = None;
        let mut guard = TurnGuard {
            pid: self.process.child.id(),
            armed: true,
            marker: self.process.marker.clone(),
            birth: self.process.birth,
        };
        self.bridge_instructions = None;
        let mut result = self.read_result(frame, &events, cancel).await;
        match &result {
            Ok(outcome) if !outcome.interrupted => self.usable = true,
            // Cancelled tools can outlive Cursor's acknowledgement in their own process
            // groups; stop the tree and let the next turn load the clean native session.
            // Uncertain descendant cleanup must not be reported as a clean cancellation.
            Ok(_) => {
                let cleanup = self.process.terminate().await;
                if let Err(error) = cleanup {
                    result =
                        Err(error.context("Cursor tool cleanup after cancellation is uncertain"));
                }
            }
            Err(_) => self.process.kill().await,
        }
        guard.armed = false;
        result
    }

    async fn read_result(
        &mut self,
        frame: Value,
        events: &mpsc::Sender<Value>,
        cancel: CancellationToken,
    ) -> Result<ExternalTurnOutcome> {
        self.next_request_id += 1;
        let prompt_id = self.next_request_id;
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                bail!("Cursor turn cancelled during submission; explicit recovery is required");
            }
            result = self.process.write_frame(frame) => result?,
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
            return Ok(ExternalTurnOutcome {
                result: Some(frame["result"].clone()),
                interrupted,
                submitted_steering_count: 0,
                failure: None,
            });
        }
    }

    #[tracing::instrument(skip_all)]
    pub async fn shutdown(&mut self) -> Result<()> {
        self.usable = false;
        // `agent acp` does not exit when stdin closes; native history is already saved.
        let cleanup = self.process.terminate().await;
        self.process.wait_for_stderr().await;
        cleanup
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
