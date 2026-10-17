//! Thread-scoped MCP ingress; execution stays in ASM's captured tool runtime.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Weak;
use std::time::Duration;

use anyhow::Context;
use anyhow::bail;
use axum::Json;
use axum::Router;
use axum::body::to_bytes;
use axum::extract::Request;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::post;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ImageReference;
use codex_protocol::models::ResponseItem;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde_json::Value;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::session::session::Session;
use crate::session::step_context::StepContext;
use crate::tools::context::ToolPayload;
use crate::tools::parallel::ToolCallRuntime;
use crate::tools::router::ToolCall;
use crate::turn_diff_tracker::TurnDiffTracker;

const MAX_REQUEST_BYTES: usize = 1024 * 1024;

pub(super) struct ClaudeToolBridge {
    state: Arc<BridgeState>,
    stop: CancellationToken,
    server: Option<JoinHandle<std::io::Result<()>>>,
}

struct BridgeState {
    authority: String,
    token: String,
    session_id: String,
    turn: Mutex<Option<Arc<BridgeTurn>>>,
    requests: Semaphore,
    stop: CancellationToken,
    tool_specs: Mutex<Option<Vec<Value>>>,
}

struct BridgeTurn {
    session: Weak<Session>,
    stop: CancellationToken,
    step: Arc<StepContext>,
    cancel: CancellationToken,
    tools: BTreeMap<String, BridgeTool>,
    execution: Arc<Semaphore>,
    tracker: crate::tools::context::SharedTurnDiffTracker,
}

struct BridgeTool {
    name: ToolName,
    custom: bool,
    spec: Value,
}

impl ClaudeToolBridge {
    #[tracing::instrument(level = "trace", skip_all, err)]
    pub(super) async fn start() -> anyhow::Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let authority = listener.local_addr()?.to_string();
        let stop = CancellationToken::new();
        let state = Arc::new(BridgeState {
            authority,
            token: Uuid::new_v4().to_string(),
            session_id: Uuid::new_v4().to_string(),
            turn: Mutex::new(/*t*/ None),
            requests: Semaphore::new(/*permits*/ 8),
            stop: stop.clone(),
            tool_specs: Mutex::new(/*t*/ None),
        });
        let app = Router::new()
            .route("/mcp", post(serve_rpc))
            .with_state(Arc::clone(&state));
        let shutdown = stop.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
        });
        Ok(Self {
            state,
            stop,
            server: Some(server),
        })
    }

    pub(super) fn url(&self) -> String {
        format!("http://{}/mcp", self.state.authority)
    }

    pub(super) fn bearer_token(&self) -> &str {
        &self.state.token
    }

    #[tracing::instrument(level = "trace", skip_all, err)]
    pub(super) async fn set_turn(
        &self,
        session: Arc<Session>,
        step: Arc<StepContext>,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        if self.stop.is_cancelled() {
            bail!("Claude tool bridge is stopped");
        }
        let tools = build_tools(&step)?;
        let specs = tools
            .values()
            .map(|tool| tool.spec.clone())
            .collect::<Vec<_>>();
        {
            let mut original_specs = self.state.tool_specs.lock().await;
            if original_specs
                .as_ref()
                .is_some_and(|original| *original != specs)
            {
                bail!("Claude runtime tool definitions changed; start a new thread");
            }
            *original_specs = Some(specs);
        }
        let mut turn = self.state.turn.lock().await;
        if turn.is_some() {
            bail!("Claude tool bridge already has an active turn");
        }
        *turn = Some(Arc::new(BridgeTurn {
            session: Arc::downgrade(&session),
            stop: self.stop.clone(),
            step,
            cancel: cancel.child_token(),
            tools,
            execution: Arc::new(Semaphore::new(/*permits*/ 1)),
            tracker: Arc::new(Mutex::new(TurnDiffTracker::new())),
        }));
        Ok(())
    }

    pub(super) async fn tool_names(&self) -> Vec<String> {
        self.state
            .turn
            .lock()
            .await
            .as_ref()
            .map(|turn| turn.tools.keys().cloned().collect())
            .unwrap_or_default()
    }

    #[tracing::instrument(level = "trace", skip_all, err)]
    pub(super) async fn clear_turn(&self) -> anyhow::Result<()> {
        let turn = self.state.turn.lock().await.take();
        if let Some(turn) = turn {
            turn.cancel.cancel();
            // Revoke first, then drain. A timed-out runtime is permanently dirty.
            if tokio::time::timeout(Duration::from_secs(/*secs*/ 5), turn.execution.acquire())
                .await
                .is_err()
            {
                self.stop.cancel();
                bail!("Claude tool cleanup exceeded 5 seconds; bridge cannot be reused");
            }
        }
        Ok(())
    }

    #[tracing::instrument(level = "trace", skip_all, err)]
    pub(super) async fn shutdown(&mut self) -> anyhow::Result<()> {
        let drained = self.clear_turn().await;
        self.stop.cancel();
        if let Some(mut server) = self.server.take() {
            match tokio::time::timeout(Duration::from_secs(/*secs*/ 5), &mut server).await {
                Ok(result) => result.context("Claude tool HTTP task failed")??,
                Err(_) => {
                    server.abort();
                    bail!("Claude tool HTTP shutdown exceeded 5 seconds");
                }
            }
        }
        drained
    }
}

