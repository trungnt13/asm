//! Selects advertised model choices without restricting runtime model access.

use super::child_config::MAX_SPAWN_AGENT_MODEL_OVERRIDES;
use super::child_config::model_supports_multi_agent_backend;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::protocol::MultiAgentVersion;

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
