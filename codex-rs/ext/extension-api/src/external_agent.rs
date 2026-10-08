//! Trusted external executors for native child threads.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use codex_protocol::ThreadId;
use codex_protocol::protocol::TokenUsage;
use codex_utils_absolute_path::AbsolutePathBuf;
use futures::future::BoxFuture;
use serde::Deserialize;
use serde::Serialize;

/// Opens a runtime without exposing Core sessions or native tool execution.
/// Implementations own transport and must reject unsupported launch requests.
/// Boxed futures permit host-installed, object-safe implementations.
pub trait ExternalAgentBackend: Send + Sync {
    fn open(
        &self,
        launch: ExternalAgentLaunch,
    ) -> BoxFuture<'_, Result<Arc<dyn ExternalAgentRuntime>, ExternalAgentLaunchError>>;
}

/// A rejected launch leaves no owned execution; an unsettled launch may still run.
#[derive(Debug)]
pub enum ExternalAgentLaunchError {
    Rejected(String),
    Unsettled(String),
}

/// Controls an owned runtime and receives passive observations.
/// Submission acknowledges acceptance, not execution or task verification.
/// Interrupt and shutdown must settle owned execution before returning success.
pub trait ExternalAgentRuntime: Send + Sync {
    fn submit(&self, input: ExternalAgentInput) -> BoxFuture<'_, Result<(), String>>;
    fn next_event(&self) -> BoxFuture<'_, Result<Option<ExternalAgentEvent>, String>>;
    fn interrupt<'a>(&'a self, turn_id: &'a str) -> BoxFuture<'a, Result<(), String>>;
    fn shutdown(&self) -> BoxFuture<'_, Result<(), String>>;
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExternalAgentLaunchMode {
    #[default]
    New,
    Resume,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalAgentLaunch {
    pub thread_id: ThreadId,
    pub runtime_session_id: ThreadId,
    pub cwd: AbsolutePathBuf,
    pub state_dir: PathBuf,
    pub model: String,
    /// Captured, filtered execution environment; never inherit the host implicitly.
    pub env: HashMap<String, String>,
    /// Immutable guidance pages; publish each separately with bounded framing.
    /// The runtime must expose all pages before acknowledging the first task.
    pub instructions: Vec<String>,
    pub mode: ExternalAgentLaunchMode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalAgentInputKind {
    Task,
    Message,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalAgentInput {
    pub turn_id: String,
    pub id: String,
    pub sender: String,
    pub text: String,
    pub kind: ExternalAgentInputKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalAgentEvent {
    pub id: String,
    pub turn_id: String,
    pub kind: ExternalObservation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
pub enum ExternalObservation {
    AssistantMessage {
        id: String,
        text: String,
    },
    ToolStarted {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
    ToolFinished {
        id: String,
        output: String,
        error: Option<String>,
    },
    ReadyToFinish {
        text: String,
    },
    Usage(TokenUsage),
    Notice(String),
    Failed(String),
    Closed,
}
