use crate::common::ResponsesApiRequest;
use crate::error::ApiError;
use codex_protocol::ToolName;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::plaintext_agent_message_content;
use serde_json::Value;
use serde_json::json;
use std::borrow::Cow;
use std::collections::HashMap;
use std::hash::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;

pub(crate) enum ChatTool {
    Function(ToolName),
    Freeform(ToolName),
}

pub(crate) fn build_chat_request(
    request: &ResponsesApiRequest,
) -> Result<(Value, HashMap<String, ChatTool>), ApiError> {
    if request.access_programs.is_some() || !request.request_country.is_omitted() {
        return Err(invalid_request(
            "managed Responses controls cannot be sent to Chat Completions",
        ));
    }
    if request
        .text
        .as_ref()
        .is_some_and(|text| text.format.is_some())
    {
        return Err(invalid_request(
            "structured output is not supported by the Chat Completions adapter",
        ));
    }

    let (tools, tool_names) = project_tools(request)?;

    let mut body = json!({
        "model": request.model,
        "stream": true,
        "stream_options": {"include_usage": true},
    });
    body["messages"] = Value::Array(project_messages(request, &tool_names)?);
    if let Some(tier) = &request.service_tier {
        body["service_tier"] = json!(tier);
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
        body["tool_choice"] = json!(request.tool_choice);
        body["parallel_tool_calls"] = json!(request.parallel_tool_calls);
    }
    if let Some(effort) = request
        .reasoning
        .as_ref()
        .and_then(|reasoning| reasoning.effort.as_ref())
    {
        body["reasoning_effort"] = json!(effort.as_str());
    }
    Ok((body, tool_names))
}

pub(super) fn project_tools(
    request: &ResponsesApiRequest,
) -> Result<(Vec<Value>, HashMap<String, ChatTool>), ApiError> {
    let mut tool_names = HashMap::new();
    let mut tools = Vec::new();
    if let Some(declarations) = &request.tools {
        let declarations: Vec<Value> = serde_json::from_str(declarations.as_raw_value().get())
            .map_err(|_| invalid_request("invalid Chat Completions tool declarations"))?;
        for mut tool in declarations {
            match tool["type"].as_str() {
                Some("function" | "custom") => {
                    tools.push(project_tool(
                        tool,
                        /*namespace*/ None,
                        &mut tool_names,
                    )?);
                }
                Some("namespace") => {
                    let namespace = tool["name"]
                        .as_str()
                        .ok_or_else(|| invalid_request("missing tool namespace name"))?
                        .to_owned();
                    let Value::Array(members) = tool["tools"].take() else {
                        return Err(invalid_request("missing tool namespace members"));
                    };
                    for member in members {
                        tools.push(project_tool(member, Some(&namespace), &mut tool_names)?);
                    }
                }
                _ => {
                    return Err(invalid_request(
                        "Chat Completions supports local tools only",
                    ));
                }
            }
        }
    }

    Ok((tools, tool_names))
}

