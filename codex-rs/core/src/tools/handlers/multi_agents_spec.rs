use codex_protocol::openai_models::ModelPreset;
use codex_protocol::protocol::MultiAgentVersion;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;

const SPAWN_AGENT_INHERITED_MODEL_GUIDANCE: &str = "Spawned agents inherit your current model by default. Omit `model` to use that preferred default; set `model` only when an explicit override is needed.";
const SPAWN_AGENT_INHERITED_MODEL_GUIDANCE_V2: &str = "Spawned agents inherit your current model by default. Do not set the `model` field unless the user explicitly asks for a different model.";
const SPAWN_AGENT_MODEL_CATALOG_GUIDANCE: &str =
    "Pick model overrides from the latest <model_catalog> listing.";
const SPAWN_AGENT_MODEL_OVERRIDE_DESCRIPTION: &str = "Model override for the new agent, taking precedence over the role model. Role effort constraints still apply. Omit unless an explicit override is needed.";
const MAX_REASONING_EFFORT_CHARS_IN_SPAWN_AGENT_DESCRIPTION: usize = 64;

#[derive(Debug, Clone)]
pub struct SpawnAgentToolOptions {
    pub available_models: Vec<ModelPreset>,
    pub subagent_model_list: Option<Vec<String>>,
    pub external_model_aliases: Vec<String>,
    pub agent_type_description: String,
    pub expose_agent_type: bool,
    pub hide_agent_type_model_reasoning: bool,
    pub expose_spawn_agent_model_overrides: bool,
    pub multi_agent_version: MultiAgentVersion,
    pub include_model_catalog_in_spawn_agent_list: bool,
    pub model_catalog_in_context: bool,
    pub usage_hint_text: Option<String>,
}

