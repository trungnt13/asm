//! Run a persistent Cursor SDK helper in an ASM-owned process group.

use crate::ExternalTurnOutcome;
use crate::InputBlock;
use crate::Prompt;
use crate::frames::FramePolicy;
use crate::process::ExternalProcess;
use crate::process::STOP_TIMEOUT;
use crate::process::StderrPolicy;
use crate::process::TurnGuard;
use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::ensure;
use serde_json::Value;
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

const SETUP_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 60);

pub struct CursorOptions {
    pub cwd: PathBuf,
    pub model: String,
    pub effort: Option<String>,
    pub system_instructions: String,
    pub bridge_directory: PathBuf,
}

/// Cursor rejected an effort change without altering the running session.
#[derive(Debug)]
pub struct CursorEffortRejected;

impl std::fmt::Display for CursorEffortRejected {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Cursor SDK does not support the requested reasoning effort")
    }
}

impl std::error::Error for CursorEffortRejected {}

/// Keeps the SDK result for accounting when a run fails.
#[derive(Debug)]
pub struct CursorRunFailed(Value);

impl CursorRunFailed {
    pub fn result(&self) -> &Value {
        &self.0
    }
}

impl std::fmt::Display for CursorRunFailed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(response_error_message(&self.0))
    }
}

impl std::error::Error for CursorRunFailed {}

pub struct CursorRuntime {
    process: ExternalProcess,
    ready: Ready,
    usable: bool,
}

struct Ready {
    id: String,
    model: String,
    effort: Option<String>,
    baseline_effort: Option<String>,
    effort_choices: Vec<String>,
}

impl CursorRuntime {
    #[tracing::instrument(skip_all)]
    pub async fn spawn(options: CursorOptions) -> Result<Self> {
        ensure!(
            cfg!(any(target_os = "linux", target_os = "macos")),
            "Cursor runtime currently supports Linux and macOS only"
        );
        let api_key = std::env::var("CURSOR_API_KEY")
            .ok()
            .filter(|key| !key.trim().is_empty())
            .context("Set CURSOR_API_KEY in the ASM process environment to use Cursor SDK")?;
        let mut command = Command::new("node");
        command
            .current_dir(&options.bridge_directory)
            .env("CURSOR_API_KEY", &api_key)
            .env_remove("CURSOR_AUTH_TOKEN")
            .env_remove("NODE_OPTIONS")
            .env_remove("NODE_PATH")
            .args([
                "--input-type=module",
                "--eval",
                include_str!("cursor_sdk.mjs"),
            ]);
        let process =
            ExternalProcess::spawn(command, FramePolicy::CursorSdk, StderrPolicy::Discard)
                .context("starting the Cursor SDK helper (requires Node.js)")?;
        Self::initialize(process, options, &api_key).await
    }

    #[tracing::instrument(skip_all)]
    async fn initialize(
        mut process: ExternalProcess,
        options: CursorOptions,
        api_key: &str,
    ) -> Result<Self> {
        let initialized = async {
            let response = request(
                &mut process,
                json!({
                    "type": "init",
                    "model": options.model,
                    "effort": options.effort,
                    "cwd": options.cwd,
                    "bridgeDirectory": options.bridge_directory,
                    "systemInstructions": options.system_instructions,
                }),
            )
            .await?;
            let ready = Ready::parse(&response, options.effort.as_deref())?;
            if response["version"]
                .as_str()
                .is_some_and(|version| !version.contains(api_key))
            {
                process.record_cli_version(&response["version"]);
            }
            Ok(ready)
        }
        .await;
        match initialized {
            Ok(ready) => Ok(Self {
                process,
                ready,
                usable: true,
            }),
            Err(error) => {
                process.kill().await;
                Err(error)
            }
        }
    }

    pub fn validate_prompt(&self, prompt: &Prompt) -> Result<()> {
        prompt_frame(prompt).map(|_| ())
    }

    pub fn current_effort(&self) -> Option<&str> {
        self.ready.effort.as_deref()
    }

    pub fn reasoning_efforts(&self) -> impl Iterator<Item = &str> {
        self.ready.effort_choices.iter().map(String::as_str)
    }

    pub fn session_id(&self) -> &str {
        &self.ready.id
    }

    pub fn diagnostic_snapshot(&self) -> Value {
        self.process.diagnostic_snapshot(&None)
    }

