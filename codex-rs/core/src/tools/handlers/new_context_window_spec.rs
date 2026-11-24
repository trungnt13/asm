use crate::session::token_budget::ContextWindowAction;
use codex_protocol::openai_models::ToolMessage;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use codex_tools::with_catalog_tool_message;
use std::collections::BTreeMap;

pub(crate) const NEW_CONTEXT_WINDOW_TOOL_NAME: &str = "new_context";

pub(super) fn create_new_context_window_tool(
    action: ContextWindowAction,
    messages: Option<&ToolMessage>,
) -> ToolSpec {
    let description = match action {
        ContextWindowAction::ResetWithNotes => {
            "Start a fresh context window. This discards the active conversation and any existing summary without making a new summary. Save important details in notes first. This does not change files."
        }
        ContextWindowAction::Summarize => {
            "Replace the active conversation with a summary. Use this when the conversation is large and you can continue without every detail. Summarizing uses tokens and may delay the next step, but can make later requests faster and cheaper."
        }
    };
    let tool = ResponsesApiTool {
        name: NEW_CONTEXT_WINDOW_TOOL_NAME.to_string(),
        description: String::new(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(BTreeMap::new(), /*required*/ None, Some(false.into())),
        output_schema: None,
    };
    let mut tool = with_catalog_tool_message(tool, messages);
    tool.description = description.to_string();
    ToolSpec::Function(tool)
}
