use crate::common::ResponseEvent;
use crate::common::ResponseStream;
use crate::error::ApiError;
use crate::error::parse_terminal_provider_error;
use crate::telemetry::SseTelemetry;
use codex_client::StreamResponse;
use codex_client::TransportError;
use codex_protocol::ToolName;
use codex_protocol::error::ResponseIncompleteError;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TokenUsage;
use eventsource_stream::EventStreamError;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio::time::timeout;

pub(crate) fn spawn_chat_stream(
    stream: StreamResponse,
    idle_timeout: Duration,
    tool_names: HashMap<String, ToolName>,
    telemetry: Option<Arc<dyn SseTelemetry>>,
    request_started_at: Instant,
) -> ResponseStream {
    let upstream_request_id = stream
        .headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let (events, rx_event) = mpsc::channel(1600);
    tokio::spawn(async move {
        if let Err(error) = decode_chat_stream(
            stream,
            idle_timeout,
            tool_names,
            telemetry,
            request_started_at,
            &events,
        )
        .await
        {
            let _ = events.send(Err(error)).await;
        }
    });
    ResponseStream {
        routing_affinity: None,
        rx_event,
        upstream_request_id,
        interrupt: None,
    }
}

#[tracing::instrument(level = "debug", skip_all)]
async fn decode_chat_stream(
    stream: StreamResponse,
    idle_timeout: Duration,
    tool_names: HashMap<String, ToolName>,
    telemetry: Option<Arc<dyn SseTelemetry>>,
    request_started_at: Instant,
    events: &mpsc::Sender<Result<ResponseEvent, ApiError>>,
) -> Result<(), ApiError> {
    let mut stream = stream.bytes.eventsource();
    let mut response_id = None;
    let mut usage = None;
    let mut finish_reason = None;
    let mut content = String::new();
    let mut calls = BTreeMap::<usize, ChatToolCall>::new();

    let completed_at = loop {
        let start = Instant::now();
        let poll = tokio::select! {
            biased;
            _ = events.closed() => return Ok(()),
            poll = timeout(idle_timeout, stream.next()) => poll,
        };
        // The shared metrics path interprets named Responses events and may log their bodies.
        if let Some(telemetry) = &telemetry
            && match &poll {
                Ok(Some(Ok(frame))) => frame.event == "message",
                Ok(Some(Err(_))) => false,
                Ok(None) | Err(_) => true,
            }
        {
            telemetry.on_sse_poll(&poll, start.elapsed());
        }
        let frame = match poll {
            Ok(Some(Ok(frame))) => frame,
            Ok(Some(Err(EventStreamError::Transport(error @ TransportError::Policy(_))))) => {
                return Err(ApiError::Transport(error));
            }
            Ok(Some(Err(_))) => return Err(ApiError::Stream("invalid chat SSE stream".into())),
            Ok(None) => return Err(ApiError::Stream("chat stream closed before [DONE]".into())),
            Err(_) => return Err(ApiError::Stream("idle timeout waiting for chat SSE".into())),
        };
        let data = frame.data.trim();
        if data == "[DONE]" {
            break Instant::now();
        }
        if data.is_empty() {
            continue;
        }
        let chunk: ChatChunk = serde_json::from_str(data)
            .map_err(|_| ApiError::Stream("invalid chat SSE chunk".into()))?;
        if let Some(error) = chunk.error {
            let code_only = serde_json::json!({ "code": error.get("code") });
            return Err(
                parse_terminal_provider_error(Some(&code_only), /*status*/ None).unwrap_or_else(
                    || ApiError::Stream("provider reported a chat stream error".into()),
                ),
            );
        }
        if let Some(id) = chunk.id.filter(|id| !id.is_empty()) {
            if let Some(current) = response_id.as_ref() {
                if current != &id {
                    return Err(ApiError::Stream(
                        "chat response ID changed midstream".into(),
                    ));
                }
            } else {
                response_id = Some(id.clone());
                if events
                    .send(Ok(ResponseEvent::Created {
                        response_id: Some(id),
                    }))
                    .await
                    .is_err()
                {
                    return Ok(());
                }
            }
        }
        if let Some(reported_usage) = chunk.usage {
            usage = Some(reported_usage.into());
        }
        let mut choices = chunk.choices.into_iter();
        let Some(choice) = choices.next() else {
            continue;
        };
        if choice.index != 0 || choices.next().is_some() || finish_reason.is_some() {
            return Err(ApiError::Stream("unexpected chat stream choice".into()));
        }
        finish_reason = choice.finish_reason;
        if let Some(delta) = choice
            .delta
            .content
            .filter(|delta| !delta.is_empty())
            .or(choice.delta.refusal.filter(|delta| !delta.is_empty()))
        {
            if content.is_empty()
                && events
                    .send(Ok(ResponseEvent::OutputItemAdded(assistant_message(
                        String::new(),
                    ))))
                    .await
                    .is_err()
            {
                return Ok(());
            }
            content.push_str(&delta);
            if events
                .send(Ok(ResponseEvent::OutputTextDelta(delta)))
                .await
                .is_err()
            {
                return Ok(());
            }
        }
        for part in choice.delta.tool_calls {
            if part.kind.as_deref().is_some_and(|kind| kind != "function") {
                return Err(ApiError::Stream("unsupported chat tool call type".into()));
            }
            let call = calls.entry(part.index).or_default();
            if let Some(id) = part.id.filter(|id| !id.is_empty()) {
                if call.id.is_empty() {
                    call.id = id;
                } else if call.id != id {
                    return Err(ApiError::Stream(
                        "chat tool call ID changed midstream".into(),
                    ));
                }
            }
            if let Some(function) = part.function {
                if let Some(name) = function.name {
                    call.name.push_str(&name);
                }
                if let Some(arguments) = function.arguments {
                    call.arguments.push_str(&arguments);
                }
            }
        }
    };

    let end_turn = match finish_reason.as_deref() {
        Some("stop") => calls.is_empty(),
        Some("tool_calls") if !calls.is_empty() => false,
        Some("content_filter") => return Err(ApiError::ContentFilter),
        Some("length") => {
            return Err(ApiError::ResponseIncomplete(Box::new(
                ResponseIncompleteError {
                    reason: "max_output_tokens".to_owned(),
                    partial_output: Vec::new(),
                    response_id,
                    token_usage: usage,
                },
            )));
        }
        _ => {
            return Err(ApiError::Stream(
                "missing or unsupported chat finish reason".into(),
            ));
        }
    };
    let response_id =
        response_id.ok_or_else(|| ApiError::Stream("missing chat response ID".into()))?;

    // A valid prefix of a malformed tool batch must not reach the tool dispatcher.
    let mut ids = HashSet::new();
    let mut items = Vec::with_capacity(calls.len() + usize::from(!content.is_empty()));
    if !content.is_empty() {
        items.push(assistant_message(content));
    }
    for call in calls.into_values() {
        let tool = tool_names
            .get(&call.name)
            .ok_or_else(|| ApiError::Stream("chat response called an unadvertised tool".into()))?;
        if call.id.is_empty()
            || !ids.insert(call.id.clone())
            || !matches!(
                serde_json::from_str::<Value>(&call.arguments),
                Ok(Value::Object(_))
            )
        {
            return Err(ApiError::Stream("invalid chat tool call".into()));
        }
        items.push(ResponseItem::FunctionCall {
            encrypted_content: None,
            status: None,
            id: None,
            name: tool.name.clone(),
            namespace: tool.namespace.clone(),
            arguments: call.arguments,
            encrypted_function_args: None,
            call_id: call.id,
            internal_chat_message_metadata_passthrough: None,
        });
    }
    for item in items {
        if events
            .send(Ok(ResponseEvent::OutputItemDone(item)))
            .await
            .is_err()
        {
            return Ok(());
        }
    }

    let _ = events
        .send(Ok(ResponseEvent::Completed {
            response_id,
            token_usage: usage,
            usage_metadata: None,
            request_duration_ms: Some(
                i64::try_from(completed_at.duration_since(request_started_at).as_millis())
                    .unwrap_or(i64::MAX),
            ),
            generation_duration_ms: None,
            reasoning_removals: None,
            end_turn: Some(end_turn),
        }))
        .await;
    Ok(())
}

