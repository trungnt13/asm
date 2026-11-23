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
use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;

pub(crate) fn build_chat_request(
    request: &ResponsesApiRequest,
) -> Result<(Value, HashMap<String, ToolName>), ApiError> {
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

    let mut tool_names = HashMap::new();
    let mut tools = Vec::new();
    if let Some(declarations) = &request.tools {
        let declarations: Vec<Value> = serde_json::from_str(declarations.as_raw_value().get())
            .map_err(|_| invalid_request("invalid Chat Completions tool declarations"))?;
        for mut tool in declarations {
            match tool["type"].as_str() {
                Some("function") => {
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
                        "Chat Completions supports direct functions only",
                    ));
                }
            }
        }
    }

    let mut body = json!({
        "model": request.model,
        "stream": true,
        "stream_options": {"include_usage": true},
    });
    body["messages"] = Value::Array(project_messages(request, &tool_names)?);
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

fn project_messages(
    request: &ResponsesApiRequest,
    tool_names: &HashMap<String, ToolName>,
) -> Result<Vec<Value>, ApiError> {
    let mut messages = Vec::with_capacity(request.input.len());
    let mut assistant = AssistantMessage::default();
    let mut pending_calls = HashSet::new();
    for item in &request.input {
        match item.as_ref() {
            ResponseItem::Message { role, content, .. } => {
                if !pending_calls.is_empty() {
                    return Err(invalid_request("missing Chat Completions tool results"));
                }
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
                match role.as_str() {
                    "assistant" => assistant.text.push_str(&text),
                    "system" | "developer" | "user" => {
                        assistant.flush(&mut messages);
                        let mut message = json!({
                            "role": if role == "developer" { "system" } else { role.as_str() },
                        });
                        message["content"] = Value::String(text);
                        messages.push(message);
                    }
                    _ => return Err(invalid_request("unsupported Chat Completions message role")),
                }
            }
            ResponseItem::FunctionCall {
                name,
                namespace,
                arguments,
                call_id,
                ..
            } => {
                if (assistant.calls.is_empty() && !pending_calls.is_empty())
                    || !pending_calls.insert(call_id.as_str())
                {
                    return Err(invalid_request(
                        "invalid Chat Completions tool call sequence",
                    ));
                }
                let wire_name = wire_tool_name(namespace.as_deref(), name);
                if let Some(tool) = tool_names.get(&wire_name)
                    && (tool.namespace != *namespace || tool.name != *name)
                {
                    return Err(invalid_request(
                        "Chat Completions tool name conflicts with history",
                    ));
                }
                assistant.calls.push(json!({
                    "id": call_id,
                    "type": "function",
                    "function": {
                        "name": wire_name,
                        "arguments": arguments,
                    },
                }));
            }
            ResponseItem::FunctionCallOutput {
                call_id, output, ..
            } => {
                assistant.flush(&mut messages);
                let call_id = call_id
                    .as_deref()
                    .filter(|call_id| pending_calls.remove(call_id))
                    .ok_or_else(|| invalid_request("unmatched Chat Completions tool result"))?;
                let content = match &output.body {
                    FunctionCallOutputBody::Text(text) => text.clone(),
                    FunctionCallOutputBody::ContentItems(parts) => parts
                        .iter()
                        .map(|part| match part {
                            FunctionCallOutputContentItem::InputText { text } => Ok(text.as_str()),
                            FunctionCallOutputContentItem::InputImage { .. }
                            | FunctionCallOutputContentItem::InputAudio { .. }
                            | FunctionCallOutputContentItem::EncryptedContent { .. } => {
                                Err(invalid_request(
                                    "Chat Completions accepts plain text tool results only",
                                ))
                            }
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .join("\n"),
                };
                let mut message = json!({"role": "tool", "tool_call_id": call_id});
                message["content"] = Value::String(content);
                messages.push(message);
            }
            // The Chat wire does not replay Responses reasoning or historical reasoning controls.
            ResponseItem::Reasoning { .. } | ResponseItem::ConfigurationUpdate { .. } => {}
            ResponseItem::AgentMessage {
                author,
                recipient,
                content,
                ..
            } => {
                if !pending_calls.is_empty() {
                    return Err(invalid_request("missing Chat Completions tool results"));
                }
                let text = plaintext_agent_message_content(content).ok_or_else(|| {
                    invalid_request("Chat Completions requires a plaintext agent message")
                })?;
                assistant.flush(&mut messages);
                messages.push(json!({
                    "role": "user",
                    "content": format!("Agent message from {author} to {recipient}:\n{text}"),
                }));
            }
            ResponseItem::AdditionalTools { .. }
            | ResponseItem::LocalShellCall { .. }
            | ResponseItem::ToolSearchCall { .. }
            | ResponseItem::CustomToolCall { .. }
            | ResponseItem::CustomToolCallOutput { .. }
            | ResponseItem::ToolSearchOutput { .. }
            | ResponseItem::WebSearchCall { .. }
            | ResponseItem::ImageGenerationCall { .. }
            | ResponseItem::Compaction { .. }
            | ResponseItem::CompactionTrigger { .. }
            | ResponseItem::ContextCompaction { .. }
            | ResponseItem::Other => {
                return Err(invalid_request("unsupported Chat Completions history item"));
            }
        }
    }
    if !pending_calls.is_empty() {
        return Err(invalid_request("missing Chat Completions tool results"));
    }
    assistant.flush(&mut messages);
    Ok(messages)
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
    tool_names: &mut HashMap<String, ToolName>,
) -> Result<Value, ApiError> {
    if tool["type"] != "function" || tool["defer_loading"] == true {
        return Err(invalid_request(
            "Chat Completions supports direct functions only",
        ));
    }
    let Value::String(name) = tool["name"].take() else {
        return Err(invalid_request("missing Chat Completions function name"));
    };
    let wire_name = wire_tool_name(namespace, &name);
    let identity = ToolName::new(namespace.map(str::to_owned), name);
    if tool_names.insert(wire_name.clone(), identity).is_some() {
        return Err(invalid_request(format!(
            "duplicate Chat Completions function name: {wire_name}"
        )));
    }
    let mut function = json!({"type": "function", "function": {"name": wire_name}});
    function["function"]["description"] = tool["description"].take();
    function["function"]["parameters"] = tool["parameters"].take();
    if tool["strict"] == true {
        function["function"]["strict"] = Value::Bool(true);
    }
    Ok(function)
}

fn wire_tool_name(namespace: Option<&str>, name: &str) -> String {
    let wire = namespace.map_or_else(
        || name.to_owned(),
        |namespace| format!("{namespace}__{name}"),
    );
    let valid = |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-');
    if !wire.is_empty() && wire.len() <= 64 && wire.bytes().all(valid) {
        return wire;
    }
    // Recreate the same alias for historical calls even if the tool is no longer advertised.
    let prefix: String = wire
        .bytes()
        .take(47)
        .map(|byte| if valid(byte) { char::from(byte) } else { '_' })
        .collect();
    let mut hasher = DefaultHasher::new();
    (namespace, name).hash(&mut hasher);
    let suffix = hasher.finish();
    format!("{prefix}_{suffix:016x}")
}

fn invalid_request(message: impl Into<String>) -> ApiError {
    ApiError::InvalidRequest {
        message: message.into(),
    }
}
