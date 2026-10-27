//! Opt-in controls for choosing ordinary reasoning budgets without changing execution modes.

use std::collections::BTreeMap;

use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ReasoningEffort;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum System1Trigger {
    TurnStart,
    ToolResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum System1Backend {
    Openai,
    Typesafe,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct System1Profile {
    #[serde(rename = "protocol", alias = "backend")]
    pub backend: System1Backend,
    #[serde(default)]
    pub endpoint: Option<String>,
    pub model: String,
    /// Environment variable name, never the credential itself.
    pub api_key_env: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct System1Config {
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
    pub profiles: BTreeMap<String, System1Profile>,
    pub decision_model: Option<String>,
    pub decision_model_api_env: Option<String>,
    pub min_effort: ReasoningEffort,
    pub max_effort: ReasoningEffort,
    #[schemars(range(min = 1, max = 5000))]
    pub decision_timeout_ms: u64,
    #[schemars(range(min = 128, max = 32768))]
    pub max_context_bytes: usize,
    pub update_on: Vec<System1Trigger>,
    pub low_effort_description: String,
    pub medium_effort_description: String,
    pub high_effort_description: String,
    pub xhigh_effort_description: String,
    pub max_effort_description: String,
    pub rubric_instructions: Option<String>,
}

impl Default for System1Config {
    fn default() -> Self {
        Self {
            enabled: false,
            decision_every_n_turns: 1,
            decision_every_n_steps: 1,
            profile: None,
            profiles: BTreeMap::new(),
            decision_model: None,
            decision_model_api_env: None,
            min_effort: ReasoningEffort::Low,
            max_effort: ReasoningEffort::Max,
            decision_timeout_ms: 200,
            max_context_bytes: 8192,
            update_on: vec![System1Trigger::TurnStart, System1Trigger::ToolResult],
            low_effort_description: "Straightforward coding, focused searches, and bounded analysis.".to_string(),
            medium_effort_description: "Ordinary substantive work, including moderately complex coding.".to_string(),
            high_effort_description: "Difficult debugging, multiple interacting constraints, conflicting evidence, or substantial uncertainty. Use max only for exceptionally difficult problems.".to_string(),
            xhigh_effort_description: "Extensive reasoning for exceptionally difficult problems, deep investigations, and demanding long-horizon planning.".to_string(),
            max_effort_description: "Exceptionally difficult problems requiring the maximum supported reasoning budget.".to_string(),
            rubric_instructions: None,
        }
    }
}

impl System1Config {
    /// Returns the choice description, or `None` for unsupported effort levels.
    pub fn effort_description(&self, effort: &ReasoningEffort) -> Option<&str> {
        match effort {
            ReasoningEffort::None => {
                Some("No reasoning budget for direct execution without deliberation.")
            }
            ReasoningEffort::Minimal => {
                Some("Minimal reasoning for trivial tasks and simple direct responses.")
            }
            ReasoningEffort::Low => Some(&self.low_effort_description),
            ReasoningEffort::Medium => Some(&self.medium_effort_description),
            ReasoningEffort::High => Some(&self.high_effort_description),
            ReasoningEffort::XHigh => Some(&self.xhigh_effort_description),
            ReasoningEffort::Max => Some(&self.max_effort_description),
            ReasoningEffort::Ultra | ReasoningEffort::Persistent | ReasoningEffort::Custom(_) => {
                None
            }
        }
    }

    /// Calibrates shared difficulty guidance against the model's default effort.
    /// A maximum default uses the preceding supported ordinary effort as its reference.
    /// Configured bounds do not shift this reference.
    pub fn calibrated_effort_description(
        &self,
        effort: &ReasoningEffort,
        model: &ModelInfo,
    ) -> Option<&str> {
        let ordinary = [
            ReasoningEffort::None,
            ReasoningEffort::Minimal,
            ReasoningEffort::Low,
            ReasoningEffort::Medium,
            ReasoningEffort::High,
            ReasoningEffort::XHigh,
            ReasoningEffort::Max,
        ];
        let supported: Vec<_> = ordinary
            .iter()
            .filter(|effort| {
                model
                    .supported_reasoning_levels
                    .iter()
                    .any(|preset| &preset.effort == *effort)
            })
            .collect();
        let Some(index) = supported
            .iter()
            .position(|effort| Some(*effort) == model.default_reasoning_level.as_ref())
            .filter(|_| !model.used_fallback_model_metadata)
        else {
            return self.effort_description(effort);
        };
        let reference = if model.default_reasoning_level == Some(ReasoningEffort::Max) {
            index.saturating_sub(/*rhs*/ 1)
        } else {
            index
        };
        let position = supported
            .iter()
            .position(|supported| *supported == effort)?;
        match effort {
            ReasoningEffort::None | ReasoningEffort::Minimal => self.effort_description(effort),
            ReasoningEffort::Max => Some(&self.max_effort_description),
            ReasoningEffort::Low
            | ReasoningEffort::Medium
            | ReasoningEffort::High
            | ReasoningEffort::XHigh => Some(if position + 1 < reference {
                "Mechanical execution, settled decisions, and simple responses."
            } else if position < reference {
                &self.low_effort_description
            } else if position == reference {
                &self.medium_effort_description
            } else if position == reference + 1 {
                &self.high_effort_description
            } else {
                &self.xhigh_effort_description
            }),
            ReasoningEffort::Ultra | ReasoningEffort::Persistent | ReasoningEffort::Custom(_) => {
                None
            }
        }
    }

    pub fn resolved_profile(&self) -> Result<System1Profile, String> {
        let profile = if let Some(name) = &self.profile {
            if self.decision_model.is_some() || self.decision_model_api_env.is_some() {
                return Err("system1.profile cannot be combined with decision_model or decision_model_api_env".to_string());
            }
            self.profiles
                .get(name)
                .cloned()
                .ok_or_else(|| format!("system1.profile references unknown profile {name:?}"))?
        } else {
            System1Profile {
                backend: System1Backend::Openai,
                endpoint: None,
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
            return Err("system1 decision_every_n_turns and decision_every_n_steps must be positive integers".to_string());
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
            return Err("system1 effort bounds must be ordinary levels: none, minimal, low, medium, high, xhigh, max".to_string());
        };
        if min > max {
            return Err("system1.min_effort must not exceed max_effort".to_string());
        }
        self.resolved_profile()?;
        for (name, profile) in &self.profiles {
            if name.trim().is_empty() {
                return Err("system1 profile names must not be blank".to_string());
            }
            profile.validate()?;
        }
        if !(1..=5000).contains(&self.decision_timeout_ms) {
            return Err("system1.decision_timeout_ms must be between 1 and 5000".to_string());
        }
        if !(128..=32768).contains(&self.max_context_bytes) {
            return Err("system1.max_context_bytes must be between 128 and 32768".to_string());
        }
        for (name, description) in [
            ("low_effort_description", &self.low_effort_description),
            ("medium_effort_description", &self.medium_effort_description),
            ("high_effort_description", &self.high_effort_description),
            ("xhigh_effort_description", &self.xhigh_effort_description),
            ("max_effort_description", &self.max_effort_description),
        ] {
            if description.trim().is_empty() {
                return Err(format!("system1.{name} must not be blank"));
            }
            if description.len() > 4096 {
                return Err(format!("system1.{name} must not exceed 4096 UTF-8 bytes"));
            }
        }
        if let Some(rubric) = &self.rubric_instructions {
            if rubric.trim().is_empty() {
                return Err("system1.rubric_instructions must not be blank".to_string());
            }
            if rubric.len() > 4096 {
                return Err(
                    "system1.rubric_instructions must not exceed 4096 UTF-8 bytes".to_string(),
                );
            }
        }
        Ok(())
    }
}

impl System1Profile {
    fn validate(&self) -> Result<(), String> {
        if self
            .endpoint
            .as_ref()
            .is_some_and(|endpoint| endpoint.trim().is_empty())
        {
            return Err("system1 profile endpoint must not be blank".to_string());
        }
        if self.model.trim().is_empty() {
            return Err("system1 decision model must not be blank".to_string());
        }
        let mut name = self.api_key_env.bytes();
        if !name
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
            || !name.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err("system1 API key environment variable must be an ASCII environment variable identifier".to_string());
        }
        Ok(())
    }
}
