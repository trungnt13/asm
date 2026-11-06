use crate::session::turn_context::TurnContext;
use crate::tools::context::ToolInvocation;
use crate::tools::handlers::ToolSearchHandler;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use crate::tools::router::ToolRouter;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ModelInfo;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ResponsesApiTool;
use codex_tools::TOOL_SEARCH_TOOL_NAME;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::sync::Arc;

pub(crate) fn enabled(turn: &TurnContext, model: &ModelInfo) -> bool {
    turn.config.model_provider_id == "fireworks" && !model.supports_search_tool
}

pub(super) struct FunctionToolSearch {
    handler: Arc<ToolSearchHandler>,
}

impl FunctionToolSearch {
    pub(super) fn new(handler: Arc<ToolSearchHandler>) -> Self {
        Self { handler }
    }
}

impl ToolExecutor<ToolInvocation> for FunctionToolSearch {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(TOOL_SEARCH_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        let ToolSpec::ToolSearch {
            description,
            parameters,
            ..
        } = self.handler.spec()
        else {
            unreachable!("native search handler owns a search spec")
        };
        ToolSpec::Function(ResponsesApiTool {
            name: TOOL_SEARCH_TOOL_NAME.to_string(),
            description,
            parameters,
            strict: false,
            defer_loading: None,
            output_schema: None,
        })
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn handle<'a>(&'a self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'a>
    where
        ToolInvocation: 'a,
    {
        self.handler.handle(invocation)
    }
}

impl CoreToolRuntime for FunctionToolSearch {}

#[derive(Deserialize)]
struct SearchResult {
    tools: Vec<ToolName>,
}

pub(crate) fn specs_for_history(router: &ToolRouter, input: &[ResponseItem]) -> Arc<[ToolSpec]> {
    let mut search_calls = HashSet::new();
    let mut discovered = BTreeSet::new();
    for item in input {
        match item {
            ResponseItem::FunctionCall {
                name,
                namespace,
                call_id,
                ..
            } if name == TOOL_SEARCH_TOOL_NAME
                && ToolName::new(namespace.clone(), name.clone()).is_default_namespace() =>
            {
                search_calls.insert(call_id.as_str());
            }
            ResponseItem::FunctionCallOutput {
                call_id: Some(call_id),
                output,
                ..
            } if search_calls.contains(call_id.as_str()) => {
                if let Some(text) = output.body.to_text()
                    && let Ok(result) = serde_json::from_str::<SearchResult>(&text)
                {
                    discovered.extend(
                        result
                            .tools
                            .into_iter()
                            .map(ToolName::with_default_namespace),
                    );
                }
            }
            _ => {}
        }
    }

    let mut specs = router.model_visible_specs().to_vec();
    for name in discovered {
        let Some(tool) = router
            .registered_tool(&name)
            .filter(|tool| tool.exposure.is_deferred())
        else {
            continue;
        };
        // Re-resolve saved discoveries against the current policy and definitions.
        let mut spec = Arc::unwrap_or_clone(tool.spec());
        match &mut spec {
            ToolSpec::Function(function) => function.defer_loading = None,
            ToolSpec::Namespace(namespace) => {
                namespace.tools.retain_mut(|tool| match tool {
                    ResponsesApiNamespaceTool::Function(function) => {
                        function.defer_loading = None;
                        function.name == name.name
                    }
                    ResponsesApiNamespaceTool::Custom(_) => false,
                });
            }
            ToolSpec::Freeform(_) | ToolSpec::ToolSearch { .. } | ToolSpec::WebSearch { .. } => {
                continue;
            }
        }
        specs.push(spec);
    }
    let mcp_namespaces = router.mcp_namespaces().map(|(_, name)| name).collect();
    super::spec_plan::merge_into_namespaces(specs, &mcp_namespaces).into()
}