fn project_messages(
    request: &ResponsesApiRequest,
    tool_names: &HashMap<String, ChatTool>,
) -> Result<Vec<Value>, ApiError> {
    let mut messages = Vec::with_capacity(request.input.len());
    let mut assistant = AssistantMessage::default();
    let mut pending_calls = HashMap::new();
    let mut freeform_calls = HashMap::new();
    let mut deferred_context = Vec::new();
    for item in &request.input {
        let context = match item.as_ref() {
            ResponseItem::Message { role, content, .. } => {
                let mut text = String::new();
                for (index, part) in content.iter().enumerate() {
                    match part {
                        ContentItem::InputText { text: part }
                        | ContentItem::OutputText { text: part, .. } => {
                            if index > 0 {
                                text.push('\n');
                            }
                            text.push_str(part);
                        }
                        ContentItem::InputImage { .. } | ContentItem::InputAudio { .. } => {
                            return Err(invalid_request(
                                "Chat Completions accepts text input only",
                            ));
                        }
                    }
                }
                let role = match role.as_str() {
                    "assistant" if pending_calls.is_empty() => {
                        assistant.text.push_str(&text);
                        continue;
                    }
                    "assistant" | "system" | "user" => role,
                    "developer" => "system",
                    _ => return Err(invalid_request("unsupported Chat Completions message role")),
                };
                json!({"role": role, "content": text})
            }
            ResponseItem::FunctionCall {
                name,
                namespace,
                arguments,
                call_id,
                ..
            }
            | ResponseItem::CustomToolCall {
                name,
                namespace,
                input: arguments,
                call_id,
                ..
            } => {
                let freeform = matches!(item.as_ref(), ResponseItem::CustomToolCall { .. });
                if (assistant.calls.is_empty() && !pending_calls.is_empty())
                    || pending_calls.insert(call_id.as_str(), freeform).is_some()
                {
                    return Err(invalid_request(
                        "invalid Chat Completions tool call sequence",
                    ));
                }
                let wire_name = wire_tool_name(namespace.as_deref(), name);
                if let Some(tool) = tool_names.get(&wire_name)
                    && !match tool {
                        ChatTool::Function(tool) => {
                            !freeform && tool.namespace == *namespace && tool.name == *name
                        }
                        ChatTool::Freeform(tool) => {
                            freeform && tool.namespace == *namespace && tool.name == *name
                        }
                    }
                {
                    return Err(invalid_request(
                        "Chat Completions tool name conflicts with history",
                    ));
                }
                let arguments = if freeform {
                    freeform_calls.insert(call_id.as_str(), name.as_str());
                    Cow::Owned(json!({"input": arguments}).to_string())
                } else {
                    Cow::Borrowed(arguments.as_str())
                };
                assistant.calls.push(json!({
                    "id": call_id,
                    "type": "function",
                    "function": {
                        "name": wire_name,
                        "arguments": arguments,
                    },
                }));
                continue;
            }
            ResponseItem::CustomToolCallOutput {
                call_id,
                name: Some(name),
                output,
                ..
            } => {
                if freeform_calls.get(call_id.as_str()) != Some(&name.as_str()) {
                    return Err(invalid_request(
                        "unmatched Chat Completions tool notification",
                    ));
                }
                // Code Mode can notify more than once per call. Chat accepts just one tool result.
                let content = tool_result_text(&output.body)?;
                json!({"role": "user", "content": format!("Additional output from {name} ({call_id}):\n{content}")})
            }
            ResponseItem::FunctionCallOutput {
                call_id: Some(call_id),
                output,
                ..
            }
            | ResponseItem::CustomToolCallOutput {
                call_id, output, ..
            } => {
                let freeform = matches!(item.as_ref(), ResponseItem::CustomToolCallOutput { .. });
                if pending_calls.remove(call_id.as_str()) != Some(freeform) {
                    return Err(invalid_request("unmatched Chat Completions tool result"));
                }
                assistant.flush(&mut messages);
                messages.push(json!({
                    "role": "tool", "tool_call_id": call_id, "content": tool_result_text(&output.body)?,
                }));
                if pending_calls.is_empty() {
                    messages.append(&mut deferred_context);
                }
                continue;
            }
            ResponseItem::FunctionCallOutput {
                call_id: None,
                name,
                namespace,
                output,
                ..
            } => {
                let name = name
                    .as_deref()
                    .filter(|name| !name.is_empty())
                    .ok_or_else(|| {
                        invalid_request("standalone Chat Completions tool output is missing a name")
                    })?;
                let source = namespace.as_ref().map_or_else(
                    || name.to_owned(),
                    |namespace| format!("{namespace}.{name}"),
                );
                let content = tool_result_text(&output.body)?;
                json!({"role": "user", "content": format!("Tool output from {source}:\n{content}")})
            }
            // The Chat wire does not replay Responses reasoning or historical reasoning controls.
            ResponseItem::Reasoning { .. } | ResponseItem::ConfigurationUpdate { .. } => continue,
            ResponseItem::AgentMessage {
                author,
                recipient,
                content,
                ..
            } => {
                let text = plaintext_agent_message_content(content).ok_or_else(|| {
                    invalid_request("Chat Completions requires a plaintext agent message")
                })?;
                json!({
                    "role": "user",
                    "content": format!("Agent message from {author} to {recipient}:\n{text}"),
                })
            }
            ResponseItem::AdditionalTools { .. }
            | ResponseItem::LocalShellCall { .. }
            | ResponseItem::ToolSearchCall { .. }
            | ResponseItem::ToolSearchOutput { .. }
            | ResponseItem::WebSearchCall { .. }
            | ResponseItem::ImageGenerationCall { .. }
            | ResponseItem::Compaction { .. }
            | ResponseItem::CompactionTrigger { .. }
            | ResponseItem::ContextCompaction { .. }
            | ResponseItem::Other => {
                return Err(invalid_request("unsupported Chat Completions history item"));
            }
        };
        if pending_calls.is_empty() {
            assistant.flush(&mut messages);
            messages.push(context);
        } else {
            // Hooks can record context before the tool result is available; Chat requires adjacency.
            deferred_context.push(context);
        }
    }
    if !pending_calls.is_empty() {
        return Err(invalid_request("missing Chat Completions tool results"));
    }
    assistant.flush(&mut messages);
    Ok(messages)
}

