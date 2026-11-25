use codex_api::ApiError;
use codex_api::ResponseEvent;
use codex_code_mode::PUBLIC_TOOL_NAME;
use codex_model_provider_info::WireApi;
use codex_protocol::DEFAULT_FUNCTION_NAMESPACE;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ModelInfo;
use codex_tools::FreeformTool;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use serde::Deserialize;
use serde::Serialize;
use serde_json::value::RawValue;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::Arc;

// Match the catalog-bound model, not the configured hostname: it can use a forwarding gateway.
const DEEPSEEK_FLASH: &str = "accounts/fireworks/models/deepseek-v4p1-flash-1017";
const DEEPSEEK_FLASH_ULTRAFAST: &str =
    "accounts/fireworks/routers/deepseek-v4p1-flash-1017-ultrafast";

pub(super) fn route(
    model: &str,
    wire_api: WireApi,
    service_tier: Option<String>,
) -> Result<(String, Option<String>)> {
    match (model, wire_api, service_tier.as_deref()) {
        (DEEPSEEK_FLASH, WireApi::Responses, Some("priority")) => Err(CodexErr::InvalidRequest(
            "Fireworks Priority requires wire_api = \"chat_completions\". Select Standard or Ultra-fast to use Responses.".into(),
        )),
        (DEEPSEEK_FLASH, _, Some("ultrafast")) => Ok((DEEPSEEK_FLASH_ULTRAFAST.to_string(), None)),
        _ => Ok((model.to_string(), service_tier)),
    }
}

pub(super) fn uses_function_exec(model: &ModelInfo, wire_api: WireApi) -> bool {
    model.slug == DEEPSEEK_FLASH && wire_api == WireApi::Responses && !model.use_responses_lite
}

pub(super) fn code_mode_exec_spec(spec: &ToolSpec) -> bool {
    matches!(spec, ToolSpec::Freeform(tool) if tool.name == PUBLIC_TOOL_NAME)
}

pub(super) fn project_tools(tools: &[ToolSpec]) -> serde_json::Result<Arc<RawValue>> {
    let Some(index) = tools.iter().position(code_mode_exec_spec) else {
        return codex_tools::create_tools_raw_json_for_responses_api(tools);
    };
    let ToolSpec::Freeform(FreeformTool { description, .. }) = &tools[index] else {
        unreachable!("Code Mode exec has a freeform spec");
    };
    let exec = ToolSpec::Function(ResponsesApiTool {
        name: PUBLIC_TOOL_NAME.to_string(),
        description: format!(
            "Pass the raw JavaScript source as the input string, without Markdown fences.\n\n{description}"
        ),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            BTreeMap::from([(
                "input".to_string(),
                JsonSchema::string(/*description*/ None),
            )]),
            Some(vec!["input".to_string()]),
            Some(false.into()),
        ),
        output_schema: None,
    });
    let mut tools: Vec<_> = tools.iter().collect();
    tools[index] = &exec;
    serde_json::value::to_raw_value(&tools).map(Arc::from)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ExecArguments<T> {
    input: T,
}

fn is_exec(name: &str, namespace: Option<&str>) -> bool {
    name == PUBLIC_TOOL_NAME
        && matches!(
            namespace,
            None | Some("") | Some(DEFAULT_FUNCTION_NAMESPACE)
        )
}

