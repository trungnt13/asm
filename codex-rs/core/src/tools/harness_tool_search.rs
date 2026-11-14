use crate::session::turn_context::TurnContext;
use crate::tools::context::ToolInvocation;
use crate::tools::handlers::ToolSearchHandler;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use crate::tools::router::ToolRouter;
use codex_prompts::ResolvedModelMessages;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ModelInfo;
use codex_tools::IndirectNamespacePrefixes;
use codex_tools::LoadableToolSpec;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ResponsesApiTool;
use codex_tools::TOOL_SEARCH_TOOL_NAME;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use codex_tools::coalesce_loadable_tool_specs;
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

pub(crate) fn specs_for_history<T: std::borrow::Borrow<ResponseItem>>(
    router: &ToolRouter,
    input: &[T],
    model: &ModelInfo,
) -> Arc<[ToolSpec]> {
    let mut search_calls = HashSet::new();
    let mut discovered = BTreeSet::new();
    for item in input {
        match item.borrow() {
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

    let mut selected_specs = Vec::new();
    for name in discovered {
        let Some(tool) = router
            .registered_tool(&name)
            .filter(|tool| tool.exposure.is_deferred())
        else {
            continue;
        };
        // Re-resolve saved discoveries against the current policy and definitions.
        let spec = match Arc::unwrap_or_clone(tool.spec()) {
            ToolSpec::Function(mut function) => {
                function.defer_loading = None;
                LoadableToolSpec::Function(function)
            }
            ToolSpec::Namespace(mut namespace) => {
                namespace.tools.retain_mut(|tool| match tool {
                    ResponsesApiNamespaceTool::Function(function) => {
                        function.defer_loading = None;
                        function.name == name.name
                    }
                    ResponsesApiNamespaceTool::Custom(_) => false,
                });
                LoadableToolSpec::Namespace(namespace)
            }
            ToolSpec::Freeform(_) | ToolSpec::ToolSearch { .. } | ToolSpec::WebSearch { .. } => {
                continue;
            }
        };
        selected_specs.push(spec);
    }
    let model_messages = ResolvedModelMessages::from_model(model);
    let indirect_prefixes = IndirectNamespacePrefixes::new(
        model_messages.indirect_description_prefixes(),
        router.mcp_namespaces(),
    )
    .expect("tool router validates indirect namespace prefixes");
    let mut selected_specs = coalesce_loadable_tool_specs(selected_specs);
    indirect_prefixes.apply_search(&mut selected_specs);
    let mut specs = router.model_visible_specs().to_vec();
    specs.extend(selected_specs.into_iter().map(ToolSpec::from));
    let mcp_namespaces = router.mcp_namespaces().map(|(_, name)| name).collect();
    super::spec_plan::merge_into_namespaces(specs, &mcp_namespaces).into()
}
