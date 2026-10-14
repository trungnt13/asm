//! Opt-in controls for choosing ordinary reasoning budgets without changing execution modes.

use std::collections::BTreeMap;

use codex_protocol::openai_models::ReasoningEffort;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AdaptiveReasoningTrigger {
    TurnStart,
    ToolResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AdaptiveReasoningBackend {
    Openai,
    Typesafe,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AdaptiveReasoningProfile {
    pub backend: AdaptiveReasoningBackend,
    pub model: String,
    /// Environment variable name, never the credential itself.
    pub api_key_env: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AdaptiveReasoningConfig {
    pub enabled: bool,
    /// Adapt turns 1, 1 + N, 1 + 2N, ... independently in each agent.
    #[schemars(range(min = 1))]
    pub decision_every_n_turns: u64,
    /// Wait N model steps after each accepted decision in selected turns.
    /// The first eligible step is due; failures do not restart the spacing.
    /// Each new turn starts without a previous accepted decision.
    #[schemars(range(min = 1))]
    pub decision_every_n_steps: u64,
    pub profile: Option<String>,
    pub profiles: BTreeMap<String, AdaptiveReasoningProfile>,
    pub decision_model: Option<String>,
    pub decision_model_api_env: Option<String>,
    pub min_effort: ReasoningEffort,
    pub max_effort: ReasoningEffort,
    #[schemars(range(min = 1, max = 5000))]
    pub decision_timeout_ms: u64,
    #[schemars(range(min = 128, max = 32768))]
    pub max_context_bytes: usize,
    pub update_on: Vec<AdaptiveReasoningTrigger>,
    pub rubric_instructions: Option<String>,
}

impl Default for AdaptiveReasoningConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            decision_every_n_turns: 1,
            decision_every_n_steps: 2,
            profile: None,
            profiles: BTreeMap::new(),
            decision_model: None,
            decision_model_api_env: None,
            min_effort: ReasoningEffort::Low,
            max_effort: ReasoningEffort::Max,
            decision_timeout_ms: 200,
            max_context_bytes: 8192,
            update_on: vec![
                AdaptiveReasoningTrigger::TurnStart,
                AdaptiveReasoningTrigger::ToolResult,
            ],
            rubric_instructions: None,
        }
    }
}

impl AdaptiveReasoningConfig {
    pub fn resolved_profile(&self) -> Result<AdaptiveReasoningProfile, String> {
        let profile = if let Some(name) = &self.profile {
            if self.decision_model.is_some() || self.decision_model_api_env.is_some() {
                return Err("adaptive_reasoning.profile cannot be combined with decision_model or decision_model_api_env".to_string());
            }
            self.profiles.get(name).cloned().ok_or_else(|| {
                format!("adaptive_reasoning.profile references unknown profile {name:?}")
            })?
        } else {
            AdaptiveReasoningProfile {
                backend: AdaptiveReasoningBackend::Openai,
                model: self
                    .decision_model
                    .clone()
                    .unwrap_or_else(|| "gpt-6-luna".to_string()),
                api_key_env: self
                    .decision_model_api_env
                    .clone()
                    .unwrap_or_else(|| "DECISION_API_KEY".to_string()),
            }
        };
        profile.validate()?;
        Ok(profile)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.decision_every_n_turns == 0 || self.decision_every_n_steps == 0 {
            return Err("adaptive_reasoning decision_every_n_turns and decision_every_n_steps must be positive integers".to_string());
        }
        let ordinary_efforts = [
            ReasoningEffort::None,
            ReasoningEffort::Minimal,
            ReasoningEffort::Low,
            ReasoningEffort::Medium,
            ReasoningEffort::High,
            ReasoningEffort::XHigh,
            ReasoningEffort::Max,
        ];
        let min = ordinary_efforts
            .iter()
            .position(|effort| effort == &self.min_effort);
        let max = ordinary_efforts
            .iter()
            .position(|effort| effort == &self.max_effort);
        let (Some(min), Some(max)) = (min, max) else {
            return Err("adaptive_reasoning effort bounds must be ordinary levels: none, minimal, low, medium, high, xhigh, max".to_string());
        };
        if min > max {
            return Err("adaptive_reasoning.min_effort must not exceed max_effort".to_string());
        }
        self.resolved_profile()?;
        for (name, profile) in &self.profiles {
            if name.trim().is_empty() {
                return Err("adaptive_reasoning profile names must not be blank".to_string());
            }
            profile.validate()?;
        }
        if !(1..=5000).contains(&self.decision_timeout_ms) {
            return Err(
                "adaptive_reasoning.decision_timeout_ms must be between 1 and 5000".to_string(),
            );
        }
        if !(128..=32768).contains(&self.max_context_bytes) {
            return Err(
                "adaptive_reasoning.max_context_bytes must be between 128 and 32768".to_string(),
            );
        }
        if let Some(rubric) = &self.rubric_instructions {
            if rubric.trim().is_empty() {
                return Err("adaptive_reasoning.rubric_instructions must not be blank".to_string());
            }
            if rubric.len() > 4096 {
                return Err(
                    "adaptive_reasoning.rubric_instructions must not exceed 4096 UTF-8 bytes"
                        .to_string(),
                );
            }
        }
        Ok(())
    }
}

impl AdaptiveReasoningProfile {
    fn validate(&self) -> Result<(), String> {
        if self.model.trim().is_empty() {
            return Err("adaptive_reasoning decision model must not be blank".to_string());
        }
        let mut name = self.api_key_env.bytes();
        if !name
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
            || !name.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err("adaptive_reasoning API key environment variable must be an ASCII environment variable identifier".to_string());
        }
        Ok(())
    }
}
