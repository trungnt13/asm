use codex_protocol::config_types::WebSearchMode;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use toml::Table;
use toml::Value;

use super::Config;

impl Config {
    pub(crate) fn subagent_collaboration_disabled(&self, session_source: &SessionSource) -> bool {
        !self.subagent_tools.collaboration
            && matches!(
                session_source,
                SessionSource::SubAgent(SubAgentSource::ThreadSpawn { .. })
            )
    }

    pub(crate) fn apply_subagent_tools(&mut self) -> anyhow::Result<()> {
        let mut session_overrides = Table::new();
        let disabled_features = self.subagent_tools.disabled_features().collect::<Vec<_>>();
        if !disabled_features.is_empty() {
            let mut features = self.features.get().clone();
            for &feature in &disabled_features {
                features.disable(feature);
            }
            self.features.set(features)?;
            let mut feature_overrides = Table::new();
            for feature in disabled_features {
                anyhow::ensure!(
                    !self.features.enabled(feature),
                    "cannot disable {} under the current feature requirements",
                    feature.key()
                );
                feature_overrides.insert(feature.key().to_owned(), Value::Boolean(false));
            }
            session_overrides.insert("features".to_owned(), Value::Table(feature_overrides));
        }

        self.subagent_mcp_disabled = self.subagent_tools.disable_mcp;
        if !self.subagent_tools.web_search {
            self.web_search_mode.set(WebSearchMode::Disabled)?;
            session_overrides.insert(
                "web_search".to_owned(),
                Value::String("disabled".to_owned()),
            );
        }

        // Inventory RPCs rebuild a fresh config from the retained child layers.
        if !session_overrides.is_empty() {
            self.config_layer_stack = self
                .config_layer_stack
                .clone()
                .with_session_config(Value::Table(session_overrides))?;
        }
        Ok(())
    }
}
