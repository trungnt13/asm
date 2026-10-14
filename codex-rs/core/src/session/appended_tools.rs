use super::TurnContext;
use super::session::Session;
use crate::context::world_state::APPENDED_TOOL_REGISTRY;
use crate::tools::handlers::DynamicToolHandler;
use crate::tools::registry::ToolExposure;
use crate::tools::router::ToolRouter;
use codex_history::InitialHistory;
use codex_history::RolloutItem;
use codex_protocol::dynamic_tools::DynamicToolSpec;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::protocol::WorldStateItem;
use codex_tools::ToolName;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub(super) fn restore_appended_tools(history: &InitialHistory) -> Result<Vec<DynamicToolSpec>> {
    // Admission/checkpoint state outranks a subsequently rendered, older step snapshot.
    let items = history.get_rollout_items();
    let registry = items.iter().rev().find_map(|item| {
        if let RolloutItem::WorldState(state) = item {
            state
                .state
                .get(APPENDED_TOOL_REGISTRY)
                .filter(|value| !value.is_null())
        } else {
            None
        }
    });
    let snapshot = || {
        items.iter().rev().find_map(|item| {
            if let RolloutItem::WorldState(state) = item {
                state
                    .state
                    .get("appended_tools")
                    .and_then(|value| value.get("specs"))
            } else {
                None
            }
        })
    };
    if let Some(value) = registry.or_else(snapshot) {
        return serde_json::from_value(value.clone())
            .map_err(|error| CodexErr::InvalidRequest(error.to_string()));
    }
    Ok(Vec::new())
}

// Retain the first advertised spec when only its description changes. Argument
// schemas and loading behavior must still agree with the executor's contract.
fn same_appended_tool_contract(left: &DynamicToolSpec, right: &DynamicToolSpec) -> bool {
    match (left, right) {
        (DynamicToolSpec::Function(left), DynamicToolSpec::Function(right)) => {
            left.name == right.name
                && left.input_schema == right.input_schema
                && left.defer_loading == right.defer_loading
        }
        (DynamicToolSpec::Namespace(_), _) | (_, DynamicToolSpec::Namespace(_)) => false,
    }
}

impl Session {
    #[tracing::instrument(level = "trace", skip_all)]
    pub(crate) async fn is_appended_tool(&self, name: &str) -> bool {
        self.state
            .lock()
            .await
            .session_configuration
            .appended_tools
            .iter()
            .any(|spec| matches!(spec, DynamicToolSpec::Function(tool) if tool.name == name))
    }

