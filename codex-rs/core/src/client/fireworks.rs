use codex_api::ApiError;
use codex_api::ResponseEvent;
use codex_code_mode::PUBLIC_TOOL_NAME;
use codex_model_provider_info::WireApi;
use codex_protocol::DEFAULT_FUNCTION_NAMESPACE;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ModelInfo;
use codex_rollout_trace::ProjectedCodeModeOutput;
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
        (_, WireApi::Responses, Some(tier))
            if codex_models_manager::chat_completions_only_service_tiers(model)
                .iter()
                .any(|restricted| restricted == tier) =>
        {
            Err(CodexErr::InvalidRequest(
                "Fireworks Priority requires wire_api = \"chat_completions\". Select Standard or Ultra-fast to use Responses.".into(),
            ))
        }
        (DEEPSEEK_FLASH, _, Some("ultrafast")) => Ok((DEEPSEEK_FLASH_ULTRAFAST.to_string(), None)),
        _ => Ok((model.to_string(), service_tier)),
    }
}

pub(super) fn uses_function_exec(model: &ModelInfo, wire_api: WireApi) -> bool {
    model.slug == DEEPSEEK_FLASH && wire_api == WireApi::Responses && !model.use_responses_lite
}

pub(super) fn uses_ultrafast_router(model: &ModelInfo, wire_model: &str) -> bool {
    model.slug == DEEPSEEK_FLASH && wire_model == DEEPSEEK_FLASH_ULTRAFAST
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
    trace_projection: bool,
) -> (Option<Vec<Arc<ResponseItem>>>, Vec<ProjectedCodeModeOutput>) {
    let mut pending_outputs: HashSet<_> = input
        .iter()
        .filter_map(|item| match item.as_ref() {
            ResponseItem::CustomToolCallOutput { call_id, name, .. }
                if name.as_deref().is_none_or(|name| name == PUBLIC_TOOL_NAME) =>
            {
                Some(call_id.as_str())
            }
            _ => None,
        })
        .collect();
    let mut exec_calls = HashSet::new();
    if !pending_outputs.is_empty() {
        // Most continuation results belong to recent calls; stop once the wire slice
        // has no remaining outputs to classify.
        for item in history.iter().rev() {
            if let ResponseItem::CustomToolCall {
                name,
                namespace,
                call_id,
                ..
            } = item.as_ref()
                && pending_outputs.remove(call_id.as_str())
            {
                if is_exec(name, namespace.as_deref()) {
                    exec_calls.insert(call_id.as_str());
                }
                if pending_outputs.is_empty() {
                    break;
                }
            }
        }
    }

    let mut projected = None;
    let mut projected_outputs = Vec::new();
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
                id,
                call_id,
                name: Some(name),
                output,
                ..
            } if name == PUBLIC_TOOL_NAME && exec_calls.contains(call_id.as_str()) => {
                // An async notification is not a second result for a completed function call.
                if trace_projection {
                    projected_outputs.push(ProjectedCodeModeOutput {
                        index,
                        call_id: call_id.clone(),
                        history_id: id.as_ref().map(ToString::to_string),
                    });
                }
                let mut content = vec![ContentItem::InputText {
                    text: format!("Additional output from {name} ({call_id}):\n"),
                }];
                match &output.body {
                    FunctionCallOutputBody::Text(text) => {
                        content.push(ContentItem::InputText { text: text.clone() });
                    }
                    FunctionCallOutputBody::ContentItems(items) => {
                        content.extend(items.iter().filter_map(|item| match item {
                            FunctionCallOutputContentItem::InputText { text } => {
                                Some(ContentItem::InputText { text: text.clone() })
                            }
                            FunctionCallOutputContentItem::InputImage { image, detail } => {
                                Some(ContentItem::InputImage {
                                    image: image.clone(),
                                    detail: *detail,
                                })
                            }
                            FunctionCallOutputContentItem::InputAudio { audio } => {
                                Some(ContentItem::InputAudio {
                                    audio: audio.clone(),
                                })
                            }
                            FunctionCallOutputContentItem::EncryptedContent { .. } => None,
                        }));
                    }
                }
                Some(ResponseItem::Message {
                    id: None,
                    role: "user".to_string(),
                    content: Arc::new(content),
                    status: None,
                    phase: None,
                    encrypted_content: None,
                    internal_chat_message_metadata_passthrough: None,
                })
            }
            _ => None,
        };
        if let Some(wire_item) = wire_item {
            let projected = projected.get_or_insert_with(|| {
                let mut items = Vec::with_capacity(input.len());
                items.extend_from_slice(&input[..index]);
                items
            });
            projected.push(Arc::new(wire_item));
        } else if let Some(projected) = &mut projected {
            projected.push(Arc::clone(item));
        }
    }
    (projected, projected_outputs)
}

pub(super) fn normalize_router_echo(event: &mut ResponseEvent) {
    if let ResponseEvent::ServerModel(model) = event
        && model.as_str() == DEEPSEEK_FLASH_ULTRAFAST
    {
        *model = DEEPSEEK_FLASH.to_string();
    }
}

// Return whether the provider's call had to be repaired for replay.
pub(super) fn normalize_exec_event(
    event: &mut ResponseEvent,
) -> std::result::Result<bool, ApiError> {
    let (item, completed) = match event {
        ResponseEvent::OutputItemAdded(item) => (item, false),
        ResponseEvent::OutputItemDone(item) => (item, true),
        _ => return Ok(false),
    };
    let ResponseItem::FunctionCall {
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
    else {
        return Ok(false);
    };
    if !is_exec(name, namespace.as_deref()) {
        return Ok(false);
    }
    if completed && call_id.trim().is_empty() {
        return Err(ApiError::InvalidRequest {
            message: "Fireworks returned a Code Mode exec call without a call ID.".into(),
        });
    }
    // A malformed wrapper contains no usable source. The Code Mode parser rejects
    // empty input before starting a cell and returns a result for this call ID.
    let args = (completed && arguments.trim_start().starts_with('{'))
        .then(|| serde_json::from_str::<ExecArguments<String>>(arguments).ok())
        .flatten();
    let repaired = completed && args.is_none();
    *item = ResponseItem::CustomToolCall {
        id: id.take(),
        name: std::mem::take(name),
        namespace: namespace.take(),
        call_id: std::mem::take(call_id),
        input: args.map(|args| args.input).unwrap_or_default(),
        status: status.take(),
        encrypted_content: encrypted_content.take(),
        internal_chat_message_metadata_passthrough: internal_chat_message_metadata_passthrough
            .take(),
    };
    Ok(repaired)
}
