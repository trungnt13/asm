//! Selects advertised model choices without restricting runtime model access.

use super::child_config::MAX_SPAWN_AGENT_MODEL_OVERRIDES;
use super::child_config::model_supports_multi_agent_backend;
use crate::config::Config;
use codex_model_provider_info::GEMINI_PROVIDER_ID;
use codex_model_provider_info::WireApi;
use codex_models_manager::manager::ModelsManager;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::NativeRuntime;
use codex_protocol::protocol::MultiAgentVersion;
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) fn advertised_models<'a>(
    models: &'a [ModelPreset],
    version: MultiAgentVersion,
    configured: Option<&[String]>,
) -> Result<Vec<&'a ModelPreset>, String> {
    if version == MultiAgentVersion::V2
        && let Some(configured) = configured
    {
        let mut selected = Vec::new();
        for id in configured {
            let model = models
                .iter()
                .find(|model| model.model == *id && model_supports_multi_agent_backend(model, version))
                .ok_or_else(|| {
                    let id = id.chars().take(128).collect::<String>();
                    format!(
                        "Invalid subagent_model_list: model `{id}` is not loaded or does not support spawning."
                    )
                })?;
            if !selected.contains(&model) {
                selected.push(model);
            }
        }
        selected.truncate(MAX_SPAWN_AGENT_MODEL_OVERRIDES);
        return Ok(selected);
    }
    Ok(models
        .iter()
        .filter(|model| model.show_in_picker)
        .filter(|model| model_supports_multi_agent_backend(model, version))
        .take(MAX_SPAWN_AGENT_MODEL_OVERRIDES)
        .collect())
}

pub(crate) const CONTEXT_WINDOW_SCOPE: &str = "Context windows in tokens, before role overrides.";

pub(crate) fn turn_model_catalog(
    manager: &dyn ModelsManager,
    active_models: Vec<ModelInfo>,
    config: &Config,
    version: MultiAgentVersion,
) -> (Vec<ModelPreset>, Arc<HashMap<String, String>>) {
    if version != MultiAgentVersion::V2
        || !config.include_model_catalog_in_spawn_agent_list
        || !config.multi_agent_v2.expose_spawn_agent_model_overrides
        || config.multi_agent_v2.disable_native_tools
    {
        return (
            manager.build_available_models(active_models),
            Arc::default(),
        );
    }

    let mut active_windows = HashMap::with_capacity(active_models.len());
    for info in &active_models {
        active_windows.entry(info.slug.clone()).or_insert_with(|| {
            info.usable_context_window_with_override(configured_window(config, &info.slug))
        });
    }
    let presets = manager.build_available_models(active_models);
    let windows = advertised_models(
        &presets,
        version,
        config.multi_agent_v2.subagent_model_list.as_deref(),
    )
    .unwrap_or_default()
    .into_iter()
    .map(|model| {
        (
            model.model.clone(),
            resolved_context_window_label(config, &model.model, &active_windows),
        )
    })
    .collect();
    (presets, Arc::new(windows))
}

fn configured_window(config: &Config, model: &str) -> Option<i64> {
    config
        .subagent_model_context_windows
        .get(model)
        .copied()
        .or_else(|| config.inherited_subagent_context_window())
}

pub(crate) fn context_window_label<'a>(windows: &'a HashMap<String, String>, id: &str) -> &'a str {
    windows.get(id).map_or("Context: unknown.", String::as_str)
}

fn resolved_context_window_label(
    config: &Config,
    id: &str,
    active_windows: &HashMap<String, Option<i64>>,
) -> String {
    let model = codex_models_manager::canonical_model(id).unwrap_or(id);
    match NativeRuntime::for_model(model) {
        Some((NativeRuntime::ClaudeCode, _)) => config
            .subagent_model_context_windows
            .get(model)
            .map_or_else(
                || "Context: runtime-managed.".to_string(),
                |tokens| format!("Claude auto-compaction setting: {tokens}."),
            ),
        Some((NativeRuntime::CursorAgent, _)) => "Context: runtime-managed.".to_string(),
        None => {
            let active_window = active_windows.get(model).copied().flatten();
            let tokens = if let Some((provider_id, bundled)) =
                codex_models_manager::bundled_provider_model(model)
            {
                config
                    .model_providers
                    .get(provider_id)
                    .and_then(|provider| {
                        let unsupported = (provider_id == GEMINI_PROVIDER_ID
                            && provider.wire_api != WireApi::GeminiInteractions)
                            || config
                                .config_layer_stack
                                .required_model_provider()
                                .is_some_and(|required| required != provider_id);
                        if unsupported {
                            None
                        } else if config.model_catalog.is_some() {
                            active_window
                        } else if (provider.wire_api == WireApi::GeminiInteractions
                            && provider_id == GEMINI_PROVIDER_ID)
                            || (provider.wire_api != WireApi::GeminiInteractions
                                && provider.model_catalog_url.is_none())
                        {
                            bundled.usable_context_window_with_override(configured_window(
                                config, model,
                            ))
                        } else {
                            None
                        }
                    })
            } else if codex_models_manager::provider_has_model_bindings(&config.model_provider_id) {
                None
            } else {
                active_window
            };
            tokens.filter(|tokens| *tokens > 0).map_or_else(
                || "Context: unknown.".to_string(),
                |tokens| format!("Context: {tokens}."),
            )
        }
    }
}
