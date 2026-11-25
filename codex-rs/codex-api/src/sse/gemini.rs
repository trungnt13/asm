use super::chat::ChatToolCall;
use super::chat::assistant_message;
use crate::common::ResponseEvent;
use crate::common::ResponseStream;
use crate::error::ApiError;
use crate::requests::chat::ChatTool;
use crate::requests::gemini::THOUGHT_PREFIX;
use crate::telemetry::SseTelemetry;
use codex_client::StreamResponse;
use codex_client::TransportError;
use codex_protocol::error::ResponseIncompleteError;
use codex_protocol::models::ReasoningItemReasoningSummary;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TokenUsage;
use eventsource_stream::EventStreamError;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio::time::timeout;
use uuid::Uuid;

pub(crate) fn spawn_gemini_stream(
    stream: StreamResponse,
    idle_timeout: Duration,
    tool_names: HashMap<String, ChatTool>,
    telemetry: Option<Arc<dyn SseTelemetry>>,
    request_started_at: Instant,
) -> ResponseStream {
    let upstream_request_id = stream
        .headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let (events, rx_event) = mpsc::channel(/*buffer*/ 1600);
    tokio::spawn(async move {
        if let Err(error) = decode_gemini_stream(
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
async fn decode_gemini_stream(
    stream: StreamResponse,
    idle_timeout: Duration,
    tool_names: HashMap<String, ChatTool>,
    telemetry: Option<Arc<dyn SseTelemetry>>,
    request_started_at: Instant,
    events: &mpsc::Sender<Result<ResponseEvent, ApiError>>,
) -> Result<(), ApiError> {
    let stream_open_ms = request_started_at.elapsed().as_millis();
    let mut stream = stream.bytes.eventsource();
    let mut provider_id: Option<String> = None;
    let mut response_id = None;
    let mut step: Option<(Value, Option<String>)> = None;
    let mut next_index = 0;
    let mut call_ids = HashSet::new();
    let mut pending_output = Vec::new();
    let mut usage = GeminiUsage::default();
    let mut first_tool_start_ms = None;
    let mut last_tool_stop_ms = None;
    let malformed = || stream_error("invalid or unsupported Gemini SSE event");
    let (response_id, end_turn, completion_ms) = loop {
        let start = Instant::now();
        let poll = tokio::select! {
            biased;
            _ = events.closed() => return Ok(()),
            poll = timeout(idle_timeout, stream.next()) => poll,
        };
        // Responses telemetry can log provider frame contents on errors.
        if let Some(telemetry) = &telemetry
            && matches!(&poll, Ok(None) | Err(_))
        {
            telemetry.on_sse_poll(&poll, start.elapsed());
        }
        let frame = match poll {
            Ok(Some(Ok(frame))) => frame,
            Ok(Some(Err(EventStreamError::Transport(error @ TransportError::Policy(_))))) => {
                return Err(ApiError::Transport(error));
            }
            Ok(Some(Err(_))) => return Err(stream_error("invalid Gemini SSE stream")),
            Ok(None) => return Err(stream_error("Gemini SSE ended before completion")),
            Err(_) => return Err(stream_error("idle timeout waiting for Gemini SSE")),
        };
        let mut frame: Value = serde_json::from_str(&frame.data).map_err(|_| malformed())?;
        let interaction = &frame["interaction"];
        let index_matches = frame["index"].as_u64() == Some(next_index);
        let matches_provider_id = |id: &Value| {
            id.is_null()
                || id.as_str().is_some_and(|id| {
                    !id.is_empty() && provider_id.as_deref().is_none_or(|expected| expected == id)
                })
        };
        match frame["event_type"].as_str() {
            Some("interaction.created")
                if response_id.is_none()
                    && interaction.is_object()
                    && matches_provider_id(&interaction["id"]) =>
            {
                provider_id = interaction["id"].as_str().map(str::to_owned);
                // Stateless Interactions omit the provider ID; ASM still needs a stream ID.
                response_id = Some(
                    provider_id
                        .clone()
                        .unwrap_or_else(|| format!("gemini_{}", Uuid::new_v4().simple())),
                );
                let _ = events
                    .send(Ok(ResponseEvent::Created {
                        response_id: response_id.clone(),
                    }))
                    .await;
            }
            Some("error") => return Err(stream_error("provider reported a Gemini stream error")),
            _ if response_id.is_none() => return Err(malformed()),
            Some("interaction.status_update") if matches_provider_id(&frame["interaction_id"]) => {
                usage.record(&frame["usage"]);
            }
            Some("step.start") if step.is_none() && index_matches => {
                let wire_step = frame["step"].take();
                let mut fragments = None;
                match wire_step["type"].as_str() {
                    Some("model_output") => {
                        if let Some(parts) =
                            wire_step.get("content").filter(|parts| !parts.is_null())
                        {
                            for part in parts.as_array().ok_or_else(malformed)? {
                                let text = part["text"]
                                    .as_str()
                                    .filter(|_| part["type"] == "text")
                                    .ok_or_else(malformed)?;
                                append_output_text(
                                    events,
                                    &mut fragments,
                                    text,
                                    /*stream*/ call_ids.is_empty(),
                                )
                                .await;
                            }
                        }
                    }
                    Some("thought") => {}
                    Some("function_call") => {
                        first_tool_start_ms
                            .get_or_insert_with(|| request_started_at.elapsed().as_millis());
                    }
                    _ => return Err(malformed()),
                }
                step = Some((wire_step, fragments));
            }
            Some("step.delta") if index_matches => {
                let (step, fragments) = step.as_mut().ok_or_else(malformed)?;
                let mut delta = frame["delta"].take();
                match (step["type"].as_str(), delta["type"].as_str()) {
                    (Some("model_output"), Some("text")) => {
                        let text = delta["text"].as_str().ok_or_else(malformed)?;
                        append_output_text(
                            events,
                            fragments,
                            text,
                            /*stream*/ call_ids.is_empty(),
                        )
                        .await;
                    }
                    (Some("thought"), Some("thought_signature"))
                        if delta["signature"].is_string()
                            && step["signature"].as_str().is_none_or(str::is_empty) =>
                    {
                        step["signature"] = delta["signature"].take();
                    }
                    (Some("thought"), Some("thought_summary")) if delta["content"].is_object() => {
                        if step["summary"].is_null() {
                            step["summary"] = Value::Array(Vec::new());
                        }
                        step["summary"]
                            .as_array_mut()
                            .ok_or_else(malformed)?
                            .push(delta["content"].take());
                    }
                    (Some("function_call"), Some("arguments_delta")) => {
                        fragments
                            .get_or_insert_default()
                            .push_str(delta["arguments"].as_str().ok_or_else(malformed)?);
                    }
                    _ => return Err(malformed()),
                }
            }
            Some("step.stop") if index_matches => {
                let (wire_step, fragments) = step.take().ok_or_else(malformed)?;
                next_index += 1;
                usage.record(&frame["usage"]);
                let item = match wire_step["type"].as_str() {
                    Some("model_output") => {
                        let Some(text) = fragments else { continue };
                        assistant_message(text, /*status*/ None)
                    }
                    Some("thought") => thought_item(&wire_step),
                    Some("function_call") => {
                        last_tool_stop_ms = Some(request_started_at.elapsed().as_millis());
                        let id = wire_step["id"]
                            .as_str()
                            .filter(|id| !id.is_empty())
                            .ok_or_else(malformed)?;
                        if !call_ids.insert(id.to_owned()) {
                            return Err(stream_error("duplicate Gemini tool call ID"));
                        }
                        let name = wire_step["name"].as_str().ok_or_else(malformed)?;
                        let tool = tool_names
                            .get(name)
                            .ok_or_else(|| stream_error("Gemini called an unadvertised tool"))?;
                        let initial_arguments =
                            wire_step["arguments"].as_object().ok_or_else(malformed)?;
                        if fragments.is_some() && !initial_arguments.is_empty() {
                            return Err(malformed());
                        }
                        ChatToolCall {
                            id: id.to_owned(),
                            name: name.to_owned(),
                            arguments: fragments
                                .unwrap_or_else(|| wire_step["arguments"].to_string()),
                        }
                        .into_item(tool)
                        .map_err(|_| stream_error("invalid Gemini tool arguments"))?
                    }
                    _ => return Err(malformed()),
                };
                // Validate the entire tool batch and terminal status before dispatching any call.
                if call_ids.is_empty() {
                    let _ = events.send(Ok(ResponseEvent::OutputItemDone(item))).await;
                } else {
                    pending_output.push(item);
                }
            }
            Some("interaction.completed")
                if matches_provider_id(&interaction["id"]) && step.is_none() =>
            {
                usage.record(&interaction["usage"]);
                let completion_ms = request_started_at.elapsed().as_millis();
                tracing::debug!(
                    stream_open_ms,
                    ?first_tool_start_ms,
                    ?last_tool_stop_ms,
                    completion_ms,
                    tool_calls = call_ids.len(),
                    terminal_cached_tokens = ?interaction["usage"]["total_cached_tokens"].as_i64(),
                    reported_cached_tokens = ?usage.reported_cached_tokens,
                    "Gemini interaction terminal frame received"
                );
                let end_turn = match interaction["status"].as_str() {
                    Some("completed") if call_ids.is_empty() => true,
                    Some("requires_action") if !call_ids.is_empty() => false,
                    Some("cancelled") => {
                        return Err(stream_error("Gemini cancelled the interaction"));
                    }
                    Some("failed") => return Err(stream_error("Gemini interaction failed")),
                    Some(reason @ ("incomplete" | "requires_continuation")) => {
                        return Err(ApiError::ResponseIncomplete(Box::new(
                            ResponseIncompleteError {
                                reason: reason.to_owned(),
                                partial_output: Vec::new(),
                                response_id,
                                token_usage: usage.totals,
                            },
                        )));
                    }
                    _ => return Err(stream_error("unexpected Gemini interaction status")),
                };
                break (response_id.ok_or_else(malformed)?, end_turn, completion_ms);
            }
            _ => return Err(malformed()),
        }
    };
    for item in pending_output {
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
            token_usage: usage.totals,
            usage_metadata: None,
            request_duration_ms: Some(i64::try_from(completion_ms).unwrap_or(i64::MAX)),
            generation_duration_ms: None,
            reasoning_removals: None,
            end_turn: Some(end_turn),
        }))
        .await;
    Ok(())
}

#[tracing::instrument(level = "trace", skip_all)]
async fn append_output_text(
    events: &mpsc::Sender<Result<ResponseEvent, ApiError>>,
    fragments: &mut Option<String>,
    text: &str,
    stream: bool,
) {
    if text.is_empty() {
        return;
    }
    if stream {
        if fragments.is_none() {
            let shell = assistant_message(String::new(), /*status*/ None);
            let _ = events.send(Ok(ResponseEvent::OutputItemAdded(shell))).await;
        }
        let _ = events
            .send(Ok(ResponseEvent::OutputTextDelta(text.to_owned())))
            .await;
    }
    fragments.get_or_insert_default().push_str(text);
}

fn thought_item(step: &Value) -> ResponseItem {
    let summary = step["summary"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| {
            let text = part["text"].as_str()?;
            (part["type"] == "text").then(|| ReasoningItemReasoningSummary::SummaryText {
                text: text.to_owned(),
            })
        })
        .collect();
    ResponseItem::Reasoning {
        id: None,
        summary,
        content: None,
        encrypted_content: Some(format!("{THOUGHT_PREFIX}{step}")),
        internal_chat_message_metadata_passthrough: None,
    }
}

#[derive(Default)]
struct GeminiUsage {
    totals: Option<TokenUsage>,
    reported_cached_tokens: Option<i64>,
}

impl GeminiUsage {
    fn record(&mut self, usage: &Value) {
        // Usage updates are cumulative, but may omit individual counters.
        self.reported_cached_tokens = usage["total_cached_tokens"]
            .as_i64()
            .or(self.reported_cached_tokens);
        if let (Some(input_tokens), Some(total_tokens)) = (
            usage["total_input_tokens"].as_i64(),
            usage["total_tokens"].as_i64(),
        ) {
            let reasoning_output_tokens =
                usage["total_thought_tokens"].as_i64().unwrap_or_default();
            self.totals = Some(TokenUsage {
                input_tokens,
                // Gemini reports visible output and thought tokens separately.
                output_tokens: usage["total_output_tokens"]
                    .as_i64()
                    .unwrap_or_default()
                    .saturating_add(reasoning_output_tokens),
                reasoning_output_tokens,
                total_tokens,
                ..Default::default()
            });
        }
        if let Some(totals) = &mut self.totals {
            totals.cached_input_tokens = self.reported_cached_tokens.unwrap_or_default();
        }
    }
}

fn stream_error(message: &str) -> ApiError {
    ApiError::Stream(message.to_owned())
}