impl Drop for ClaudeToolBridge {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Ok(mut turn) = self.state.turn.try_lock()
            && let Some(turn) = turn.take()
        {
            turn.cancel.cancel();
        }
        // Graceful shutdown owns connection draining; aborting the accept loop here
        // would detach HTTP tasks that still hold execution authority.
    }
}

#[tracing::instrument(level = "trace", skip_all)]
async fn serve_rpc(State(state): State<Arc<BridgeState>>, request: Request) -> Response {
    let Ok(_request_slot) = state.requests.try_acquire() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    if state.stop.is_cancelled() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let headers = request.headers();
    if headers.get("host").and_then(|v| v.to_str().ok()) != Some(state.authority.as_str())
        || headers.get("authorization").and_then(|v| v.to_str().ok())
            != Some(format!("Bearer {}", state.token).as_str())
        || headers.contains_key("origin")
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let supplied_session = headers.get("mcp-session-id").and_then(|v| v.to_str().ok());
    if supplied_session.is_some_and(|id| id != state.session_id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let session_supplied = supplied_session.is_some();
    if headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_none_or(|v| v.split(';').next() != Some("application/json"))
    {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    let bytes = match tokio::time::timeout(
        Duration::from_secs(/*secs*/ 10),
        to_bytes(request.into_body(), MAX_REQUEST_BYTES),
    )
    .await
    {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(_)) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
        Err(_) => return StatusCode::REQUEST_TIMEOUT.into_response(),
    };
    let Ok(rpc) = serde_json::from_slice::<Value>(&bytes) else {
        return rpc_error(Value::Null, /*code*/ -32700, "Invalid JSON");
    };
    let id = rpc.get("id").cloned().unwrap_or(Value::Null);
    if rpc.get("jsonrpc").and_then(Value::as_str) != Some("2.0") || !rpc.is_object() {
        return rpc_error(id, /*code*/ -32600, "Invalid request");
    }
    let method = rpc.get("method").and_then(Value::as_str).unwrap_or("");
    if method == "initialize" {
        let version = rpc
            .pointer("/params/protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or("");
        let version = match version {
            "2024-11-05" | "2025-03-26" | "2025-06-18" => version,
            _ => "2025-03-26",
        };
        return (
            [("mcp-session-id", state.session_id.clone())],
            Json(json!({
                "jsonrpc":"2.0", "id":id, "result": {
                    "protocolVersion":version, "capabilities":{"tools":{}},
                    "serverInfo":{"name":"asm", "version":env!("CARGO_PKG_VERSION")}
                }
            })),
        )
            .into_response();
    }
    if !session_supplied {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if method == "notifications/initialized" || method == "notifications/cancelled" {
        // Turn cancellation is controlled by ASM, not by an uncorrelated request id.
        return StatusCode::ACCEPTED.into_response();
    }
    if method == "ping" {
        return rpc_result(id, json!({}));
    }
    if method == "tools/list" && !state.stop.is_cancelled() {
        // Discovery has no execution authority and must also work between turns.
        let tools = state.tool_specs.lock().await.clone().unwrap_or_default();
        return rpc_result(id, json!({"tools": tools}));
    }
    let Some(turn) = state.turn.lock().await.clone() else {
        return rpc_error(id, /*code*/ -32000, "No active ASM turn");
    };
    if turn.cancel.is_cancelled() {
        return rpc_error(id, /*code*/ -32000, "ASM turn cancelled");
    }
    match method {
        "tools/call" if !id.is_null() => {
            let params = rpc.get("params").cloned().unwrap_or(Value::Null);
            // Keep admitted work alive on HTTP disconnect until ASM cancels/drains it.
            match tokio::spawn(invoke_tool(turn, params))
                .await
                .unwrap_or_else(|error| Err(error.into()))
            {
                Ok(result) => rpc_result(id, result),
                Err(error) => rpc_result(
                    id,
                    json!({"isError":true,"content":[{"type":"text","text":error.to_string()}]}),
                ),
            }
        }
        _ => rpc_error(id, /*code*/ -32601, "Unsupported MCP method"),
    }
}

fn rpc_result(id: Value, result: Value) -> Response {
    Json(json!({"jsonrpc":"2.0","id":id,"result":result})).into_response()
}

fn rpc_error(id: Value, code: i64, message: &str) -> Response {
    Json(json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})).into_response()
}

fn build_tools(step: &StepContext) -> anyhow::Result<BTreeMap<String, BridgeTool>> {
    let mut tools = BTreeMap::new();
    for spec in step.tool_router.model_visible_specs().iter() {
        let entries = match spec {
            ToolSpec::Function(tool) => vec![(
                ToolName::plain(&tool.name),
                false,
                serde_json::to_value(tool)?,
            )],
            ToolSpec::Freeform(tool) => vec![(
                ToolName::plain(&tool.name),
                true,
                serde_json::to_value(tool)?,
            )],
            ToolSpec::Namespace(namespace) => namespace
                .tools
                .iter()
                .map(|tool| match tool {
                    ResponsesApiNamespaceTool::Function(tool) => Ok((
                        ToolName::namespaced(&namespace.name, &tool.name),
                        false,
                        serde_json::to_value(tool)?,
                    )),
                    ResponsesApiNamespaceTool::Custom(tool) => Ok((
                        ToolName::namespaced(&namespace.name, &tool.name),
                        true,
                        serde_json::to_value(tool)?,
                    )),
                })
                .collect::<anyhow::Result<Vec<_>>>()?,
            ToolSpec::WebSearch { .. } | ToolSpec::ToolSearch { .. } => bail!(
                "Claude runtime does not support hosted/deferred tool {}: disable it explicitly",
                spec.name()
            ),
        };
        for (name, custom, spec) in entries {
            let wire_name = match &name.namespace {
                Some(namespace) => format!("{namespace}__{}", name.name),
                None => name.name.clone(),
            };
            if wire_name.len() > 64
                || !wire_name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            {
                bail!("Tool name is not MCP-compatible: {wire_name}");
            }
            let schema = if custom {
                json!({"type":"object","properties":{"input":{"type":"string", "description":"Complete tool input, unchanged."}},"required":["input"],"additionalProperties":false})
            } else {
                spec.get("parameters")
                    .context("Tool has no input schema")?
                    .clone()
            };
            let description = spec
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("");
            let tool = BridgeTool {
                name,
                custom,
                spec: json!({"name":wire_name,"description":description,"inputSchema":schema}),
            };
            if tools.insert(wire_name.clone(), tool).is_some() {
                bail!("Colliding MCP tool name: {wire_name}");
            }
        }
    }
    Ok(tools)
}

#[tracing::instrument(level = "trace", skip_all, err)]
async fn invoke_tool(turn: Arc<BridgeTurn>, params: Value) -> anyhow::Result<Value> {
    let _execution = tokio::select! {
        permit = turn.execution.acquire() => permit?,
        _ = turn.cancel.cancelled() => bail!("ASM turn cancelled"),
        _ = turn.stop.cancelled() => bail!("ASM bridge stopped"),
    };
    if turn.cancel.is_cancelled() || turn.stop.is_cancelled() {
        bail!("ASM turn cancelled");
    }
    let session = turn.session.upgrade().context("ASM thread stopped")?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .context("Missing tool name")?;
    let tool = turn
        .tools
        .get(name)
        .context("Tool is not exposed by this ASM turn")?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if !arguments.is_object() {
        bail!("Tool arguments must be an object");
    }
    let call_id = format!("claude-{}", Uuid::new_v4());
    let (payload, item) = if tool.custom {
        let input = arguments
            .get("input")
            .and_then(Value::as_str)
            .context("Freeform tool needs string input")?
            .to_string();
        (
            ToolPayload::Custom {
                input: input.clone(),
            },
            ResponseItem::CustomToolCall {
                encrypted_content: None,
                id: None,
                status: None,
                call_id: call_id.clone(),
                name: tool.name.name.clone(),
                namespace: tool.name.namespace.clone(),
                input,
                internal_chat_message_metadata_passthrough: None,
            },
        )
    } else {
        let arguments = arguments.to_string();
        (
            ToolPayload::Function {
                arguments: arguments.clone(),
            },
            ResponseItem::FunctionCall {
                encrypted_content: None,
                status: None,
                id: None,
                name: tool.name.name.clone(),
                namespace: tool.name.namespace.clone(),
                arguments,
                encrypted_function_args: None,
                call_id: call_id.clone(),
                internal_chat_message_metadata_passthrough: None,
            },
        )
    };
    session
        .record_conversation_items(&turn.step.turn, &turn.step.settings.model_info, &[item])
        .await;
    let call = ToolCall {
        tool_name: tool.name.clone(),
        call_id,
        payload,
        encrypted_function_args: None,
    };
    let runtime = ToolCallRuntime::new(
        Arc::clone(&session),
        Arc::clone(&turn.step),
        Arc::clone(&turn.tracker),
    );
    let dispatch = runtime.handle_tool_call(call, turn.cancel.child_token());
    tokio::pin!(dispatch);
    let output = tokio::select! {
        biased;
        _ = turn.stop.cancelled() => {
            turn.cancel.cancel();
            dispatch.await?
        }
        result = &mut dispatch => result?,
    };
    let result = convert_output(&output.item);
    session
        .record_conversation_items(
            &turn.step.turn,
            &turn.step.settings.model_info,
            &[output.item],
        )
        .await;
    result
}

fn convert_output(item: &ResponseItem) -> anyhow::Result<Value> {
    let output = match item {
        ResponseItem::FunctionCallOutput { output, .. }
        | ResponseItem::CustomToolCallOutput { output, .. } => output,
        _ => bail!("Unsupported ASM tool result"),
    };
    let content = match &output.body {
        FunctionCallOutputBody::Text(text) => vec![json!({"type":"text","text":text})],
        FunctionCallOutputBody::ContentItems(items) => items
            .iter()
            .map(|item| match item {
                FunctionCallOutputContentItem::InputText { text } => {
                    Ok(json!({"type":"text","text":text}))
                }
                FunctionCallOutputContentItem::InputImage {
                    image: ImageReference::Inline { image_url },
                    ..
                } => {
                    let (mime, data) = image_url
                        .strip_prefix("data:")
                        .and_then(|url| url.split_once(";base64,"))
                        .context("Claude MCP bridge requires inline base64 images")?;
                    Ok(json!({"type":"image","mimeType":mime,"data":data}))
                }
                FunctionCallOutputContentItem::InputImage {
                    image: ImageReference::File { .. },
                    ..
                }
                | FunctionCallOutputContentItem::InputAudio { .. }
                | FunctionCallOutputContentItem::EncryptedContent { .. } => bail!(
                    "Unsupported tool media in Claude MCP bridge; output retained in ASM history"
                ),
            })
            .collect::<anyhow::Result<Vec<_>>>()?,
    };
    Ok(json!({"isError":output.success == Some(false), "content":content}))
}