fn assistant_message(text: String) -> ResponseItem {
    ResponseItem::Message {
        encrypted_content: None,
        status: None,
        id: None,
        role: "assistant".to_owned(),
        content: Arc::new(vec![ContentItem::OutputText {
            text,
            annotations: None,
            logprobs: None,
        }]),
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

#[derive(Deserialize)]
struct ChatChunk {
    id: Option<String>,
    #[serde(default)]
    choices: Vec<ChatChoice>,
    usage: Option<ChatUsage>,
    error: Option<Value>,
}

#[derive(Deserialize)]
struct ChatChoice {
    index: usize,
    #[serde(default)]
    delta: ChatDelta,
    finish_reason: Option<String>,
}

#[derive(Default, Deserialize)]
struct ChatDelta {
    content: Option<String>,
    refusal: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ChatToolCallDelta>,
}

#[derive(Deserialize)]
struct ChatToolCallDelta {
    index: usize,
    id: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    function: Option<ChatFunctionDelta>,
}

#[derive(Deserialize)]
struct ChatFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Default)]
struct ChatToolCall {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct ChatUsage {
    prompt_tokens: i64,
    completion_tokens: i64,
    total_tokens: i64,
    prompt_tokens_details: Option<ChatPromptTokensDetails>,
    completion_tokens_details: Option<ChatCompletionTokensDetails>,
}

#[derive(Deserialize)]
struct ChatPromptTokensDetails {
    cached_tokens: Option<i64>,
}

#[derive(Deserialize)]
struct ChatCompletionTokensDetails {
    reasoning_tokens: Option<i64>,
}

impl From<ChatUsage> for TokenUsage {
    fn from(usage: ChatUsage) -> Self {
        Self {
            input_tokens: usage.prompt_tokens,
            cached_input_tokens: usage
                .prompt_tokens_details
                .and_then(|details| details.cached_tokens)
                .unwrap_or_default(),
            output_tokens: usage.completion_tokens,
            reasoning_output_tokens: usage
                .completion_tokens_details
                .and_then(|details| details.reasoning_tokens)
                .unwrap_or_default(),
            total_tokens: usage.total_tokens,
            ..Self::default()
        }
    }
}