pub(super) fn tool_result_text(output: &FunctionCallOutputBody) -> Result<Cow<'_, str>, ApiError> {
    match output {
        FunctionCallOutputBody::Text(text) => Ok(Cow::Borrowed(text)),
        FunctionCallOutputBody::ContentItems(parts) => {
            let mut text = String::new();
            for (index, part) in parts.iter().enumerate() {
                let FunctionCallOutputContentItem::InputText { text: part } = part else {
                    return Err(invalid_request(
                        "Chat Completions accepts plain text tool results only",
                    ));
                };
                if index > 0 {
                    text.push('\n');
                }
                text.push_str(part);
            }
            Ok(Cow::Owned(text))
        }
    }
}

#[derive(Default)]
struct AssistantMessage {
    text: String,
    calls: Vec<Value>,
}

impl AssistantMessage {
    fn flush(&mut self, messages: &mut Vec<Value>) {
        if self.text.is_empty() && self.calls.is_empty() {
            return;
        }
        let mut message = json!({"role": "assistant", "content": null});
        if !self.text.is_empty() {
            message["content"] = Value::String(std::mem::take(&mut self.text));
        }
        if !self.calls.is_empty() {
            message["tool_calls"] = Value::Array(std::mem::take(&mut self.calls));
        }
        messages.push(message);
    }
}

fn project_tool(
    mut tool: Value,
    namespace: Option<&str>,
    tool_names: &mut HashMap<String, ChatTool>,
) -> Result<Value, ApiError> {
    let Value::String(name) = tool["name"].take() else {
        return Err(invalid_request("missing Chat Completions function name"));
    };
    let freeform = tool["type"] == "custom";
    if (!freeform && tool["type"] != "function") || tool["defer_loading"] == true {
        return Err(invalid_request(
            "Chat Completions supports local tools only",
        ));
    }
    let wire_name = wire_tool_name(namespace, &name);
    let identity = ToolName::new(namespace.map(str::to_owned), name);
    let tool_kind = if freeform {
        ChatTool::Freeform(identity)
    } else {
        ChatTool::Function(identity)
    };
    if tool_names.insert(wire_name.clone(), tool_kind).is_some() {
        return Err(invalid_request(format!(
            "duplicate Chat Completions function name: {wire_name}"
        )));
    }
    let mut function = json!({"type": "function", "function": {"name": wire_name}});
    function["function"]["description"] = if freeform {
        let description = tool["description"].as_str().unwrap_or_default();
        json!(format!(
            "Pass the raw tool input as the `input` string, without Markdown fences.\n\n{description}"
        ))
    } else {
        tool["description"].take()
    };
    function["function"]["parameters"] = if freeform {
        json!({
            "type": "object",
            "properties": {"input": {"type": "string"}},
            "required": ["input"],
            "additionalProperties": false,
        })
    } else {
        tool["parameters"].take()
    };
    if tool["strict"] == true {
        function["function"]["strict"] = Value::Bool(true);
    }
    Ok(function)
}

pub(super) fn wire_tool_name(namespace: Option<&str>, name: &str) -> String {
    let wire = namespace.map_or_else(
        || name.to_owned(),
        |namespace| format!("{namespace}__{name}"),
    );
    let valid = |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-');
    let unambiguous = !name.contains("__")
        && namespace.is_none_or(|namespace| {
            !namespace.is_empty()
                && !namespace.contains("__")
                && !namespace.ends_with('_')
                && !name.starts_with('_')
                && !name.is_empty()
        });
    if unambiguous && !wire.is_empty() && wire.len() <= 64 && wire.bytes().all(valid) {
        return wire;
    }
    // Reserve triple underscores for hashed aliases, including when replaying historical calls.
    let prefix: String = wire
        .bytes()
        .take(45)
        .map(|byte| if valid(byte) { char::from(byte) } else { '_' })
        .collect();
    let mut hasher = DefaultHasher::new();
    (namespace, name).hash(&mut hasher);
    let suffix = hasher.finish();
    format!("{prefix}___{suffix:016x}")
}

fn invalid_request(message: impl Into<String>) -> ApiError {
    ApiError::InvalidRequest {
        message: message.into(),
    }
}
