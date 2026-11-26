use super::chat::ChatTool;
use super::chat::project_tools;
use super::chat::tool_result_text;
use super::chat::wire_tool_name;
use crate::common::ResponsesApiRequest;
use crate::error::ApiError;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::plaintext_agent_message_content;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;

pub(crate) const THOUGHT_PREFIX: &str = "asm:gemini-interactions:1:";

pub fn is_gemini_thought(item: &ResponseItem) -> bool {
    matches!(
        item,
        ResponseItem::Reasoning { encrypted_content: Some(opaque), .. }
            if opaque.starts_with(THOUGHT_PREFIX)
    )
}

pub(crate) fn build_gemini_request(
    request: &ResponsesApiRequest,
) -> Result<(Value, HashMap<String, ChatTool>), ApiError> {
    if request.access_programs.is_some()
        || !request.request_country.is_omitted()
        || request
            .text
            .as_ref()
            .is_some_and(|text| text.format.is_some())
    {
        return Err(invalid_request(
            "unsupported Gemini Interactions request controls",
        ));
    }
    let (tools, tool_names) = project_tools(request)?;
    let tools = tools
        .into_iter()
        .map(|mut declaration| {
            let mut function = declaration["function"].take();
            function["type"] = json!("function");
            if let Some(properties) = function.as_object_mut() {
                properties.remove("strict");
            }
            function
        })
        .collect::<Vec<_>>();
    let (instructions, input) = project_steps(request)?;
    let mut body = json!({"model": request.model});
    body["input"] = Value::Array(input);
    body["store"] = Value::Bool(false);
    body["stream"] = Value::Bool(true);
    if !instructions.is_empty() {
        body["system_instruction"] = Value::String(instructions);
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    if let Some(effort) = request
        .reasoning
        .as_ref()
        .and_then(|reasoning| reasoning.effort.as_ref())
    {
        let effort = match effort.as_str() {
            level @ ("low" | "medium" | "high" | "xhigh") => level,
            _ => return Err(invalid_request("unsupported Gemini thinking level")),
        };
        body["generation_config"] = json!({"thinking_level": effort});
    }
    Ok((body, tool_names))
}

fn project_steps(request: &ResponsesApiRequest) -> Result<(String, Vec<Value>), ApiError> {
    let mut instructions = String::new();
    let mut steps = Vec::with_capacity(request.input.len());
    let mut pending = HashMap::new();
    let mut deferred = Vec::new();
    for item in &request.input {
        let context = match item.as_ref() {
            ResponseItem::Message { role, content, .. } => {
                let parts = content
                    .iter()
                    .map(|part| match part {
                        ContentItem::InputText { text } | ContentItem::OutputText { text, .. } => {
                            Ok(text)
                        }
                        ContentItem::InputImage { .. } | ContentItem::InputAudio { .. } => {
                            Err(invalid_request(
                                "Gemini Interactions currently accepts text input only",
                            ))
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                match role.as_str() {
                    "developer" | "system" if steps.is_empty() => {
                        for text in parts {
                            if !instructions.is_empty() {
                                instructions.push_str("\n\n");
                            }
                            instructions.push_str(text);
                        }
                        continue;
                    }
                    "developer" | "system" => text_user_input(format!(
                        "ASM instructions:\n{}",
                        parts
                            .into_iter()
                            .map(String::as_str)
                            .collect::<Vec<_>>()
                            .join("\n"),
                    )),
                    role @ ("assistant" | "user") => {
                        let kind = if role == "assistant" {
                            "model_output"
                        } else {
                            "user_input"
                        };
                        let content = parts
                            .into_iter()
                            .map(|text| json!({"type":"text", "text":text}))
                            .collect::<Vec<_>>();
                        let mut step = json!({"type": kind});
                        step["content"] = Value::Array(content);
                        if role == "assistant" {
                            steps.push(step);
                            continue;
                        }
                        step
                    }
                    _ => {
                        return Err(invalid_request(
                            "unsupported Gemini Interactions message role",
                        ));
                    }
                }
            }
            ResponseItem::Reasoning {
                encrypted_content: Some(opaque),
                ..
            } if let Some(step) = opaque.strip_prefix(THOUGHT_PREFIX) => {
                let step: Value = serde_json::from_str(step)
                    .map_err(|_| invalid_request("invalid saved Gemini thought step"))?;
                if step["type"] != "thought" {
                    return Err(invalid_request("invalid saved Gemini thought step type"));
                }
                steps.push(step);
                continue;
            }
            ResponseItem::Reasoning { .. } | ResponseItem::ConfigurationUpdate { .. } => continue,
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
                let name = wire_tool_name(namespace.as_deref(), name);
                let arguments = if matches!(item.as_ref(), ResponseItem::CustomToolCall { .. }) {
                    json!({"input": arguments})
                } else {
                    serde_json::from_str(arguments)
                        .map_err(|_| invalid_request("invalid saved Gemini tool arguments"))?
                };
                if !arguments.is_object()
                    || call_id.is_empty()
                    || pending.insert(call_id.as_str(), name.clone()).is_some()
                {
                    return Err(invalid_request("invalid saved Gemini tool call"));
                }
                let mut step = json!({"type":"function_call", "id": call_id, "name": name});
                step["arguments"] = arguments;
                steps.push(step);
                continue;
            }
            ResponseItem::FunctionCallOutput {
                call_id: Some(call_id),
                output,
                ..
            }
            | ResponseItem::CustomToolCallOutput {
                call_id,
                name: None,
                output,
                ..
            } => {
                let name = pending
                    .remove(call_id.as_str())
                    .ok_or_else(|| invalid_request("unmatched Gemini tool result"))?;
                let mut result = json!({
                    "type": "function_result",
                    "call_id": call_id,
                    "name": name,
                    "result": [{"type":"text", "text":tool_result_text(&output.body)?}],
                });
                if output.success == Some(false) {
                    result["is_error"] = json!(true);
                }
                steps.push(result);
                if pending.is_empty() {
                    steps.append(&mut deferred);
                }
                continue;
            }
            ResponseItem::CustomToolCallOutput {
                call_id,
                name: Some(name),
                output,
                ..
            } => text_user_input(format!(
                "Additional output from {name} ({call_id}):\n{}",
                tool_result_text(&output.body)?,
            )),
            ResponseItem::FunctionCallOutput {
                call_id: None,
                name: Some(name),
                output,
                ..
            } => text_user_input(format!(
                "Additional output from {name}:\n{}",
                tool_result_text(&output.body)?,
            )),
            ResponseItem::AgentMessage {
                author,
                recipient,
                content,
                ..
            } => {
                let text = plaintext_agent_message_content(content).ok_or_else(|| {
                    invalid_request("Gemini Interactions requires a plaintext agent message")
                })?;
                text_user_input(format!(
                    "Agent message from {author} to {recipient}:\n{text}"
                ))
            }
            _ => {
                return Err(invalid_request(
                    "unsupported Gemini Interactions history item",
                ));
            }
        };
        if pending.is_empty() {
            steps.push(context);
        } else {
            deferred.push(context);
        }
    }
    if !pending.is_empty() {
        return Err(invalid_request("missing Gemini tool results"));
    }
    Ok((instructions, steps))
}

fn text_user_input(text: String) -> Value {
    json!({"type": "user_input", "content": [{"type": "text", "text": text}]})
}

fn invalid_request(message: &str) -> ApiError {
    ApiError::InvalidRequest {
        message: message.to_owned(),
    }
}