    #[tracing::instrument(skip_all)]
    pub async fn set_effort(&mut self, effort: Option<&str>) -> Result<()> {
        ensure!(self.usable, "Cursor SDK session is no longer usable");
        self.usable = false;
        let mut guard = self.turn_guard();
        let updated: Result<()> = async {
            let response = request(
                &mut self.process,
                json!({"type": "effort", "effort": effort}),
            )
            .await?;
            if response["type"] == "error" && response["code"] == "invalid_effort" {
                return Err(CursorEffortRejected.into());
            }
            let ready = Ready::parse(&response, effort)?;
            ensure!(
                ready.id == self.ready.id
                    && ready.model == self.ready.model
                    && ready.baseline_effort == self.ready.baseline_effort
                    && ready.effort_choices == self.ready.effort_choices,
                "Cursor SDK did not confirm the existing session, model, and effort capabilities"
            );
            self.ready = ready;
            Ok(())
        }
        .await;
        match &updated {
            Ok(()) => self.usable = true,
            Err(error) if error.is::<CursorEffortRejected>() => self.usable = true,
            Err(_) => self.process.kill().await,
        }
        guard.armed = false;
        updated
    }

    #[tracing::instrument(skip_all)]
    pub async fn run_turn(
        &mut self,
        prompt: &Prompt,
        events: mpsc::Sender<Value>,
        cancel: CancellationToken,
    ) -> Result<ExternalTurnOutcome> {
        ensure!(self.usable, "Cursor SDK session is no longer usable");
        ensure!(
            !cancel.is_cancelled(),
            "Cursor turn cancelled before submission"
        );
        let frame = prompt_frame(prompt)?;
        self.usable = false;
        let mut guard = self.turn_guard();
        let running = async {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => bail!("Cursor turn cancelled during submission"),
                result = timeout(SETUP_TIMEOUT, self.process.write_frame(frame)) => {
                    result.context("Cursor SDK prompt submission timed out")??;
                }
            }
            tokio::select! {
                biased;
                _ = cancel.cancelled() => Ok(None),
                result = read_result(&mut self.process, Some(&events)) => result.map(Some),
            }
        }
        .await;
        let outcome = match running {
            Ok(Some(result)) => {
                self.usable = true;
                Ok(turn_outcome(result, /*interrupted*/ false))
            }
            Ok(None) => {
                let cancelled = timeout(STOP_TIMEOUT, async {
                    self.process.write_frame(json!({"type": "cancel"})).await?;
                    read_result(&mut self.process, /*events*/ None).await
                })
                .await
                .context("Cursor SDK cancel acknowledgement timed out")
                .and_then(std::convert::identity);
                self.process
                    .terminate()
                    .await
                    .context("Cursor SDK cleanup after cancellation is uncertain")
                    .and(cancelled)
                    .map(|result| turn_outcome(result, /*interrupted*/ true))
            }
            Err(error) => {
                // A failed submission may leave an incomplete line; cancel is not reliable.
                self.process.kill().await;
                Err(error)
            }
        };
        guard.armed = false;
        outcome
    }

    #[tracing::instrument(skip_all)]
    pub async fn shutdown(&mut self) -> Result<()> {
        self.usable = false;
        self.process.terminate().await
    }

    fn turn_guard(&self) -> TurnGuard {
        TurnGuard {
            pid: self.process.child.id(),
            armed: true,
            marker: self.process.marker.clone(),
            birth: self.process.birth,
        }
    }
}

