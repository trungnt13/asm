use codex_protocol::ThreadId;
use codex_protocol::openai_models::ReasoningEffort;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::num::NonZeroUsize;

/// Stores trusted defaults for protected review sessions, not ordinary role overrides.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ReviewerSubagentConfig {
    pub model: Option<String>,
    pub model_reasoning_effort: ReasoningEffort,
    pub developer_instructions: Option<String>,
    /// Omission selects the caller's actual root, without granting cross-tree access.
    pub root_session_id: Option<ThreadId>,
    /// Limits total explorer creations per review, not concurrent ordinary agents.
    pub max_explorers: usize,
    pub max_concurrent_reviews: NonZeroUsize,
    pub system1_min_effort: ReasoningEffort,
    pub system1_max_effort: ReasoningEffort,
}

impl Default for ReviewerSubagentConfig {
    fn default() -> Self {
        Self {
            model: None,
            model_reasoning_effort: ReasoningEffort::High,
            developer_instructions: None,
            root_session_id: None,
            max_explorers: 2,
            max_concurrent_reviews: NonZeroUsize::MIN,
            system1_min_effort: ReasoningEffort::High,
            system1_max_effort: ReasoningEffort::XHigh,
        }
    }
}

impl ReviewerSubagentConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self
            .model
            .as_ref()
            .is_some_and(|model| model.trim().is_empty())
        {
            return Err("reviewer_subagent.model must not be blank".to_string());
        }
        crate::System1Config {
            min_effort: self.system1_min_effort.clone(),
            max_effort: self.system1_max_effort.clone(),
            ..Default::default()
        }
        .validate()
        .map_err(|error| format!("reviewer_subagent: {error}"))
    }
}
