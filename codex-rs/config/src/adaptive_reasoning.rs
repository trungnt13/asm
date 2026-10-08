//! Opt-in controls for choosing ordinary reasoning budgets without changing execution modes.

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AdaptiveReasoningConfig {
    pub enabled: bool,
    pub decision_model: String,
    pub decision_model_api_env: String,
    pub min_effort: ReasoningEffort,
    pub max_effort: ReasoningEffort,
    #[schemars(range(min = 1, max = 5000))]
    pub decision_timeout_ms: u64,
    #[schemars(range(min = 128, max = 8192))]
    pub max_context_bytes: usize,
    pub update_on: Vec<AdaptiveReasoningTrigger>,
    pub additional_instructions: String,
}

impl Default for AdaptiveReasoningConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            decision_model: "gpt-6-luna".to_string(),
            decision_model_api_env: "OPENAI_API_KEY".to_string(),
            min_effort: ReasoningEffort::Low,
            max_effort: ReasoningEffort::Max,
            decision_timeout_ms: 200,
            max_context_bytes: 8192,
            update_on: vec![
                AdaptiveReasoningTrigger::TurnStart,
                AdaptiveReasoningTrigger::ToolResult,
            ],
            additional_instructions: String::new(),
        }
    }
}

impl AdaptiveReasoningConfig {
    pub fn validate(&self) -> Result<(), String> {
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
        if self.decision_model.trim().is_empty() {
            return Err("adaptive_reasoning.decision_model must not be blank".to_string());
        }
        let mut name = self.decision_model_api_env.bytes();
        if !name
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
            || !name.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err("adaptive_reasoning.decision_model_api_env must be an ASCII environment variable identifier".to_string());
        }
        if !(1..=5000).contains(&self.decision_timeout_ms) {
            return Err(
                "adaptive_reasoning.decision_timeout_ms must be between 1 and 5000".to_string(),
            );
        }
        if !(128..=8192).contains(&self.max_context_bytes) {
            return Err(
                "adaptive_reasoning.max_context_bytes must be between 128 and 8192".to_string(),
            );
        }
        if self.additional_instructions.len() > 4096 {
            return Err(
                "adaptive_reasoning.additional_instructions must not exceed 4096 UTF-8 bytes"
                    .to_string(),
            );
        }
        Ok(())
    }
}
