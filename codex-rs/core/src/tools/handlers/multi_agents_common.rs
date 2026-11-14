use crate::agent::AgentStatus;
use crate::agent::types::SpawnAgentForkMode;
use crate::config::DEFAULT_MULTI_AGENT_V2_MIN_WAIT_TIMEOUT_MS;
use crate::config::HARD_MAX_MULTI_AGENT_V2_TIMEOUT_MS;
use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use codex_otel::SessionTelemetry;
use codex_protocol::ThreadId;
use codex_protocol::error::CodexErr;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::items::CollabAgentToolCallStatus;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::protocol::MultiAgentVersion;
use serde::Serialize;
use serde_json::Value as JsonValue;

/// Minimum wait timeout to prevent tight polling loops from burning CPU.
pub(crate) const MIN_WAIT_TIMEOUT_MS: i64 = DEFAULT_MULTI_AGENT_V2_MIN_WAIT_TIMEOUT_MS;
pub(crate) const DEFAULT_WAIT_TIMEOUT_MS: i64 = 30_000;
pub(crate) const MAX_WAIT_TIMEOUT_MS: i64 = HARD_MAX_MULTI_AGENT_V2_TIMEOUT_MS;
pub(crate) fn function_arguments(payload: ToolPayload) -> Result<String, FunctionCallError> {
    match payload {
        ToolPayload::Function { arguments } => Ok(arguments),
        _ => Err(FunctionCallError::RespondToModel(
            "collab handler received unsupported payload".to_string(),
        )),
    }
}

pub(crate) fn tool_output_json_text<T>(value: &T, tool_name: &str) -> String
where
    T: Serialize,
{
    serde_json::to_string(value).unwrap_or_else(|err| {
        JsonValue::String(format!("failed to serialize {tool_name} result: {err}")).to_string()
    })
}

pub(crate) fn tool_output_response_item<T>(
    call_id: &str,
    payload: &ToolPayload,
    value: &T,
    success: Option<bool>,
    tool_name: &str,
) -> ResponseInputItem
where
    T: Serialize,
{
    FunctionToolOutput::from_text(tool_output_json_text(value, tool_name), success)
        .to_response_item(call_id, payload)
}

pub(crate) fn tool_output_code_mode_result<T>(value: &T, tool_name: &str) -> JsonValue
where
    T: Serialize,
{
    serde_json::to_value(value).unwrap_or_else(|err| {
        JsonValue::String(format!("failed to serialize {tool_name} result: {err}"))
    })
}

pub(crate) fn collab_spawn_error(err: CodexErr) -> FunctionCallError {
    match err.details() {
        CodexErrorDetails::UnsupportedOperation(message) if message == "thread manager dropped" => {
            FunctionCallError::RespondToModel("collab manager unavailable".to_string())
        }
        CodexErrorDetails::UnsupportedOperation(message) => {
            FunctionCallError::RespondToModel(message.clone())
        }
        _ => FunctionCallError::RespondToModel(format!("collab spawn failed: {err}")),
    }
}

pub(crate) fn record_collab_spawn_failure(
    telemetry: &SessionTelemetry,
    product_sku: Option<&str>,
    err: &CodexErr,
    call_id: &str,
    turn_id: &str,
    fork_mode: Option<&SpawnAgentForkMode>,
    multi_agent_version: MultiAgentVersion,
) {
    let reason = match err.details() {
        CodexErrorDetails::AgentLimitReached { .. } => "limit_reached",
        CodexErrorDetails::InvalidRequest(_) => "invalid_request",
        CodexErrorDetails::ThreadNotFound(_) => "thread_not_found",
        CodexErrorDetails::UnsupportedOperation(_) => "unsupported_operation",
        _ => "internal",
    };
    let fork_mode = match fork_mode {
        None => "none",
        Some(SpawnAgentForkMode::FullHistory) => "all",
    };
    let multi_agent_version = match multi_agent_version {
        MultiAgentVersion::Disabled => "disabled",
        MultiAgentVersion::V1 => "v1",
        MultiAgentVersion::V2 => "v2",
    };
    telemetry
        .clone()
        .with_product_sku(product_sku)
        .record_multi_agent_spawn_failure(
            reason,
            err,
            call_id,
            turn_id,
            fork_mode,
            multi_agent_version,
        );
}

pub(crate) fn collab_agent_error(agent_id: ThreadId, err: CodexErr) -> FunctionCallError {
    match err.details() {
        CodexErrorDetails::ThreadNotFound(id) => {
            FunctionCallError::RespondToModel(format!("agent with id {id} not found"))
        }
        CodexErrorDetails::InternalAgentDied => {
            FunctionCallError::RespondToModel(format!("agent with id {agent_id} is closed"))
        }
        CodexErrorDetails::UnsupportedOperation(_) => {
            FunctionCallError::RespondToModel("collab manager unavailable".to_string())
        }
        _ => FunctionCallError::RespondToModel(format!("collab tool failed: {err}")),
    }
}

/// Keeps V2 request validation distinct from a dropped local manager.
pub(crate) fn collab_v2_agent_error(agent_id: ThreadId, err: CodexErr) -> FunctionCallError {
    match err.details() {
        CodexErrorDetails::UnsupportedOperation(message) if message != "thread manager dropped" => {
            FunctionCallError::RespondToModel(message.clone())
        }
        _ => collab_agent_error(agent_id, err),
    }
}

pub(crate) fn collab_tool_call_status(
    status: &AgentStatus,
    receiver_thread_id: Option<ThreadId>,
) -> CollabAgentToolCallStatus {
    match status {
        AgentStatus::Errored(_) | AgentStatus::NotFound => CollabAgentToolCallStatus::Failed,
        _ if receiver_thread_id.is_some() => CollabAgentToolCallStatus::Completed,
        _ => CollabAgentToolCallStatus::Failed,
    }
}
