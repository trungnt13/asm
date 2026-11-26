use crate::compact::AutoCompactionDisabled;
use crate::function_tool::FunctionCallError;
use crate::session::token_budget::ContextWindowAction;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::new_context_window_spec::NEW_CONTEXT_WINDOW_TOOL_NAME;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_protocol::openai_models::ToolMessage;
use codex_tools::ToolName;
use codex_tools::ToolSpec;

pub struct NewContextWindowHandler {
    spec: ToolSpec,
}

impl NewContextWindowHandler {
    pub(crate) fn new(action: ContextWindowAction, messages: Option<&ToolMessage>) -> Self {
        Self {
            spec: super::new_context_window_spec::create_new_context_window_tool(action, messages),
        }
    }
}

impl ToolExecutor<ToolInvocation> for NewContextWindowHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(NEW_CONTEXT_WINDOW_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(async move {
            if !matches!(invocation.payload, ToolPayload::Function { .. }) {
                return Err(FunctionCallError::RespondToModel(
                    "new_context handler received unsupported payload".to_string(),
                ));
            }

            if invocation
                .session
                .services
                .thread_extension_data
                .get::<AutoCompactionDisabled>()
                .is_some()
            {
                return Err(FunctionCallError::RespondToModel(
                    "Context changes are disabled for this thread.".to_string(),
                ));
            }

            invocation.session.request_new_context_window().await;

            let message = match invocation
                .turn
                .context_window_action(&invocation.step_context.settings.model_info)
            {
                ContextWindowAction::ResetWithNotes => {
                    "A new context window will start without summarizing conversation history."
                }
                ContextWindowAction::Summarize => "Context compaction requested to keep a summary.",
            };
            Ok(boxed_tool_output(FunctionToolOutput::from_text(
                message.to_string(),
                Some(true),
            )))
        })
    }
}

impl CoreToolRuntime for NewContextWindowHandler {}