impl Default for SpawnAgentToolOptions {
    fn default() -> Self {
        Self {
            available_models: Vec::new(),
            subagent_model_list: None,
            external_model_aliases: Vec::new(),
            agent_type_description: String::new(),
            expose_agent_type: true,
            hide_agent_type_model_reasoning: false,
            expose_spawn_agent_model_overrides: false,
            multi_agent_version: MultiAgentVersion::Disabled,
            include_model_catalog_in_spawn_agent_list: true,
            model_catalog_in_context: false,
            usage_hint_text: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaitAgentTimeoutOptions {
    pub default_timeout_ms: i64,
    pub min_timeout_ms: i64,
    pub max_timeout_ms: i64,
}

impl Default for WaitAgentTimeoutOptions {
    fn default() -> Self {
        Self {
            default_timeout_ms: super::multi_agents_common::DEFAULT_WAIT_TIMEOUT_MS,
            min_timeout_ms: super::multi_agents_common::MIN_WAIT_TIMEOUT_MS,
            max_timeout_ms: super::multi_agents_common::MAX_WAIT_TIMEOUT_MS,
        }
    }
}

pub fn create_spawn_agent_tool_v2(
    options: SpawnAgentToolOptions,
    description_override: Option<&str>,
) -> ToolSpec {
    let available_models_description = (options.include_model_catalog_in_spawn_agent_list
        && !options.model_catalog_in_context
        && options.expose_spawn_agent_model_overrides)
        .then(|| {
            spawn_agent_models_description(
                &options.available_models,
                options.multi_agent_version,
                options.subagent_model_list.as_deref(),
            )
        });
    let inherited_model_guidance = if options.model_catalog_in_context {
        options
            .expose_spawn_agent_model_overrides
            .then_some(SPAWN_AGENT_INHERITED_MODEL_GUIDANCE_V2)
    } else {
        (options.expose_spawn_agent_model_overrides && !options.hide_agent_type_model_reasoning)
            .then_some(SPAWN_AGENT_INHERITED_MODEL_GUIDANCE)
    };
    let mut properties = spawn_agent_common_properties_v2(&options.agent_type_description);
    if !options.expose_agent_type {
        properties.remove("agent_type");
    }
    if !options.expose_spawn_agent_model_overrides {
        properties.remove("model");
        properties.remove("reasoning_effort");
    }
    properties.insert(
        "task_name".to_string(),
        JsonSchema::string(Some(
            "Task name for the new agent. Use lowercase letters, digits, and underscores."
                .to_string(),
        )),
    );

    let mut parameters = JsonSchema::object(
        properties,
        Some(vec!["task_name".to_string(), "message".to_string()]),
        Some(false.into()),
    );
    require_spawn_agent_fork_turns(&mut parameters);

    ToolSpec::Function(ResponsesApiTool {
        name: "spawn_agent".to_string(),
        description: append_external_model_aliases(
            spawn_agent_tool_description_v2(
                available_models_description.as_deref(),
                options.include_model_catalog_in_spawn_agent_list
                    && options.model_catalog_in_context,
                options
                    .expose_spawn_agent_model_overrides
                    .then_some(SPAWN_AGENT_MODEL_CATALOG_GUIDANCE),
                inherited_model_guidance,
                options.usage_hint_text,
                description_override,
            ),
            &options.external_model_aliases,
            "fork_turns=\"none\"",
        ),
        strict: false,
        defer_loading: None,
        parameters,
        output_schema: Some(
            spawn_agent_output_schema_v2(options.hide_agent_type_model_reasoning).into(),
        ),
    })
}

pub fn create_send_message_tool() -> ToolSpec {
    let properties = BTreeMap::from([
        (
            "target".to_string(),
            JsonSchema::string(Some(
                "Relative or canonical task name to message (from spawn_agent).".to_string(),
            )),
        ),
        (
            "message".to_string(),
            JsonSchema::string(Some(
                "Message text to queue on the target agent.".to_string(),
            )),
        ),
    ]);

    ToolSpec::Function(ResponsesApiTool {
        name: "send_message".to_string(),
        description: "Send a message to an existing agent. The message will be delivered promptly. Does not trigger a new turn. Busy Claude Code agents receive it after their current native tool finishes, without stopping work; use interrupt_agent first to redirect urgently. Idle agents remain queued."
            .to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            properties,
            Some(vec!["target".to_string(), "message".to_string()]),
            Some(false.into()),
        ),
        output_schema: None,
    })
}

pub fn create_followup_task_tool() -> ToolSpec {
    let properties = BTreeMap::from([
        (
            "target".to_string(),
            JsonSchema::string(Some(
                "Agent id or canonical task name to send a follow-up task to (from spawn_agent)."
                    .to_string(),
            )),
        ),
        (
            "message".to_string(),
            JsonSchema::string(Some(
                "Message text to send to the target agent.".to_string(),
            )),
        ),
    ]);

    ToolSpec::Function(ResponsesApiTool {
        name: "followup_task".to_string(),
        description: "Send a follow-up task to an existing non-root target agent and trigger a turn if it is idle. If the target is already running, deliver the task promptly at message boundaries while sampling, or after the pending tool call completes."
            .to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(properties, Some(vec!["target".to_string(), "message".to_string()]), Some(false.into())),
        output_schema: None,
    })
}

pub fn create_wait_agent_tool_v2(options: WaitAgentTimeoutOptions) -> ToolSpec {
    ToolSpec::Function(ResponsesApiTool {
        name: "wait_agent".to_string(),
        description: "Wait for a mailbox update from any live agent, including queued messages and final-status notifications. The wait also ends early when new user input is steered into the active turn. Does not return the content; returns either a summary of which agents have updates (if any), an interruption summary for steered input, or a timeout summary if no activity arrives before the deadline."
            .to_string(),
        strict: false,
        defer_loading: None,
        parameters: wait_agent_tool_parameters_v2(options),
        output_schema: Some(wait_output_schema_v2().into()),
    })
}

pub fn create_list_agents_tool() -> ToolSpec {
    let properties = BTreeMap::from([(
        "path_prefix".to_string(),
        JsonSchema::string(Some(
            "Task-path prefix filter without a trailing slash. Omit to list all live agents."
                .to_string(),
        )),
    )]);

    ToolSpec::Function(ResponsesApiTool {
        name: "list_agents".to_string(),
        description:
            "List live agents in the current root thread tree. Optionally filter by task-path prefix."
                .to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(properties, /*required*/ None, Some(false.into())),
        output_schema: Some(list_agents_output_schema().into()),
    })
}

pub fn create_interrupt_agent_tool_v2() -> ToolSpec {
    let properties = BTreeMap::from([(
        "target".to_string(),
        JsonSchema::string(Some(
            "Agent id or canonical task name to interrupt (from spawn_agent).".to_string(),
        )),
    )]);

    ToolSpec::Function(ResponsesApiTool {
        name: "interrupt_agent".to_string(),
        description: "Interrupt an agent's current turn, if any, and return its previous status. The agent remains available for messages and follow-up tasks.".to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(properties, Some(vec!["target".to_string()]), Some(false.into())),
        output_schema: Some(
            agent_previous_status_output_schema(
                "The agent status observed before the interrupt request was handled.",
            )
            .into(),
        ),
    })
}

fn agent_status_output_schema() -> Value {
    json!({
        "oneOf": [
            {
                "type": "string",
                "enum": ["pending_init", "running", "interrupted", "shutdown", "not_found"]
            },
            {
                "type": "object",
                "properties": {
                    "completed": {
                        "type": ["string", "null"]
                    }
                },
                "required": ["completed"],
                "additionalProperties": false
            },
            {
                "type": "object",
                "properties": {
                    "errored": {
                        "type": "string"
                    }
                },
                "required": ["errored"],
                "additionalProperties": false
            }
        ]
    })
}

fn spawn_agent_output_schema_v2(hide_agent_metadata: bool) -> Value {
    if hide_agent_metadata {
        return json!({
            "type": "object",
            "properties": {
                "task_name": {
                    "type": "string",
                    "description": "Canonical task name for the spawned agent."
                }
            },
            "required": ["task_name"],
            "additionalProperties": false
        });
    }

    json!({
        "type": "object",
        "properties": {
            "task_name": {
                "type": "string",
                "description": "Canonical task name for the spawned agent."
            },
            "nickname": {
                "type": ["string", "null"],
                "description": "User-facing nickname for the spawned agent when available."
            }
        },
        "required": ["task_name", "nickname"],
        "additionalProperties": false
    })
}

fn list_agents_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "agents": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "agent_name": {
                            "type": "string",
                            "description": "Canonical task name for the agent when available, otherwise the agent id."
                        },
                        "agent_status": {
                            "description": "Last known status of the agent.",
                            "allOf": [agent_status_output_schema()]
                        }
                    },
                    "required": ["agent_name", "agent_status"],
                    "additionalProperties": false
                },
                "description": "Live agents visible in the current root thread tree."
            }
        },
        "required": ["agents"],
        "additionalProperties": false
    })
}

