//! Selects advertised model choices without restricting runtime model access.

use super::child_config::MAX_SPAWN_AGENT_MODEL_OVERRIDES;
use super::child_config::model_supports_multi_agent_backend;
use crate::config::Config;
use codex_model_provider_info::GEMINI_PROVIDER_ID;
use codex_model_provider_info::WireApi;
use codex_models_manager::ModelsManagerConfig;
use codex_models_manager::model_info::with_config_overrides;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::NativeRuntime;
use codex_protocol::protocol::MultiAgentVersion;
use std::collections::HashMap;

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

pub(crate) fn advertised_context_windows(
    models: &[ModelPreset],
    version: MultiAgentVersion,
    config: &Config,
    active_models: &[ModelInfo],
) -> HashMap<String, String> {
    let Ok(models) = advertised_models(
        models,
        version,
        config.multi_agent_v2.subagent_model_list.as_deref(),
    ) else {
        return HashMap::new();
    };
    models
        .into_iter()
        .map(|model| {
            (
                model.model.clone(),
                context_window_label(config, version, active_models, &model.model),
            )
        })
        .collect()
}

pub(crate) fn context_window_label(
    config: &Config,
    version: MultiAgentVersion,
    active_models: &[ModelInfo],
    id: &str,
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
            let info = if let Some((provider_id, bundled)) =
                codex_models_manager::bundled_provider_model(model)
            {
                config
                    .model_providers
                    .get(provider_id)
                    .and_then(|provider| {
                        let unsupported = provider.is_amazon_bedrock()
                            || provider.is_gcp()
                            || (provider_id == GEMINI_PROVIDER_ID
                                && provider.wire_api != WireApi::GeminiInteractions)
                            || config
                                .config_layer_stack
                                .required_model_provider()
                                .is_some_and(|required| required != provider_id);
                        if unsupported {
                            None
                        } else if config.model_catalog.is_some() {
                            active_models.iter().find(|info| info.slug == model)
                        } else if provider.wire_api == WireApi::GeminiInteractions {
                            (provider_id == GEMINI_PROVIDER_ID).then_some(bundled)
                        } else {
                            provider.model_catalog_url.is_none().then_some(bundled)
                        }
                    })
            } else if codex_models_manager::provider_has_model_bindings(&config.model_provider_id) {
                None
            } else {
                active_models.iter().find(|info| info.slug == model)
            };
            let model_context_window = (version == MultiAgentVersion::V2)
                .then(|| config.subagent_model_context_windows.get(model).copied())
                .flatten()
                .or_else(|| config.inherited_subagent_context_window());
            info.and_then(|info| {
                with_config_overrides(
                    info.clone(),
                    &ModelsManagerConfig {
                        model_context_window,
                        ..Default::default()
                    },
                )
                .usable_context_window()
            })
            .filter(|tokens| *tokens > 0)
            .map_or_else(
                || "Context: unknown.".to_string(),
                |tokens| format!("Context: {tokens}."),
            )
        }
    }
}
