use super::*;
use crate::agent::agent_resolver::resolve_agent_target;
use crate::function_tool::FunctionCallError::RespondToModel;
use crate::tools::context::FunctionToolOutput;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use std::collections::BTreeMap;

pub(crate) struct ProtectedReviewTool(&'static str, &'static str, &'static str);
pub(crate) const SPAWN_REVIEW: ProtectedReviewTool = ProtectedReviewTool(
    "spawn_review",
    "message",
    "Start a fresh protected reviewer with trusted settings. Use list_agents/wait_agent for its report, then close_agent.",
);
pub(crate) const READ_CONTEXT: ProtectedReviewTool = ProtectedReviewTool(
    "read_review_context",
    "cursor",
    "Read pinned evidence. Start with cursor: null (JSON null, not a string); then pass next_cursor unchanged; concatenate JSONL fragments. Retained supplements can repeat IDs, not global chronology.",
);
pub(crate) const EXPLORE: ProtectedReviewTool = ProtectedReviewTool(
    "review_explore",
    "message",
    "Ask a trusted explorer one evidence question. Waits for its result and closes it. Each creation consumes lifetime budget.",
);
pub(crate) const CLOSE_REVIEW: ProtectedReviewTool = ProtectedReviewTool(
    "close_agent",
    "target",
    "Close your reviewer and drain its private evidence jobs; cannot close ordinary agents.",
);

impl ToolExecutor<ToolInvocation> for ProtectedReviewTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(self.0)
    }

    fn spec(&self) -> ToolSpec {
        let argument = if self.0 == READ_CONTEXT.0 {
            JsonSchema::any_of(
                vec![JsonSchema::string(None), JsonSchema::null(None)],
                Some("Use JSON null for the first page; otherwise copy the exact next_cursor string from the previous page. Never invent a cursor or use strings such as none, null, or empty.".into()),
            )
        } else {
            JsonSchema::string(None)
        };
        ToolSpec::Function(ResponsesApiTool {
            name: self.0.into(),
            description: self.2.into(),
            strict: false,
            defer_loading: None,
            parameters: JsonSchema::object(
                [(self.1.into(), argument)].into(),
                Some(vec![self.1.into()]),
                Some(false.into()),
            ),
            output_schema: None,
        })
    }

    fn handle<'a>(&'a self, mut call: ToolInvocation) -> codex_tools::ToolExecutorFuture<'a>
    where
        ToolInvocation: 'a,
    {
        Box::pin(async move {
            let arguments = function_arguments(call.payload.clone())?;
            let mut args: BTreeMap<String, Option<String>> = parse_arguments(&arguments)?;
            let value = args.remove(self.1).flatten();
            if !args.is_empty() {
                return Err(RespondToModel("unexpected review arguments".into()));
            }
            if self.0 == SPAWN_REVIEW.0 {
                call.payload = crate::tools::context::ToolPayload::Function {
                    arguments: serde_json::json!({
                        "message": value.ok_or_else(|| RespondToModel("missing review assignment".into()))?,
                        "task_name": format!("review_{}", uuid::Uuid::new_v4().simple()),
                        "agent_type": "reviewer", "fork_turns": "none",
                    }).to_string(),
                };
                return crate::tools::handlers::multi_agents_v2::SpawnAgentHandler::default()
                    .handle(call)
                    .await;
            }
            let session = call.session;
            let id = session.thread_id;
            let runtime = &session.services.local_agent_runtime;
            let control = runtime.control(session.session_id());
            let review = || {
                session
                    .review_context()
                    .ok_or_else(|| RespondToModel("not a reviewer".into()))
            };
            let result = match (self.0, value) {
                ("read_review_context", cursor) => review()?.page(cursor.as_deref()),
                ("review_explore", Some(message)) if !message.trim().is_empty() => review()?
                    .explore(control, call.step_context, message)
                    .await
                    .map_err(|e| e.to_string()),
                ("close_agent", Some(target)) => {
                    let target = resolve_agent_target(&session, &call.turn, &target).await?;
                    control
                        .close_review_agent(id, target)
                        .await
                        .map(|()| "Protected review member closed.".into())
                        .map_err(|e| e.to_string())
                }
                _ => Err("missing review argument".into()),
            };
            let text = result.map_err(RespondToModel)?;
            Ok(boxed_tool_output(FunctionToolOutput::from_text(
                text,
                Some(true),
            )))
        })
    }
}
impl CoreToolRuntime for ProtectedReviewTool {}
