//! Client-owned controls for TUI titles; manual rename suggestions are unchanged.

use codex_protocol::openai_models::ReasoningEffort;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use std::num::NonZeroUsize;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AutoRenameFirstTrigger {
    #[default]
    FirstUserMessage,
    FirstCompletedTurn,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AutoRenameContext {
    #[default]
    UserMessage,
    RecentConversation,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AutoRenameUpdatePolicy {
    #[default]
    Once,
    UntilManual,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AutoRenameConfig {
    pub enabled: bool,
    #[serde(deserialize_with = "deserialize_model")]
    pub model: Option<String>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub first_trigger: AutoRenameFirstTrigger,
    pub context: AutoRenameContext,
    pub update_policy: AutoRenameUpdatePolicy,
    pub auto_update_interval_turns: NonZeroUsize,
    /// Source-text budget from 128 to 8192 bytes, separate from additional instructions.
    /// Omission retains the original source-text budget.
    #[serde(deserialize_with = "deserialize_context_bytes")]
    #[schemars(range(min = 128, max = 8192))]
    pub max_context_bytes: Option<usize>,
    #[serde(deserialize_with = "deserialize_message_limit")]
    #[schemars(range(min = 1, max = 32))]
    pub recent_message_limit: Option<usize>,
    #[serde(deserialize_with = "deserialize_title_chars")]
    #[schemars(range(min = 1, max = 128))]
    pub max_title_chars: Option<usize>,
    /// Additional naming guidance, limited to 512 UTF-8 bytes.
    #[serde(deserialize_with = "deserialize_instructions")]
    pub instructions: Option<String>,
}

impl Default for AutoRenameConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            model: None,
            reasoning_effort: None,
            first_trigger: AutoRenameFirstTrigger::default(),
            context: AutoRenameContext::default(),
            update_policy: AutoRenameUpdatePolicy::default(),
            auto_update_interval_turns: NonZeroUsize::new(/*n*/ 5)
                .expect("positive update interval"),
            max_context_bytes: None,
            recent_message_limit: None,
            max_title_chars: None,
            instructions: None,
        }
    }
}

fn deserialize_limit<'de, D>(deserializer: D, maximum: usize) -> Result<Option<usize>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<usize>::deserialize(deserializer)?;
    if value.is_some_and(|value| value == 0 || value > maximum) {
        return Err(serde::de::Error::custom(format!(
            "auto_rename limit must be between 1 and {maximum}"
        )));
    }
    Ok(value)
}

fn deserialize_context_bytes<'de, D>(deserializer: D) -> Result<Option<usize>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = deserialize_limit(deserializer, /*maximum*/ 8192)?;
    if value.is_some_and(|value| value < 128) {
        return Err(serde::de::Error::custom(
            "auto_rename.max_context_bytes must be between 128 and 8192",
        ));
    }
    Ok(value)
}

fn deserialize_message_limit<'de, D>(deserializer: D) -> Result<Option<usize>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_limit(deserializer, /*maximum*/ 32)
}

fn deserialize_title_chars<'de, D>(deserializer: D) -> Result<Option<usize>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_limit(deserializer, /*maximum*/ 128)
}

fn deserialize_instructions<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    if value.as_ref().is_some_and(|value| value.len() > 512) {
        return Err(serde::de::Error::custom(
            "auto_rename.instructions must not exceed 512 UTF-8 bytes",
        ));
    }
    Ok(value)
}

fn deserialize_model<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    if value.as_ref().is_some_and(|value| value.trim().is_empty()) {
        return Err(serde::de::Error::custom(
            "auto_rename.model must not be blank",
        ));
    }
    Ok(value.map(|value| value.trim().to_string()))
}