fn wait_output_schema_v2() -> Value {
    json!({
        "type": "object",
        "properties": {
            "message": {
                "type": "string",
                "description": "Brief wait summary without the agent's final content, including any timeout adjustment."
            },
            "timed_out": {
                "type": "boolean",
                "description": "Whether the wait call returned because no mailbox update arrived before the timeout."
            }
        },
        "required": ["message", "timed_out"],
        "additionalProperties": false
    })
}

fn agent_previous_status_output_schema(previous_status_description: &str) -> Value {
    json!({
        "type": "object",
        "properties": {
            "previous_status": {
                "description": previous_status_description,
                "allOf": [agent_status_output_schema()]
            }
        },
        "required": ["previous_status"],
        "additionalProperties": false
    })
}

fn spawn_agent_common_properties_v2(agent_type_description: &str) -> BTreeMap<String, JsonSchema> {
    BTreeMap::from([
        (
            "message".to_string(),
            JsonSchema::string(Some(
                "Initial plain-text task for the new agent.".to_string(),
            )),
        ),
        (
            "agent_type".to_string(),
            JsonSchema::string(Some(format!(
                "Agent type override for the new agent. Omit unless explicitly asked. The selected role applies regardless of how much parent history is inherited.\n{agent_type_description}"
            ))),
        ),
        (
            "model".to_string(),
            JsonSchema::string(Some(SPAWN_AGENT_MODEL_OVERRIDE_DESCRIPTION.to_string())),
        ),
        (
            "reasoning_effort".to_string(),
            JsonSchema::string(Some(
                "Reasoning effort override for the new agent. Omit to inherit the parent effort."
                    .to_string(),
            )),
        ),
    ])
}

pub(crate) fn require_spawn_agent_fork_turns(parameters: &mut JsonSchema) {
    parameters.properties.get_or_insert_default().insert(
        "fork_turns".to_string(),
        JsonSchema::string(Some(
            "Parent history to inherit. Use `none` for no history or `all` for full history."
                .to_string(),
        )),
    );
    let mut required = parameters.required.take().unwrap_or_default().into_vec();
    if !required.iter().any(|name| name == "fork_turns") {
        required.push("fork_turns".to_string());
    }
    parameters.required = Some(required.into_boxed_slice());
}

fn append_external_model_aliases(
    mut description: String,
    aliases: &[String],
    fork_argument: &str,
) -> String {
    if !aliases.is_empty() {
        description.push_str("\nExternal models (native runtimes or configured providers; runtime/service availability is not verified):\n- ");
        description.push_str(&aliases.join("\n- "));
        description.push_str(&format!("\nUse these exact model aliases. Cross-provider spawns require `{fork_argument}`; native-runtime spawns also require fresh history; include all needed context in `message`."));
    }
    description
}

