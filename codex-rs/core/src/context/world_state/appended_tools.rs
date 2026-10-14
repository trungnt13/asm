use super::PreviousSectionState;
use super::SectionTransition;
use super::WorldStateSection;
use super::WorldStateUpdate;
use crate::context::DeveloperInstructions;
use codex_protocol::ResponseItemId;
use codex_protocol::dynamic_tools::DynamicToolSpec;
use codex_protocol::models::ResponseItem;
use serde::Deserialize;
use serde::Serialize;

const APPENDED_TOOL_ID_PREFIX: &str = "at_appended_";
pub(crate) const APPENDED_TOOL_REGISTRY: &str = "appended_tool_registry";

pub(crate) fn is_appended_declaration(item: &ResponseItem) -> bool {
    matches!(item, ResponseItem::AdditionalTools { id: Some(id), .. }
        if id.as_str().starts_with(APPENDED_TOOL_ID_PREFIX))
}

pub(crate) struct AppendedToolsState {
    specs: Vec<DynamicToolSpec>,
    definitions: Vec<serde_json::Value>,
    available: bool,
    namespace_description: String,
}

impl AppendedToolsState {
    pub(crate) fn new(
        specs: Vec<DynamicToolSpec>,
        baseline: &[codex_tools::ToolSpec],
        available: bool,
    ) -> codex_protocol::error::Result<Self> {
        let definitions = specs
            .iter()
            .map(|spec| {
                let DynamicToolSpec::Function(tool) = spec else {
                    return Err(codex_protocol::error::CodexErr::InvalidRequest(
                        "appended tools must be standalone functions".to_string(),
                    ));
                };
                let tool =
                    codex_tools::dynamic_tool_to_responses_api_tool(tool).map_err(|error| {
                        codex_protocol::error::CodexErr::InvalidRequest(error.to_string())
                    })?;
                serde_json::to_value(codex_tools::ToolSpec::Function(tool)).map_err(|error| {
                    codex_protocol::error::CodexErr::InvalidRequest(error.to_string())
                })
            })
            .collect::<codex_protocol::error::Result<Vec<_>>>()?;
        let namespace_description = baseline
            .iter()
            .find_map(|spec| {
                if let codex_tools::ToolSpec::Namespace(namespace) = spec
                    && namespace.name == codex_protocol::DEFAULT_FUNCTION_NAMESPACE
                    && !namespace.description.trim().is_empty()
                {
                    Some(namespace.description.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| {
                codex_tools::default_namespace_description(
                    codex_protocol::DEFAULT_FUNCTION_NAMESPACE,
                )
            });
        let state = Self {
            specs,
            definitions,
            available,
            namespace_description,
        };
        // Any later diff is a subset of this complete, bounded declaration.
        if !state.specs.is_empty() {
            crate::context::validate_injected_item_size(
                &state.declaration(state.definitions.clone()),
            )?;
        }
        Ok(state)
    }

    fn declaration(&self, tools: Vec<serde_json::Value>) -> ResponseItem {
        ResponseItem::AdditionalTools {
            id: Some(ResponseItemId::new("at_appended")),
            role: "developer".to_string(),
            tools: vec![serde_json::json!({
                "type": "namespace",
                "name": codex_protocol::DEFAULT_FUNCTION_NAMESPACE,
                "description": self.namespace_description,
                "tools": tools,
            })],
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct AppendedToolsSnapshot {
    specs: Vec<DynamicToolSpec>,
    available: bool,
}

impl WorldStateSection for AppendedToolsState {
    const ID: &'static str = "appended_tools";
    type Snapshot = AppendedToolsSnapshot;

    fn render_diff(
        &self,
        previous: PreviousSectionState<'_, Self::Snapshot>,
    ) -> SectionTransition<Self::Snapshot> {
        if self.specs.is_empty() {
            return (None, Vec::new());
        }
        let known = match previous {
            PreviousSectionState::Known(snapshot) => Some(snapshot),
            PreviousSectionState::Absent | PreviousSectionState::Unknown => None,
        };
        let mut updates = Vec::new();
        if !self.available {
            if known.is_none_or(|known| known.available) {
                updates.push(WorldStateUpdate::fragment(DeveloperInstructions::new(
                    "Appended tools are unavailable with the current model or tool mode. They require Responses Lite without strict third-party Code Mode Only.".to_string(),
                )).standalone());
            }
        } else {
            let tools = self
                .specs
                .iter()
                .zip(&self.definitions)
                .filter(|(spec, _)| {
                    known.is_none_or(|known| !known.available || !known.specs.contains(spec))
                })
                .map(|(_, definition)| definition.clone())
                .collect::<Vec<_>>();
            if !tools.is_empty() {
                updates.push(WorldStateUpdate::prefix_item(self.declaration(tools)));
            }
        }
        (
            Some(AppendedToolsSnapshot {
                specs: self.specs.clone(),
                available: self.available,
            }),
            updates,
        )
    }
}