// A WebSocket delta may contain a result whose call is in its omitted prefix.
pub(super) fn project_input(
    input: &[Arc<ResponseItem>],
    history: &[Arc<ResponseItem>],
) -> Option<Vec<Arc<ResponseItem>>> {
    let exec_calls: HashSet<_> = history
        .iter()
        .filter_map(|item| match item.as_ref() {
            ResponseItem::CustomToolCall {
                name,
                namespace,
                call_id,
                ..
            } if is_exec(name, namespace.as_deref()) => Some(call_id.as_str()),
            _ => None,
        })
        .collect();
    if exec_calls.is_empty() {
        return None;
    }

    let mut projected = None;
    for (index, item) in input.iter().enumerate() {
        let wire_item = match item.as_ref() {
            ResponseItem::CustomToolCall {
                id,
                name,
                namespace,
                call_id,
                input,
                status,
                encrypted_content,
                internal_chat_message_metadata_passthrough,
            } if is_exec(name, namespace.as_deref()) => Some(ResponseItem::FunctionCall {
                id: id.clone(),
                name: name.clone(),
                namespace: None,
                call_id: call_id.clone(),
                arguments: serde_json::to_string(&ExecArguments { input })
                    .expect("serializing one string cannot fail"),
                status: status.clone(),
                encrypted_content: encrypted_content.clone(),
                encrypted_function_args: None,
                internal_chat_message_metadata_passthrough:
                    internal_chat_message_metadata_passthrough.clone(),
            }),
            ResponseItem::CustomToolCallOutput {
                id,
                call_id,
                name: None,
                output,
                internal_chat_message_metadata_passthrough,
            } if exec_calls.contains(call_id.as_str()) => Some(ResponseItem::FunctionCallOutput {
                id: id.clone(),
                call_id: Some(call_id.clone()),
                name: None,
                namespace: None,
                output: output.clone(),
                internal_chat_message_metadata_passthrough:
                    internal_chat_message_metadata_passthrough.clone(),
            }),
            ResponseItem::CustomToolCallOutput {
                call_id,
                name: Some(name),
                output,
                ..
            } if name == PUBLIC_TOOL_NAME && exec_calls.contains(call_id.as_str()) => {
                // An async notification is not a second result for a completed function call.
                let text = output.body.to_text().unwrap_or_default();
                Some(ResponseItem::Message {
                    id: None,
                    role: "user".to_string(),
                    content: Arc::new(vec![ContentItem::InputText {
                        text: format!("Additional output from {name} ({call_id}):\n{text}"),
                    }]),
                    status: None,
                    phase: None,
                    encrypted_content: None,
                    internal_chat_message_metadata_passthrough: None,
                })
            }
            _ => None,
        };
        if let Some(wire_item) = wire_item {
            projected
                .get_or_insert_with(|| input[..index].to_vec())
                .push(Arc::new(wire_item));
        } else if let Some(projected) = &mut projected {
            projected.push(Arc::clone(item));
        }
    }
    projected
}

pub(super) fn normalize_event(
    mut event: ResponseEvent,
) -> std::result::Result<ResponseEvent, ApiError> {
    let (item, completed) = match &mut event {
        ResponseEvent::OutputItemAdded(item) => (item, false),
        ResponseEvent::OutputItemDone(item) => (item, true),
        _ => return Ok(event),
    };
    if let ResponseItem::FunctionCall {
        id,
        name,
        namespace,
        call_id,
        arguments,
        status,
        encrypted_content,
        internal_chat_message_metadata_passthrough,
        ..
    } = item
        && is_exec(name, namespace.as_deref())
    {
        // Responses only provides complete JSON arguments in the done item.
        let input = if completed {
            let args = arguments
                .trim_start()
                .starts_with('{')
                .then(|| serde_json::from_str::<ExecArguments<String>>(arguments).ok())
                .flatten()
                .ok_or_else(|| {
                    ApiError::Stream("invalid Fireworks Code Mode exec arguments".into())
                })?;
            args.input
        } else {
            String::new()
        };
        *item = ResponseItem::CustomToolCall {
            id: id.take(),
            name: std::mem::take(name),
            namespace: namespace.take(),
            call_id: std::mem::take(call_id),
            input,
            status: status.take(),
            encrypted_content: encrypted_content.take(),
            internal_chat_message_metadata_passthrough: internal_chat_message_metadata_passthrough
                .take(),
        };
    }
    Ok(event)
}