fn spawn_agent_tool_description_v2(
    available_models_description: Option<&str>,
    model_catalog_in_context: bool,
    model_catalog_guidance: Option<&str>,
    inherited_model_guidance: Option<&str>,
    usage_hint_text: Option<String>,
    description: Option<&str>,
) -> String {
    let model_catalog_guidance = model_catalog_guidance.unwrap_or_default();
    let inherited_model_guidance = inherited_model_guidance.unwrap_or_default();

    let (catalog_prefix, catalog_suffix) = if model_catalog_in_context {
        (String::new(), format!("\n{model_catalog_guidance}"))
    } else {
        let available_models_description = available_models_description.unwrap_or_default();
        (
            format!("        {available_models_description}\n"),
            String::new(),
        )
    };
    let tool_description = if let Some(description) = description {
        format!(
            r#"
{catalog_prefix}        {description}
{inherited_model_guidance}{catalog_suffix}"#
        )
    } else {
        format!(
            r#"
{catalog_prefix}        Spawns an agent to work on the specified task. If your current task is `/root/task1` and you spawn_agent with task_name "task_3" the agent will have canonical task name `/root/task1/task_3`.
You are then able to refer to this agent as `task_3` or `/root/task1/task_3` interchangeably. However an agent `/root/task2/task_3` would only be able to communicate with this agent via its canonical name `/root/task1/task_3`.
Codex-runtime agents will have the same tools as you and the ability to spawn their own subagents.
{inherited_model_guidance}{catalog_suffix}
It will be able to send you and other running agents messages, and its final answer will be provided to you when it finishes.
The new agent's canonical task name will be provided to it along with the message.

Note that passing `fork_turns="none"` will not pass any surrounding context to the spawned subagent, which may cause the agent to lack the context it needs to complete its task, whereas `fork_turns="all"` will provide the subagent with all surrounding context."#
        )
    };

    let tool_description = format!(
        "{tool_description}\nNative runtime models (`claude-code/*`, `cursor-agent/*`) require `fork_turns=\"none\"`. Include all needed context in `message`. They use their native instructions, tools and permissions, not the parent's. Cursor agents cannot call agent messaging tools; their final answer is still returned."
    );

    if let Some(usage_hint_text) = usage_hint_text {
        return format!(
            r#"
        {tool_description}
{usage_hint_text}"#
        );
    }
    tool_description
}

fn spawn_agent_models_description(
    models: &[ModelPreset],
    multi_agent_version: MultiAgentVersion,
    configured: Option<&[String]>,
) -> String {
    let visible_models = match crate::agent::model_catalog::advertised_models(
        models,
        multi_agent_version,
        configured,
    ) {
        Ok(models) => models,
        Err(error) => return error,
    };
    if visible_models.is_empty()
        && multi_agent_version == MultiAgentVersion::V2
        && configured.is_some()
    {
        return "No model overrides are advertised by subagent_model_list.".to_string();
    }
    if visible_models.is_empty() {
        return "No picker-visible model overrides are currently loaded.".to_string();
    }

    let model_descriptions = visible_models
        .into_iter()
        .map(|model| {
            let default_reasoning_effort = &model.default_reasoning_effort;
            let efforts = model
                .supported_reasoning_efforts
                .iter()
                .map(|preset| {
                    let effort = preset.effort.as_str();
                    let effort = match effort
                        .char_indices()
                        .nth(MAX_REASONING_EFFORT_CHARS_IN_SPAWN_AGENT_DESCRIPTION)
                    {
                        Some((index, _)) => &effort[..index],
                        None => effort,
                    };
                    if &preset.effort == default_reasoning_effort {
                        format!("{effort} (default)")
                    } else {
                        effort.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            let reasoning_efforts_suffix = if efforts.is_empty() {
                String::new()
            } else {
                format!(" Reasoning efforts: {efforts}.")
            };
            let model_slug = &model.model;
            let description = &model.description;
            format!("- `{model_slug}`: {description}{reasoning_efforts_suffix}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Available model overrides (optional; inherited parent model is preferred):\n{model_descriptions}"
    )
}

fn wait_agent_tool_parameters_v2(options: WaitAgentTimeoutOptions) -> JsonSchema {
    let properties = BTreeMap::from([(
        "timeout_ms".to_string(),
        JsonSchema::number(Some(format!(
            "Timeout in milliseconds. Defaults to {}, min {}, max {}.",
            options.default_timeout_ms, options.min_timeout_ms, options.max_timeout_ms,
        ))),
    )]);

    let mut parameters = JsonSchema::object(properties, /*required*/ None, Some(false.into()));
    codex_tools::require_wait_reason(&mut parameters);
    parameters
}

#[cfg(test)]
#[path = "multi_agents_spec_tests.rs"]
mod tests;
