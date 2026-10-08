//! Executor selection and authority checks for external V2 children.

use crate::config::Config;
use codex_history::InitialHistory;
use codex_history::RolloutItem;
use codex_protocol::ExternalAgentDescriptor;
use codex_protocol::ThreadId;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::protocol::MultiAgentVersion;
use codex_protocol::protocol::SandboxPolicy;
use codex_protocol::protocol::SessionSource;

pub(crate) fn configured_backend(
    config: &Config,
    role: Option<&str>,
) -> Result<Option<String>, String> {
    let selected = role.unwrap_or(crate::agent::role::DEFAULT_ROLE_NAME);
    let backend = config
        .agent_roles
        .get(selected)
        .and_then(|role| role.execution_backend.clone());
    if role.is_none() && backend.is_some() {
        return Err("external execution requires an explicit named agent_type".to_owned());
    }
    Ok(backend)
}

/// Rejects policies the external executor cannot enforce.
pub(crate) fn validate_config(config: &Config) -> Result<(), String> {
    let Some(external) = &config.external_agent else {
        return Ok(());
    };
    if external.version != 1
        || external.backend_id.trim().is_empty()
        || external.model.trim().is_empty()
    {
        return Err("unsupported or incomplete external agent descriptor".to_owned());
    }
    if config.model_providers.contains_key(&external.backend_id) {
        return Err("external backend identities must not alias native model providers".to_owned());
    }
    let requirements = config.config_layer_stack.requirements();
    if requirements.exec_policy.is_some()
        || requirements.managed_hooks.is_some()
        || requirements
            .allow_managed_hooks_only
            .as_ref()
            .is_some_and(|required| required.value)
        || requirements.model_provider.is_some()
        || requirements.allowed_chatgpt_workspaces.is_some()
        || requirements
            .feature_requirements
            .as_ref()
            .is_some_and(|required| {
                required.entries.iter().any(|(key, enabled)| {
                    *enabled
                        && matches!(
                            codex_features::feature_for_key(key),
                            Some(
                                codex_features::Feature::GuardianApproval
                                    | codex_features::Feature::GuardianV2
                            )
                        )
                })
            })
    {
        return Err("external agents cannot enforce this thread's mandatory managed execution or provider restrictions".to_owned());
    }
    if !matches!(
        config.legacy_sandbox_policy(),
        SandboxPolicy::DangerFullAccess
    ) || config.permissions.network.is_some()
        || config.approvals_reviewer == ApprovalsReviewer::AutoReview
        || config
            .config_layer_stack
            .requirements()
            .auto_review_required_for_model(&external.model)
    {
        return Err("external agents require explicit Full Access without managed network or automatic review restrictions".to_owned());
    }
    if config.rollout_budget.is_some()
        || config.token_budget.is_some()
        || config
            .features
            .enabled(codex_features::Feature::TokenBudget)
    {
        return Err("external agents do not support shared token budgets".to_owned());
    }
    Ok(())
}

/// Restores immutable executor identity before any native startup service can run.
pub(crate) fn restore(
    config: &mut Config,
    history: &InitialHistory,
    source: &SessionSource,
) -> Result<(), String> {
    let stored = history
        .get_rollout_items()
        .iter()
        .find_map(|item| match item {
            RolloutItem::SessionMeta(line) => line.meta.external_agent.clone(),
            _ => None,
        });
    if let Some(stored) = stored {
        if !matches!(history, InitialHistory::Resumed(_)) {
            return Err(
                "external conversations cannot be forked as native model history".to_owned(),
            );
        }
        if config
            .external_agent
            .as_ref()
            .is_some_and(|current| current != &stored)
        {
            return Err("external executor descriptor disagrees with persisted history".to_owned());
        }
        let backend = configured_backend(config, source.get_agent_role().as_deref())?;
        if backend.as_deref() != Some(stored.backend_id.as_str()) {
            return Err(
                "external agent backend is missing or differs from its recorded role".to_owned(),
            );
        }
        config.external_agent = Some(stored);
        config.external_agent_launch_mode = codex_extension_api::ExternalAgentLaunchMode::Resume;
    }
    if let Some(external) = &config.external_agent {
        if source.parent_thread_id().is_none() || source.get_agent_path().is_none() {
            return Err("external executors are supported only for owned V2 children".to_owned());
        }
        if history
            .get_multi_agent_version()
            .is_some_and(|version| version != MultiAgentVersion::V2)
        {
            return Err("external executor history is not multi-agent V2".to_owned());
        }
        config.model = Some(external.model.clone());
        config.model_provider_id = external.backend_id.clone();
        config.model_provider.name = format!("External runtime ({})", external.backend_id);
        config.model_reasoning_effort = None;
        config.service_tier = None;
        validate_config(config)?;
    }
    Ok(())
}

pub(crate) fn select_new(config: &mut Config, backend_id: String) -> Result<(), String> {
    let model = config
        .model
        .clone()
        .filter(|model| !model.trim().is_empty())
        .ok_or("external roles require an explicit model")?;
    config.model_provider_id = backend_id.clone();
    config.model_provider.name = format!("External runtime ({backend_id})");
    config.external_agent = Some(ExternalAgentDescriptor {
        version: 1,
        backend_id,
        runtime_session_id: ThreadId::new(),
        model,
    });
    config.external_agent_launch_mode = codex_extension_api::ExternalAgentLaunchMode::New;
    config.model_reasoning_effort = None;
    config.service_tier = None;
    validate_config(config)
}

/// Projects external identity without claiming native API features or context capacity.
pub(crate) fn model_info(model: &str) -> codex_protocol::openai_models::ModelInfo {
    let mut info = codex_models_manager::model_info::model_info_from_slug(model);
    info.supported_in_api = false;
    info.context_window = None;
    info.max_context_window = None;
    info.auto_compact_token_limit = None;
    info.model_messages = None;
    info.include_skills_usage_instructions = false;
    info.include_plugin_usage_instructions = false;
    info.include_apps_usage_instructions = false;
    info.supports_reasoning_summary_parameter = false;
    info.used_fallback_model_metadata = false;
    info.input_modalities = vec![codex_protocol::openai_models::InputModality::Text];
    info
}
