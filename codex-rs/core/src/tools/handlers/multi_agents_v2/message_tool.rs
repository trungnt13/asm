//! Shared argument parsing and dispatch for the v2 agent messaging tools.
//!
//! `send_message` and `followup_task` share the same submission path and differ only in whether the
//! resulting `InterAgentCommunication` should wake the target immediately.

use super::analytics::ToolCallAnalytics;
use super::*;
use crate::TurnStartOptions;
use crate::agent::api::AgentInput;
use crate::agent::api::AgentTarget;
use crate::agent::api::SendRequest;
use crate::agent::child_config::build_agent_resume_config;
use crate::agent::types::MessageDeliveryMode;
use crate::tools::context::FunctionToolOutput;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Input for the MultiAgentV2 `send_message` tool.
pub(crate) struct SendMessageArgs {
    pub(crate) target: String,
    pub(crate) message: Option<String>,
    pub(crate) external_message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Input for the MultiAgentV2 `followup_task` tool.
pub(crate) struct FollowupTaskArgs {
    pub(crate) target: String,
    pub(crate) message: Option<String>,
    pub(crate) external_message: Option<String>,
}

pub(super) fn message_content(message: String) -> Result<String, FunctionCallError> {
    if message.trim().is_empty() {
        return Err(FunctionCallError::RespondToModel(
            "Empty message can't be sent to an agent".to_string(),
        ));
    }
    Ok(message)
}

pub(super) fn message_from_arguments(
    message: Option<String>,
    external_message: Option<String>,
    external_backend: Option<&str>,
    source: &crate::tools::context::ToolCallSource,
) -> Result<crate::agent::types::AgentMessage, FunctionCallError> {
    match (message, external_message, external_backend) {
        (None, Some(message), Some(_)) => {
            let message = message_content(message)?;
            // A byte cap bounds even adversarial text below the context-item token ceiling.
            if message.len() > 8192 {
                return Err(FunctionCallError::RespondToModel(
                    "external_message must not exceed 8192 bytes".to_string(),
                ));
            }
            Ok(crate::agent::types::AgentMessage::Plaintext(message))
        }
        (Some(message), None, None) => {
            Ok(agent_message_from_tool(message_content(message)?, source))
        }
        (None, Some(_), None) => Err(FunctionCallError::RespondToModel(
            "external_message requires a configured external-runtime agent".to_string(),
        )),
        (Some(_), None, Some(_)) => Err(FunctionCallError::RespondToModel(
            "External-runtime agents require external_message, not encrypted message".to_string(),
        )),
        (Some(_), Some(_), _) | (None, None, _) => Err(FunctionCallError::RespondToModel(
            "Provide exactly one of message or external_message".to_string(),
        )),
    }
}

/// Handles the shared MultiAgentV2 message flow for both `send_message` and `followup_task`.
pub(super) async fn handle_message_string_tool(
    invocation: ToolInvocation,
    mode: MessageDeliveryMode,
    target: String,
    message: Option<String>,
    external_message: Option<String>,
    analytics: &mut ToolCallAnalytics,
) -> Result<FunctionToolOutput, FunctionCallError> {
    let message = message.map(message_content).transpose()?;
    let ToolInvocation {
        session,
        turn,
        call_id,
        source,
        ..
    } = invocation;
    let receiver_thread_id = resolve_agent_target(&session, &turn, &target).await?;
    let external_descriptor = if external_message.is_some() {
        session
            .services
            .agent_control
            .external_agent_descriptor(receiver_thread_id)
            .await
            .map_err(|err| collab_v2_agent_error(receiver_thread_id, err))?
    } else {
        None
    };
    let message = message_from_arguments(
        message,
        external_message,
        external_descriptor
            .as_ref()
            .map(|descriptor| descriptor.backend_id.as_str()),
        &source,
    )?;
    analytics.set_receiver(receiver_thread_id);
    let resume_config =
        build_agent_resume_config(&turn).map_err(FunctionCallError::RespondToModel)?;
    let receipt = session
        .services
        .agent_control
        .send(SendRequest {
            caller: session.thread_id,
            target: AgentTarget::Id(receiver_thread_id),
            resume_config,
            input: AgentInput::Message { message, mode },
            start_options: TurnStartOptions {
                parent_turn_id: (mode == MessageDeliveryMode::TriggerTurn)
                    .then(|| turn.sub_id.clone()),
                root_turn_id: turn.turn_metadata_state.root_turn_id(),
                turn_trigger: turn.turn_metadata_state.current_turn_trigger(),
                cyber_access_program: turn.cyber_access_program,
                ..Default::default()
            },
        })
        .await
        .map_err(|err| collab_v2_agent_error(receiver_thread_id, err))?;
    let receiver_agent_path = receipt.metadata.agent_path.ok_or_else(|| {
        FunctionCallError::RespondToModel("target agent is missing an agent_path".to_string())
    })?;
    emit_sub_agent_activity(
        &session,
        &turn,
        SubAgentActivityItem {
            model: None,
            reasoning_effort: None,
            id: call_id,
            agent_thread_id: receiver_thread_id,
            agent_path: receiver_agent_path,
            kind: SubAgentActivityKind::Interacted,
        },
    )
    .await;

    Ok(FunctionToolOutput::from_text(String::new(), Some(true)))
}
