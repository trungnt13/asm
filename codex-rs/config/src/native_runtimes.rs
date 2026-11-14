use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct NativeRuntimesConfig {
    pub claude_code: ClaudeCodeConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ClaudeCodeConfig {
    /// Additional native tool exclusions; ASM's mandatory restrictions remain active.
    pub disallowed_tools: Vec<String>,
    /// Native skill visibility overrides, applied only to ASM-launched Claude processes.
    pub skill_overrides: BTreeMap<String, SkillVisibility>,
    /// Controls queued Desktop Page context without changing native history.
    pub page_context: PageContextPolicy,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PageContextPolicy {
    #[default]
    Inherit,
    WhenPresent,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SkillVisibility {
    On,
    NameOnly,
    UserInvocableOnly,
    Off,
}
