//! Keeps spawn model choices in append-only context instead of the tool schema.
//! Snapshots compare the bounded text actually shown to the model; catalog refreshes
//! never rewrite earlier messages, and missing retained context gets a fresh catalog.

use super::PreviousSectionState;
use super::SectionTransition;
use super::WorldStateHash;
use super::WorldStateSection;
use super::WorldStateUpdate;
use crate::agent::model_catalog::CONTEXT_WINDOW_SCOPE;
use crate::agent::model_catalog::context_window_label;
use crate::config::Config;
use crate::context::ContextualUserFragment;
use crate::context::environment_context::push_xml_escaped_text;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ContentItemKind;
use codex_protocol::models::ContentItemMetadata;
use codex_protocol::models::ContentItemNamespace;
use codex_protocol::models::input_text;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::protocol::MultiAgentVersion;

// Includes XML escaping and markers; at most 1,000 tokens even with byte fallback.
const MAX_RENDERED_BYTES: usize = 1_000;
const MAX_DESCRIPTION_CHARS: usize = 250;
const OMITTED_NOTICE: &str = "Additional model choices omitted.\n";

/// The current model choices, or an empty catalog to invalidate a retained listing.
#[derive(Clone, Default)]
pub(crate) struct ModelCatalogState {
    catalog: String,
    namespace: Option<ContentItemNamespace>,
}

impl ModelCatalogState {
    pub(crate) fn new(
        models: &[ModelPreset],
        multi_agent_version: MultiAgentVersion,
        config: &Config,
        active_models: &[ModelInfo],
    ) -> Self {
        let configured = config.multi_agent_v2.subagent_model_list.as_deref();
        let mut catalog = String::new();
        let mut models = match crate::agent::model_catalog::advertised_models(
            models,
            multi_agent_version,
            configured,
        ) {
            Ok(models) => models,
            Err(error) => {
                push_xml_escaped_text(&mut catalog, &error);
                return Self {
                    catalog,
                    namespace: None,
                };
            }
        };
        if multi_agent_version != MultiAgentVersion::V2 || configured.is_none() {
            // Preserve stable automatic ordering without reordering explicit choices.
            models.sort_by(|left, right| left.model.cmp(&right.model));
        }
        if models.is_empty() {
            if multi_agent_version == MultiAgentVersion::V2 && configured.is_some() {
                catalog.push_str("No model overrides are advertised by subagent_model_list.\n");
            } else {
                catalog.push_str("No picker-visible model overrides are currently loaded.\n");
            }
        } else {
            catalog.push_str(CONTEXT_WINDOW_SCOPE);
            catalog.push('\n');
        }
        let (open, close) = Self::type_markers();
        let body_budget = MAX_RENDERED_BYTES - open.len() - close.len() - 2;
        // Reserve model IDs and capability labels before spending the budget on descriptions.
        let model_count = models.len();
        let mut rows = Vec::new();
        let mut remaining = body_budget - catalog.len();
        for model in models {
            let mut line =
                context_window_label(config, multi_agent_version, active_models, &model.model);
            if !model.supported_reasoning_efforts.is_empty() {
                line.push_str(" Reasoning efforts: ");
                for (index, preset) in model.supported_reasoning_efforts.iter().enumerate() {
                    if index > 0 {
                        line.push_str(", ");
                    }
                    line.push_str(preset.effort.as_str());
                    if preset.effort == model.default_reasoning_effort {
                        line.push_str(" (default)");
                    }
                }
                line.push('.');
            }
            if !model.service_tiers.is_empty() {
                line.push_str(" Service tiers: ");
                for (index, tier) in model.service_tiers.iter().enumerate() {
                    if index > 0 {
                        line.push_str(", ");
                    }
                    line.push_str(tier.id.as_str());
                }
                line.push('.');
            }
            let mut prefix = String::new();
            let mut details = String::new();
            push_xml_escaped_text(&mut prefix, &format!("- `{}`: ", model.model));
            push_xml_escaped_text(&mut details, &line);
            let row_bytes = prefix.len() + details.len() + 1;
            if row_bytes > remaining {
                break;
            }
            remaining -= row_bytes;
            rows.push((prefix, &model.description, details));
        }
        let omitted = rows.len() < model_count;
        if omitted {
            while remaining < OMITTED_NOTICE.len() {
                let Some((prefix, _, details)) = rows.pop() else {
                    break;
                };
                remaining += prefix.len() + details.len() + 1;
            }
            remaining -= OMITTED_NOTICE.len();
        }
        for (prefix, description, details) in rows {
            catalog.push_str(&prefix);
            let start = catalog.len();
            let mut ellipsis_end = start;
            if remaining > 1 {
                for (index, ch) in description.chars().enumerate() {
                    push_xml_escaped_text(&mut catalog, ch.encode_utf8(&mut [0; 4]));
                    if index == MAX_DESCRIPTION_CHARS || catalog.len() - start >= remaining {
                        catalog.truncate(ellipsis_end);
                        if ellipsis_end > start {
                            catalog.push('…');
                        }
                        break;
                    }
                    if catalog.len() - start + '…'.len_utf8() < remaining {
                        ellipsis_end = catalog.len();
                    }
                }
            }
            if catalog.len() > start {
                catalog.push(' ');
                remaining -= catalog.len() - start;
            }
            catalog.push_str(&details);
            catalog.push('\n');
        }
        if omitted {
            catalog.push_str(OMITTED_NOTICE);
        }
        Self {
            catalog,
            namespace: None,
        }
    }

    pub(crate) fn with_namespace(mut self, namespace: Option<ContentItemNamespace>) -> Self {
        self.namespace = namespace;
        self
    }
}

impl ContextualUserFragment for ModelCatalogState {
    fn content_metadata(&self) -> ContentItemMetadata {
        ContentItemMetadata::tool(self.namespace.clone())
    }

    fn content_kind(&self) -> ContentItemKind {
        ContentItemKind("multi_agent.model_catalog".to_string())
    }

    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("<model_catalog>", "</model_catalog>")
    }

    fn body(&self) -> ContentItem {
        input_text(if self.catalog.is_empty() {
            "\nThe previous spawn_agent model catalog no longer applies.\n".to_string()
        } else {
            format!("\n{}", self.catalog)
        })
    }
}

impl WorldStateSection for ModelCatalogState {
    const ID: &'static str = "model_catalog";
    type Snapshot = WorldStateHash;

    fn matches_legacy_fragment(role: &str, text: &str) -> bool {
        role == "developer" && Self::matches_text(text)
    }

    fn has_retained_fragment_matcher() -> bool {
        true
    }

    fn matches_retained_fragment(role: &str, text: &str) -> bool {
        Self::matches_legacy_fragment(role, text)
    }

    fn render_diff(
        &self,
        previous: PreviousSectionState<'_, Self::Snapshot>,
    ) -> SectionTransition<Self::Snapshot> {
        let current = WorldStateHash::from_fragment(self);
        if matches!(previous, PreviousSectionState::Known(previous) if previous == &current)
            || self.catalog.is_empty() && matches!(previous, PreviousSectionState::Absent)
        {
            return (Some(current), Vec::new());
        }
        (
            Some(current),
            vec![WorldStateUpdate::fragment(self.clone())],
        )
    }
}
