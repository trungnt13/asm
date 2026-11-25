//! Outgoing context-tool definitions follow the captured acting model.

use super::*;
use codex_protocol::openai_models::ContextManagementToolMessages;
use codex_protocol::openai_models::ModelInfo;

fn configure_tools(model: &mut ModelInfo, description: &str) {
    let message = ToolMessage {
        description: Some(description.into()),
        parameters: Some(r#"{"type":"object","properties":{},"additionalProperties":false,"description":"Catalog arguments."}"#.into()),
    };
    model
        .model_messages
        .get_or_insert_default()
        .tools
        .get_or_insert_default()
        .context_management = Some(ContextManagementToolMessages {
        new_context: Some(message.clone()),
        get_context_remaining: Some(message),
    });
}
