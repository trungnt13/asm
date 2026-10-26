use super::Config;
use std::io;

impl Config {
    pub(crate) fn validate_external_subagent_model(&self) -> io::Result<()> {
        let provider = self.model_provider_id.as_str();
        if codex_models_manager::provider_has_model_bindings(provider)
            && !self.model.as_deref().is_some_and(|model| {
                crate::claude_runtime::is_native_model(model)
                    || codex_models_manager::provider_for_model(model) == Some(provider)
            })
        {
            let model = self.model.as_deref().unwrap_or("<default>");
            return Err(io::Error::other(format!(
                "Model `{model}` has no binding to provider `{provider}`; subagents using this catalog-bound provider require a bound model or native runtime. Use a separately configured provider ID for unbound models."
            )));
        }
        Ok(())
    }

    pub fn apply_external_model_provider(&mut self) -> io::Result<()> {
        if let Some(model) = self
            .model
            .as_deref()
            .and_then(codex_models_manager::canonical_model)
        {
            self.model = Some(model.to_owned());
        }
        let Some(provider_id) = self
            .model
            .as_deref()
            .and_then(codex_models_manager::provider_for_model)
        else {
            return Ok(());
        };
        if self
            .config_layer_stack
            .required_model_provider()
            .is_some_and(|required| required != provider_id)
        {
            return Err(io::Error::other(
                "Selected model conflicts with the required model provider",
            ));
        }
        let provider = self
            .model_providers
            .get(provider_id)
            .ok_or_else(|| {
                io::Error::other(format!("Model provider `{provider_id}` is not configured"))
            })?
            .clone();
        let projected = toml::toml! {
            model = (self.model.as_deref().unwrap_or_default())
            model_provider = provider_id
        };
        let effective = self.config_layer_stack.effective_config();
        if projected
            .iter()
            .any(|(key, value)| effective.get(key) != Some(value))
        {
            let mut layers: Vec<_> = self
                .config_layer_stack
                .all_layers_low_to_high()
                .cloned()
                .collect();
            let layer = codex_config::ConfigLayerEntry::new(
                codex_config::ConfigLayerSource::SessionFlags,
                toml::Value::Table(projected),
            );
            let index = layers.partition_point(|existing| existing.name <= layer.name);
            layers.insert(index, layer);
            self.config_layer_stack = codex_config::ConfigLayerStack::new(
                layers,
                self.config_layer_stack.requirements().clone(),
                self.config_layer_stack.requirements_toml().clone(),
            )?
            .with_cloud_config_binding(self.config_layer_stack.cloud_config_binding().cloned())
            .with_mcp_ema_cancellation(self.config_layer_stack.mcp_ema_cancellation().clone())
            .with_applied_config_server(self.config_layer_stack.applied_config_server().cloned())
            .with_user_and_project_exec_policy_rules_ignored(
                self.config_layer_stack
                    .ignore_user_and_project_exec_policy_rules(),
            );
        }
        self.model_provider_id = provider_id.to_owned();
        self.model_provider = provider;
        Ok(())
    }
}