    #[tracing::instrument(level = "trace", skip_all)]
    pub(crate) async fn append_tools(self: &Arc<Self>, tools: Vec<DynamicToolSpec>) -> Result<()> {
        // Serialize append admission with other append calls and compaction checkpoints.
        let _permit = self
            .thread_settings_persistence
            .acquire()
            .await
            .map_err(|error| CodexErr::InvalidRequest(error.to_string()))?;
        let mut appended = self
            .state
            .lock()
            .await
            .session_configuration
            .appended_tools
            .clone();
        let registered_count = appended.len();
        let compatible_repeat = !tools.is_empty()
            && tools.iter().all(|spec| {
                appended
                    .iter()
                    .any(|existing| same_appended_tool_contract(existing, spec))
            });
        let context = if compatible_repeat {
            // Validate current model/mode without discovering skills, AGENTS.md or MCP tools.
            // This recording-only context is never used to capture or execute a step.
            self.new_inject_items_context().await
        } else {
            self.new_default_turn().await
        };
        if !context.model_info().use_responses_lite
            || crate::tools::spec_plan::code_mode_only_strict_3p_tools(
                &context,
                context.model_info(),
            )
        {
            return Err(CodexErr::InvalidRequest(
                "append-only tools require Responses Lite without strict third-party Code Mode Only".to_string(),
            ));
        }
        if tools.is_empty() {
            return Err(CodexErr::InvalidRequest(
                "tools must not be empty".to_string(),
            ));
        }
        for spec in tools {
            let DynamicToolSpec::Function(tool) = &spec else {
                return Err(CodexErr::InvalidRequest(
                    "appended tools must be standalone functions".to_string(),
                ));
            };
            let name = ToolName::plain(tool.name.clone());
            if tool.defer_loading
                || matches!(tool.name.as_str(), "exec_command" | "shell_command")
                || !self.tool_policy.allows(&name)
            {
                return Err(CodexErr::InvalidRequest(
                    "appended tools must be allowed, directly callable functions".to_string(),
                ));
            }
            codex_tools::dynamic_tool_to_responses_api_tool(tool)
                .map_err(|error| CodexErr::InvalidRequest(error.to_string()))?;
            if appended
                .iter()
                .any(|existing| same_appended_tool_contract(existing, &spec))
            {
                continue;
            }
            if appended.iter().any(|existing| matches!(existing, DynamicToolSpec::Function(existing) if existing.name == tool.name))
            {
                return Err(CodexErr::InvalidRequest(format!("tool already registered: {}", tool.name)));
            }
            appended.push(spec);
        }
        if appended.len() == registered_count {
            return Ok(());
        }
        let step = self
            .capture_speculative_step_context(context, &CancellationToken::new())
            .await?;
        if !step.tool_router.appended_tools_available {
            return Err(CodexErr::InvalidRequest(
                "append-only tools require Responses Lite without strict third-party Code Mode Only".to_string(),
            ));
        }
        for spec in &appended[registered_count..] {
            if let DynamicToolSpec::Function(tool) = spec
                && step
                    .tool_router
                    .registered_tool(&ToolName::plain(tool.name.clone()))
                    .is_some()
            {
                return Err(CodexErr::InvalidRequest(format!(
                    "tool already registered: {}",
                    tool.name
                )));
            }
        }
        crate::context::world_state::AppendedToolsState::new(
            appended.clone(),
            &step.tool_router.model_visible_specs(),
            /*available*/ true,
        )?;
        let value = serde_json::to_value(&appended)
            .map_err(|error| CodexErr::InvalidRequest(error.to_string()))?;
        let patch = WorldStateItem::patch(
            [(APPENDED_TOOL_REGISTRY.to_string(), value)]
                .into_iter()
                .collect(),
        );
        // Keep persistence and publication atomic to readers and cancellation. The shared
        // permit also prevents compaction/settings checkpoints from overtaking admission.
        let mut state = self.state.lock().await;
        if !self
            .persist_rollout_items(&[RolloutItem::WorldState(patch)])
            .await
        {
            return Err(CodexErr::InvalidRequest(
                "failed to persist appended tools".to_string(),
            ));
        }
        state.session_configuration.appended_tools = appended;
        // The next captured step emits declarations and registers handlers together.
        Ok(())
    }

    #[tracing::instrument(level = "trace", skip_all)]
    pub(crate) async fn register_appended_tools(
        &self,
        turn_context: &TurnContext,
        model: &ModelInfo,
        router: &mut ToolRouter,
    ) -> Result<()> {
        let specs = self
            .state
            .lock()
            .await
            .session_configuration
            .appended_tools
            .clone();
        router.appended_tools = specs.clone();
        router.appended_tools_available = model.use_responses_lite
            && !crate::tools::spec_plan::code_mode_only_strict_3p_tools(turn_context, model);
        if !router.appended_tools_available {
            return Ok(());
        }
        for spec in &specs {
            let DynamicToolSpec::Function(tool) = spec else {
                return Err(CodexErr::InvalidRequest(
                    "appended tools must be standalone functions".to_string(),
                ));
            };
            let handler = DynamicToolHandler::new(tool).ok_or_else(|| {
                CodexErr::InvalidRequest("invalid appended tool schema".to_string())
            })?;
            // Keep baseline tools, Code Mode declarations and namespace metadata untouched.
            if !router
                .registry
                .register_external_with_exposure(Arc::new(handler), ToolExposure::Hidden)
            {
                return Err(CodexErr::InvalidRequest(format!(
                    "cannot register appended tool: {}",
                    tool.name
                )));
            }
        }
        Ok(())
    }
}
