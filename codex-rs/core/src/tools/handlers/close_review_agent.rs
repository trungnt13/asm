use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde::Deserialize;
use std::collections::BTreeMap;

pub(crate) struct CloseReviewAgentHandler;

impl ToolExecutor<ToolInvocation> for CloseReviewAgentHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("close_agent")
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: "close_agent".to_string(),
            description: "Close a protected review member, releasing its live capacity after teardown. The tree root may close a reviewer subtree (including completed reviews); a reviewer may close its own explorer helper. Closing a helper never replenishes its total creation budget. Ordinary agents are not supported by this tool.".to_string(),
            strict: false, defer_loading: None,
            parameters: JsonSchema::object(BTreeMap::from([("target".to_string(), JsonSchema::string(Some("Agent path or ID.".to_string())))]), Some(vec!["target".to_string()]), Some(false.into())),
            output_schema: None,
        })
    }
    fn handle<'a>(&'a self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'a>
    where
        ToolInvocation: 'a,
    {
        Box::pin(async move {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Arguments {
                target: String,
            }
            let ToolPayload::Function { arguments } = invocation.payload else {
                return Err(FunctionCallError::RespondToModel(
                    "unsupported close payload".to_string(),
                ));
            };
            let args: Arguments = super::parse_arguments(&arguments)?;
            let target = crate::agent::agent_resolver::resolve_agent_target(
                &invocation.session,
                &invocation.turn,
                &args.target,
            )
            .await?;
            invocation.session.services.local_agent_runtime.control(invocation.session.session_id())
                .close_review_agent(invocation.session.thread_id, target).await
                .map_err(|_| FunctionCallError::RespondToModel("protected close denied or incomplete; capacity is retained on teardown failure".to_string()))?;
            Ok(boxed_tool_output(FunctionToolOutput::from_text(
                "Protected review member closed.".to_string(),
                Some(true),
            )))
        })
    }
}
impl CoreToolRuntime for CloseReviewAgentHandler {}
