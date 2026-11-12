//! Rejects retired V1 execution while preserving legacy tool names and schemas.

use crate::agent::AgentStatus;
use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::multi_agents_spec::MULTI_AGENT_V1_NAMESPACE;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_protocol::ThreadId;
use codex_protocol::items::CollabAgentToolCallStatus;
use codex_tools::ToolName;
use codex_tools::ToolSearchInfo;
use codex_tools::ToolSearchSourceInfo;

const MULTI_AGENT_TOOL_SEARCH_SOURCE_NAME: &str = "Multi-agent tools";
const MULTI_AGENT_TOOL_SEARCH_SOURCE_DESCRIPTION: &str = "Spawn and manage sub-agents.";

fn retired_v1_execution_error() -> FunctionCallError {
    FunctionCallError::RespondToModel(
        "Multi-agent V1 execution is unsupported in this build; explicitly enable features.multi_agent_v2 to use V2 tools. Saved history remains readable.".to_string(),
    )
}

fn multi_agent_tool_search_info(
    search_text: &str,
    spec: codex_tools::ToolSpec,
) -> Option<ToolSearchInfo> {
    ToolSearchInfo::from_spec(
        search_text.to_string(),
        spec,
        Some(ToolSearchSourceInfo {
            name: MULTI_AGENT_TOOL_SEARCH_SOURCE_NAME.to_string(),
            description: Some(MULTI_AGENT_TOOL_SEARCH_SOURCE_DESCRIPTION.to_string()),
        }),
    )
}

pub(crate) use close_agent::Handler as CloseAgentHandler;
pub(crate) use resume_agent::Handler as ResumeAgentHandler;
pub(crate) use send_input::Handler as SendInputHandler;
pub(crate) use spawn::Handler as SpawnAgentHandler;
pub(crate) use wait::Handler as WaitAgentHandler;

pub(crate) mod close_agent;
mod resume_agent;
mod send_input;
mod spawn;
pub(crate) mod wait;

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

#[cfg(test)]
#[path = "multi_agents_tests.rs"]
mod tests;