impl Ready {
    fn parse(frame: &Value, requested_effort: Option<&str>) -> Result<Self> {
        match frame["type"].as_str() {
            Some("ready") => {}
            Some("error") => return Err(anyhow!(response_error_message(frame))),
            _ => bail!("Cursor SDK did not confirm setup"),
        }
        let id = frame["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .context("Cursor SDK did not return a session ID")?;
        let model = frame["model"]
            .as_str()
            .filter(|model| !model.is_empty())
            .context("Cursor SDK did not confirm the selected model")?;
        let read_effort = |field| match frame.get(field) {
            Some(Value::Null) => Ok(None),
            Some(Value::String(effort)) if !effort.is_empty() => Ok(Some(effort.as_str())),
            _ => Err(anyhow!("Cursor SDK did not confirm reasoning effort")),
        };
        let effort = read_effort("effort")?;
        let baseline_effort = read_effort("baselineEffort")?;
        let effort_choices: Vec<String> = serde_json::from_value(frame["effortChoices"].clone())
            .context("Cursor SDK did not report reasoning effort choices")?;
        ensure!(
            !effort_choices.iter().any(String::is_empty)
                && [effort, baseline_effort]
                    .into_iter()
                    .flatten()
                    .all(|value| effort_choices.iter().any(|choice| choice == value)),
            "Cursor SDK reported inconsistent reasoning effort choices"
        );
        ensure!(
            effort == requested_effort.or(baseline_effort)
                || (requested_effort == Some("none")
                    && baseline_effort.is_none()
                    && effort.is_none()),
            "Cursor SDK did not confirm the requested reasoning effort"
        );
        Ok(Self {
            id: id.to_owned(),
            model: model.to_owned(),
            effort: effort.map(str::to_owned),
            baseline_effort: baseline_effort.map(str::to_owned),
            effort_choices,
        })
    }
}

#[tracing::instrument(skip_all)]
async fn request(process: &mut ExternalProcess, request: Value) -> Result<Value> {
    timeout(SETUP_TIMEOUT, async {
        process.write_frame(request).await?;
        process
            .frames
            .recv()
            .await
            .context("Cursor SDK closed during setup")?
    })
    .await
    .context("Cursor SDK setup timed out")?
}

#[tracing::instrument(skip_all)]
async fn read_result(
    process: &mut ExternalProcess,
    events: Option<&mpsc::Sender<Value>>,
) -> Result<Value> {
    loop {
        let frame = process
            .frames
            .recv()
            .await
            .context("Cursor SDK stream reader stopped")??;
        match frame["type"].as_str() {
            Some("assistant_end" | "thinking_end") => {}
            Some("assistant_delta" | "thinking_delta") if frame["text"].is_string() => {}
            Some("result") => match frame["status"].as_str() {
                Some("finished") => {
                    ensure!(
                        frame["uuid"].as_str().is_some_and(|id| !id.is_empty()),
                        "Cursor SDK did not confirm the completed turn ID"
                    );
                    return Ok(frame);
                }
                Some("cancelled") if events.is_none() => return Ok(frame),
                Some("cancelled") => bail!("Cursor SDK cancelled the turn"),
                Some("error") if events.is_some() => return Err(CursorRunFailed(frame).into()),
                Some("error") => return Err(anyhow!(response_error_message(&frame))),
                _ => bail!("Cursor SDK returned an invalid turn result"),
            },
            Some("error") => return Err(anyhow!(response_error_message(&frame))),
            _ => bail!("Cursor SDK returned an unexpected turn event"),
        }
        if let Some(events) = events {
            events
                .send(frame)
                .await
                .context("Cursor SDK event consumer closed")?;
        }
    }
}

fn response_error_message(frame: &Value) -> &'static str {
    match frame["code"].as_str() {
        Some("invalid_effort") => "Cursor SDK does not support the requested reasoning effort",
        Some("invalid_model") => "Cursor SDK does not support the selected model or parameters",
        Some("authentication") => "Cursor SDK authentication failed; check CURSOR_API_KEY",
        Some("sdk_missing") => "The official @cursor/sdk package is not installed for ASM",
        Some("sdk_version") => "The installed @cursor/sdk package is not supported by ASM",
        Some("tool_boundary") => "Cursor SDK could not confirm the ASM tool boundary",
        Some("usage_limit") => "Cursor SDK usage limit reached",
        _ => "Cursor SDK request failed",
    }
}

fn turn_outcome(result: Value, interrupted: bool) -> ExternalTurnOutcome {
    ExternalTurnOutcome {
        result: Some(result),
        interrupted,
        submitted_steering_count: 0,
        failure: None,
    }
}

fn prompt_frame(prompt: &Prompt) -> Result<Value> {
    let blocks = match prompt {
        Prompt::Text(text) => vec![json!({"type": "text", "text": text})],
        Prompt::Blocks(blocks) => blocks
            .iter()
            .map(|block| match block {
                InputBlock::Text { text } => json!({"type": "text", "text": text}),
                InputBlock::Image { mime_type, data } => {
                    json!({"type": "image", "mimeType": mime_type, "data": data})
                }
            })
            .collect(),
    };
    let frame = json!({"type": "prompt", "blocks": blocks});
    let limit = FramePolicy::CursorSdk.input_limit(&frame);
    ensure!(
        serde_json::to_vec(&frame)?.len() < limit,
        "Cursor input exceeds the SDK frame limit ({limit} bytes); reduce the image or prompt size"
    );
    Ok(frame)
}

#[cfg(all(test, target_os = "linux"))]
#[path = "cursor_tests.rs"]
mod tests;
