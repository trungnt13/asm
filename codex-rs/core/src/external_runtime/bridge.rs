//! Session-local JSON transport for canonical ASM tools and native child communication.

use std::sync::Arc;
use std::sync::Weak;
use std::time::Duration;

use anyhow::Context;
use anyhow::bail;
use axum::Json;
use axum::Router;
use axum::body::HttpBody;
use axum::body::to_bytes;
use axum::extract::Request;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::post;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ImageReference;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::NativeRuntime;
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

use super::active_handoff::CursorHandoffInvocation;
use super::active_handoff::HandoffInvocation;
use crate::session::session::Session;
use crate::session::step_context::StepContext;
use crate::tools::context::ToolPayload;
use crate::tools::parallel::ToolCallRuntime;
use crate::tools::router::ToolCall;
use crate::turn_diff_tracker::TurnDiffTracker;

const MAX_REQUEST_BYTES: usize = 1024 * 1024;
// Claude can echo text in both message content and structured tool output.
// Leave room for both copies and their envelope below the 1 MiB frame limit.
const MAX_CLAUDE_TEXT_RESULT_BYTES: u64 = 480 * 1024;
// Reserve native metadata space below the 32 MiB image frame limit.
const MAX_IMAGE_RESULT_BYTES: usize = 31 * 1024 * 1024;
/// Concurrent HTTP requests admitted; their tool calls run one at a time per turn.
const ADMITTED_REQUESTS: u32 = 8;
/// Allowance beyond queued canonical waits for dispatch and result conversion.
const TOOL_TIMEOUT_MARGIN: Duration = Duration::from_secs(/*secs*/ 30);

pub(super) struct ExternalMessageBridge {
    state: Arc<BridgeState>,
    stop: CancellationToken,
    server: Option<JoinHandle<std::io::Result<()>>>,
    directory: tempfile::TempDir,
}

struct BridgeState {
    runtime: NativeRuntime,
    ready: tokio::sync::watch::Sender<Option<bool>>,
    authority: String,
    token: String,
    turn: Mutex<Option<Arc<BridgeTurn>>>,
    requests: Semaphore,
    stop: CancellationToken,
    message_tools: Mutex<Option<Vec<MessageTool>>>,
}

struct BridgeTurn {
    session: Weak<Session>,
    stop: CancellationToken,
    step: Arc<StepContext>,
    cancel: CancellationToken,
    execution: Arc<Semaphore>,
    tracker: crate::tools::context::SharedTurnDiffTracker,
    worker: Option<Mutex<crate::tools::code_mode::CodeModeDispatchWorker>>,
}

#[derive(Clone, PartialEq)]
struct MessageTool {
    name: ToolName,
    spec: Value,
    custom: bool,
}

impl ExternalMessageBridge {
    #[tracing::instrument(level = "trace", skip_all, err)]
    pub(super) async fn start(max_wait: Duration, runtime: NativeRuntime) -> anyhow::Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let authority = listener.local_addr()?.to_string();
        let stop = CancellationToken::new();
        let mut directory_builder = tempfile::Builder::new();
        directory_builder.prefix("asm-native-tools-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            directory_builder.permissions(std::fs::Permissions::from_mode(0o700));
        }
        let directory = directory_builder.tempdir()?;
        let state = Arc::new(BridgeState {
            runtime,
            ready: tokio::sync::watch::channel(/*init*/ None).0,
            authority,
            token: Uuid::new_v4().to_string(),
            turn: Mutex::new(/*t*/ None),
            requests: Semaphore::new(ADMITTED_REQUESTS as usize),
            stop: stop.clone(),
            message_tools: Mutex::new(/*t*/ None),
        });
        let app = Router::new()
            .route("/attest", post(serve_json))
            .route("/tools", post(serve_json))
            .route("/call", post(serve_json))
            .with_state(Arc::clone(&state));
        let descriptor = json!({"url":format!("http://{}",state.authority), "token":state.token,
            "unboundedCalls":true,
            "timeoutMs":u64::try_from(max_wait.saturating_mul(ADMITTED_REQUESTS).saturating_add(TOOL_TIMEOUT_MARGIN).as_millis()).unwrap_or(u64::MAX)});
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new();
        file.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            file.mode(0o600);
        }
        file.open(directory.path().join("descriptor.json"))?
            .write_all(&serde_json::to_vec(&descriptor)?)?;
        if runtime == NativeRuntime::ClaudeCode {
            std::fs::create_dir(directory.path().join(".claude-plugin"))?;
            std::fs::create_dir(directory.path().join("hooks"))?;
            std::fs::write(
                directory.path().join(".claude-plugin/plugin.json"),
                r#"{"name":"asm","version":"1.0.0"}"#,
            )?;
            std::fs::write(
                directory.path().join("hooks/hooks.json"),
                r#"{"modules":["./register.js"]}"#,
            )?;
            std::fs::write(
                directory.path().join("hooks/register.js"),
                include_str!("../../assets/native-tools/register.js"),
            )?;
        }
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
            directory,
        })
    }

    #[tracing::instrument(level = "trace", skip_all, err)]
    pub(super) async fn wait_ready(&self) -> anyhow::Result<()> {
        let mut ready = self.state.ready.subscribe();
        let valid = tokio::time::timeout(
            Duration::from_secs(/*secs*/ 10),
            ready.wait_for(Option::is_some),
        )
        .await
        .context("Claude startup did not attest its effective ASM tool inventory")?
        .context("Claude startup inventory channel closed")?
        .unwrap_or(/*default*/ false);
        anyhow::ensure!(valid, "Claude startup exposed unexpected tools or commands");
        Ok(())
    }

    pub(super) fn directory(&self) -> &std::path::Path {
        self.directory.path()
    }

    #[tracing::instrument(level = "trace", skip_all, err)]
    pub(super) async fn set_turn(
        &self,
        session: Arc<Session>,
        step: Arc<StepContext>,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        if self.stop.is_cancelled() {
            bail!("Native messaging bridge is stopped");
        }
        let mut message_tools = bridge_tools(step.tool_router.model_visible_specs().as_ref())?;
        message_tools.retain(|tool| {
            matches!(
                tool.name.name.as_str(),
                codex_code_mode::PUBLIC_TOOL_NAME | codex_code_mode::WAIT_TOOL_NAME
            )
        });
        let direct_tool_description = match self.state.runtime {
            NativeRuntime::ClaudeCode => {
                "Call with {} (omit name) to discover CURRENT direct-tool schemas and Code Mode exec definitions. ALL_TOOLS and tool_search cover only tools nested inside Code Mode; direct-only tools, including configured agent tools, are absent there. Discover here before declaring a tool unavailable. Call a listed direct tool using its exact name and arguments; other tools use exec/wait."
            }
            NativeRuntime::CursorAgent => {
                "Compatibility for ASM direct-only tools not exposed by Code Mode. Use exec for workspace tasks. Only when a task needs a direct-only tool, call with {} to list its schemas, or pass name and arguments to invoke it."
            }
        };
        message_tools.push(MessageTool {
                name: ToolName::plain("direct_tool"), custom: false,
                spec: json!({"name":"direct_tool","description":direct_tool_description,"inputSchema":{"type":"object","properties":{"name":{"type":"string","description":"Omit to discover the current catalog; otherwise use an exact listed direct-tool name."},"arguments":{"type":"object","description":"Arguments matching the selected direct tool schema."}},"additionalProperties":false}}),
            });
        {
            let mut original = self.state.message_tools.lock().await;
            if original.as_ref().is_some_and(|original| {
                original
                    .iter()
                    .map(|tool| (&tool.name, tool.custom))
                    .ne(message_tools.iter().map(|tool| (&tool.name, tool.custom)))
            }) {
                bail!("Native messaging definitions changed; start a new thread");
            }
            *original = Some(message_tools);
        }
        let mut turn = self.state.turn.lock().await;
        if turn.is_some() {
            bail!("Native messaging bridge already has an active turn");
        }
        let tracker = Arc::new(Mutex::new(TurnDiffTracker::new()));
        let worker = session.services.code_mode_service.start_turn_worker(
            &session,
            Arc::clone(&step),
            Arc::clone(&tracker),
        );
        *turn = Some(Arc::new(BridgeTurn {
            session: Arc::downgrade(&session),
            stop: self.stop.clone(),
            step,
            cancel: cancel.child_token(),
            execution: Arc::new(Semaphore::new(/*permits*/ 1)),
            tracker,
            worker: worker.map(Mutex::new),
        }));
        Ok(())
    }

    #[tracing::instrument(level = "trace", skip_all, err)]
    pub(super) async fn clear_turn(&self) -> anyhow::Result<()> {
        let turn = self.state.turn.lock().await.take();
        if let Some(turn) = turn {
            turn.cancel.cancel();
            // Yielded cells outlive exec: revoke them and drain callbacks before a clean checkpoint.
            let cleanup = async {
                let _permit = turn.execution.acquire().await?;
                if let Some(worker) = &turn.worker {
                    turn.session
                        .upgrade()
                        .context("ASM thread stopped during cleanup")?
                        .services
                        .code_mode_service
                        .drain_active_cells()
                        .await
                        .map_err(anyhow::Error::msg)?;
                    worker
                        .lock()
                        .await
                        .drain()
                        .await
                        .map_err(anyhow::Error::msg)?;
                }
                Ok::<_, anyhow::Error>(())
            };
            match tokio::time::timeout(Duration::from_secs(/*secs*/ 5), cleanup).await {
                Ok(Ok(())) => {
                    if let Some(session) = turn.session.upgrade() {
                        session
                            .external_runtime
                            .queued_input_count
                            .store(/*val*/ 0, std::sync::atomic::Ordering::SeqCst);
                    }
                }
                result => {
                    self.stop.cancel();
                    return result.context(
                        "Native tool cleanup exceeded 5 seconds; bridge cannot be reused",
                    )?;
                }
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
                Ok(result) => result.context("Native messaging HTTP task failed")??,
                Err(_) => {
                    server.abort();
                    bail!("Native messaging HTTP shutdown exceeded 5 seconds");
                }
            }
        }
        drained
    }
}

impl Drop for ExternalMessageBridge {
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
async fn serve_json(State(state): State<Arc<BridgeState>>, request: Request) -> Response {
    let Ok(_request_slot) = state.requests.try_acquire() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    if state.stop.is_cancelled() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let request_path = request.uri().path().to_owned();
    let headers = request.headers();
    if headers.get("host").and_then(|v| v.to_str().ok()) != Some(state.authority.as_str())
        || headers.get("authorization").and_then(|v| v.to_str().ok())
            != Some(format!("Bearer {}", state.token).as_str())
        || headers.contains_key("origin")
    {
        return StatusCode::FORBIDDEN.into_response();
    }
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
    let path = request_path;
    if path == "/tools" {
        let tools = state
            .message_tools
            .lock()
            .await
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|tool| tool.spec.clone())
            .collect::<Vec<_>>();
        return Json(json!({"tools":tools})).into_response();
    }
    let Ok(params) = serde_json::from_slice::<Value>(&bytes) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if path == "/attest" {
        let expected = state.message_tools.lock().await;
        let tools = params["tools"].as_array();
        let commands_empty = params["commands"].as_array().is_some_and(Vec::is_empty);
        let valid = commands_empty
            && tools.is_some_and(|tools| {
                expected.as_ref().is_some_and(|expected| {
                    tools.len() == expected.len()
                        && expected.iter().all(|tool| {
                            tools.iter().any(|entry| {
                                entry["name"].as_str()
                                    == tool.spec["name"]
                                        .as_str()
                                        .map(|name| format!("mcp__asm__{name}"))
                                        .as_deref()
                            })
                        })
                })
            });
        state.ready.send_replace(Some(valid));
        return Json(json!({"isError":!valid,"ready":valid})).into_response();
    }
    let Some(turn) = state.turn.lock().await.clone() else {
        return (StatusCode::CONFLICT, "No active ASM turn").into_response();
    };
    if turn.cancel.is_cancelled() {
        return (StatusCode::CONFLICT, "ASM turn cancelled").into_response();
    }
    // Calls survive client disconnect until ASM cancels or drains the active turn.
    match tokio::spawn(invoke_message_tool(turn, params, state.runtime))
        .await
        .unwrap_or_else(|error| Err(error.into()))
    {
        Ok(response) => response,
        Err(error) => {
            let result =
                json!({"isError":true,"content":[{"type":"text","text":error.to_string()}]});
            bounded_tool_response(&result).unwrap_or_else(|error| {
                Json(json!({"isError":true,"content":[{"type":"text","text":error.to_string()}]}))
                    .into_response()
            })
        }
    }
}

fn bounded_tool_response(result: &Value) -> anyhow::Result<Response> {
    let has_image = result["content"]
        .as_array()
        .is_some_and(|content| content.iter().any(|item| item["type"] == "image"));
    let limit = if has_image {
        MAX_IMAGE_RESULT_BYTES as u64
    } else {
        MAX_CLAUDE_TEXT_RESULT_BYTES
    };
    let response = Json(result).into_response();
    anyhow::ensure!(
        response
            .body()
            .size_hint()
            .upper()
            .is_some_and(|size| size <= limit),
        "Native tool result exceeds the transport budget"
    );
    Ok(response)
}

fn bridge_tools(specs: &[ToolSpec]) -> anyhow::Result<Vec<MessageTool>> {
    let mut tools = Vec::new();
    for spec in specs {
        let entries = match spec {
            ToolSpec::Namespace(namespace) => namespace
                .tools
                .iter()
                .map(|tool| {
                    (
                        Some(namespace.name.clone()),
                        match tool {
                            ResponsesApiNamespaceTool::Function(tool) => {
                                ToolSpec::Function(tool.clone())
                            }
                            ResponsesApiNamespaceTool::Custom(tool) => {
                                ToolSpec::Freeform(tool.clone())
                            }
                        },
                    )
                })
                .collect(),
            spec => vec![(None, spec.clone())],
        };
        for (namespace, spec) in entries {
            let (name, description, parameters, custom) = match spec {
                ToolSpec::Function(tool) => (
                    tool.name,
                    tool.description,
                    serde_json::to_value(tool.parameters)?,
                    false,
                ),
                ToolSpec::Freeform(tool) => (
                    tool.name,
                    tool.description,
                    json!({"type":"object","properties":{"input":{"type":"string"}},"required":["input"],"additionalProperties":false}),
                    true,
                ),
                _ => continue,
            };
            let wire_name = namespace
                .as_ref()
                .map_or_else(|| name.clone(), |namespace| format!("{namespace}__{name}"));
            tools.push(MessageTool {
                name: ToolName::new(namespace, name),
                custom,
                spec: json!({"name":wire_name,"description":description,"inputSchema":parameters}),
            });
        }
    }
    anyhow::ensure!(
        tools
            .iter()
            .any(|tool| tool.name.name == codex_code_mode::PUBLIC_TOOL_NAME),
        "Native runtimes require ASM Code Mode tools"
    );
    Ok(tools)
}

#[tracing::instrument(level = "trace", skip_all, err)]
async fn invoke_message_tool(
    turn: Arc<BridgeTurn>,
    mut params: Value,
    native_runtime: NativeRuntime,
) -> anyhow::Result<Response> {
    let _execution = tokio::select! {
        permit = turn.execution.acquire() => permit?,
        _ = turn.cancel.cancelled() => bail!("ASM turn cancelled"),
        _ = turn.stop.cancelled() => bail!("ASM bridge stopped"),
    };
    if turn.cancel.is_cancelled() || turn.stop.is_cancelled() {
        bail!("ASM turn cancelled");
    }
    let session = turn.session.upgrade().context("ASM thread stopped")?;
    let step = session
        .capture_step_context(Arc::clone(&turn.step.turn), &turn.cancel)
        .await?;
    let tools = bridge_tools(step.tool_router.model_visible_specs().as_ref())?;
    let mut name = params
        .get("name")
        .and_then(Value::as_str)
        .context("Missing tool name")?
        .to_owned();
    let handoff_lease = if native_runtime == NativeRuntime::ClaudeCode
        && let Some(tool_use_id) = params["nativeToolUseId"]
            .as_str()
            .filter(|id| !id.is_empty())
    {
        let active = turn.cancel.child_token();
        let lease = active.clone().drop_guard();
        step.extension_data.insert(HandoffInvocation {
            tool_use_id: tool_use_id.to_owned(),
            tool_name: format!("mcp__asm__{name}"),
            active,
        });
        Some(lease)
    } else if native_runtime == NativeRuntime::CursorAgent
        && let Some(tool_use_id) = params["nativeToolUseId"]
            .as_str()
            .filter(|id| !id.is_empty())
    {
        let active = turn.cancel.child_token();
        let lease = active.clone().drop_guard();
        step.extension_data.insert(CursorHandoffInvocation {
            tool_use_id: tool_use_id.to_owned(),
            active,
        });
        Some(lease)
    } else {
        None
    };
    if name == "direct_tool" {
        let direct = tools.iter().filter(|tool| {
            !matches!(
                tool.name.name.as_str(),
                codex_code_mode::PUBLIC_TOOL_NAME | codex_code_mode::WAIT_TOOL_NAME
            )
        });
        let arguments = &params["arguments"];
        let local_result = match arguments["name"].as_str() {
            None => {
                let catalog = json!({
                    "execution": {
                        "model": step.settings.model_info.slug,
                        "reasoning_effort": step.settings.effective_reasoning_effort(),
                        "spawn_guidance": if step.turn.config.multi_agent_v2.requires_spawn_agent_model() {
                            "For spawn_agent, pass the canonical ASM slug for the chosen model. Use your current model unless instructed otherwise."
                        } else {
                            "Omit model and reasoning_effort to inherit the current execution settings. When specifying model, use its canonical ASM slug, not a backend-only alias."
                        }
                    },
                    "tools": match native_runtime {
                        NativeRuntime::ClaudeCode => tools.iter().map(|tool| &tool.spec).collect::<Vec<_>>(),
                        NativeRuntime::CursorAgent => direct.map(|tool| &tool.spec).collect::<Vec<_>>(),
                    }
                });
                Some((serde_json::to_string(&catalog)?, true))
            }
            Some(selected) if direct.into_iter().any(|tool| tool.spec["name"] == selected) => {
                name = selected.to_owned();
                params = json!({"arguments":arguments.get("arguments").cloned().unwrap_or_else(|| json!({}))});
                None
            }
            Some(_) => Some(("No tool was dispatched: this name is not a current direct tool. Call direct_tool with {} to discover current direct-tool names and schemas. ALL_TOOLS and tool_search cover only tools nested inside Code Mode exec.".to_owned(), false)),
        };
        if let Some((mut text, mut success)) = local_result {
            let to_response = |text: &str, success: bool| {
                bounded_tool_response(
                    &json!({"isError":!success,"notDispatched":!success,"content":[{"type":"text","text":text}]}),
                )
            };
            let response = match to_response(&text, success) {
                Ok(response) => response,
                Err(error) => {
                    text = format!("No tool was dispatched: {error}");
                    success = false;
                    to_response(&text, success)?
                }
            };
            let call_id = format!(
                "{}-{}",
                if native_runtime == NativeRuntime::ClaudeCode {
                    "claude"
                } else {
                    "cursor"
                },
                Uuid::new_v4()
            );
            let items = [
                ResponseItem::FunctionCall {
                    status: None,
                    encrypted_content: None,
                    id: None,
                    name,
                    namespace: None,
                    arguments: params["arguments"].to_string(),
                    encrypted_function_args: None,
                    call_id: call_id.clone(),
                    internal_chat_message_metadata_passthrough: None,
                },
                ResponseItem::FunctionCallOutput {
                    id: None,
                    call_id: Some(call_id),
                    name: None,
                    namespace: None,
                    output: FunctionCallOutputPayload {
                        body: FunctionCallOutputBody::Text(text),
                        success: Some(success),
                    },
                    internal_chat_message_metadata_passthrough: None,
                },
            ];
            session
                .record_conversation_items(&step.turn, &step.settings.model_info, &items)
                .await;
            return Ok(response);
        }
    }
    let tool = tools
        .iter()
        .find(|tool| match &tool.name.namespace {
            Some(namespace) => name == format!("{namespace}__{}", tool.name.name),
            None => name == tool.name.name,
        })
        .context("Only exposed ASM tools are allowed")?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if !arguments.is_object() {
        bail!("Tool arguments must be an object");
    }
    let call_id = format!(
        "{}-{}",
        if native_runtime == NativeRuntime::ClaudeCode {
            "claude"
        } else {
            "cursor"
        },
        Uuid::new_v4()
    );
    let arguments = arguments.to_string();
    let custom = tool
        .custom
        .then(|| {
            params["arguments"]["input"]
                .as_str()
                .context("Freeform tool requires an input string")
        })
        .transpose()?;
    let payload = match custom {
        Some(input) => ToolPayload::Custom {
            input: input.to_owned(),
        },
        None => ToolPayload::Function {
            arguments: arguments.clone(),
        },
    };
    let item = if let Some(input) = custom {
        ResponseItem::CustomToolCall {
            encrypted_content: None,
            status: None,
            id: None,
            name: tool.name.name.clone(),
            namespace: tool.name.namespace.clone(),
            input: input.to_owned(),
            call_id: call_id.clone(),
            internal_chat_message_metadata_passthrough: None,
        }
    } else {
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
        }
    };
    session
        .record_conversation_items(
            &step.turn,
            &step.settings.model_info,
            std::slice::from_ref(&item),
        )
        .await;
    let call = ToolCall {
        tool_name: tool.name.clone(),
        call_id: call_id.into(),
        payload,
        encrypted_function_args: None,
        native_call: Some(Arc::new(item)),
    };
    let runtime = ToolCallRuntime::new(
        Arc::clone(&session),
        Arc::clone(&step),
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
    drop(handoff_lease);
    let mut item = Arc::unwrap_or_clone(output.item);
    let result = convert_output(&item);
    let (mut body, response) = match result.and_then(|body| {
        let response = bounded_tool_response(&body)?;
        Ok((body, response))
    }) {
        Ok(pair) => pair,
        Err(error) => {
            let payload = match &mut item {
                ResponseItem::FunctionCallOutput { output, .. }
                | ResponseItem::CustomToolCallOutput { output, .. } => output,
                _ => return Err(error),
            };
            let execution = match payload.success {
                Some(true) => "ASM tool succeeded",
                Some(false) => "ASM tool failed",
                None => "ASM tool returned",
            };
            let text = format!(
                "{execution}, but its output was not delivered: {error}. Do not rerun the tool just to recover the output."
            );
            payload.body = FunctionCallOutputBody::Text(text.clone());
            let body = json!({
                "isError": payload.success == Some(false),
                "content": [{"type":"text","text":text}]
            });
            let response = bounded_tool_response(&body)?;
            (body, response)
        }
    };
    session
        .record_conversation_items(&step.turn, &step.settings.model_info, &[item])
        .await;
    if let Some(mailbox) = step.turn.extension_data.get::<super::CursorMailbox>()
        && mailbox.take_wake()
        && !turn.cancel.is_cancelled()
    {
        mailbox.append(&session, &step.turn, &mut body).await?;
        return bounded_tool_response(&body);
    }
    Ok(response)
}

fn convert_output(item: &ResponseItem) -> anyhow::Result<Value> {
    let output = match item {
        ResponseItem::FunctionCallOutput { output, .. }
        | ResponseItem::CustomToolCallOutput { output, .. } => output,
        _ => bail!("Unsupported native tool result"),
    };
    let encoded_bytes = match &output.body {
        FunctionCallOutputBody::Text(text) => text.len(),
        FunctionCallOutputBody::ContentItems(items) => items
            .iter()
            .map(|item| match item {
                FunctionCallOutputContentItem::InputText { text } => text.len(),
                FunctionCallOutputContentItem::InputImage {
                    image: ImageReference::Inline { image_url },
                    ..
                } => image_url.len(),
                _ => 0,
            })
            .sum(),
    };
    anyhow::ensure!(
        encoded_bytes <= MAX_IMAGE_RESULT_BYTES,
        "Native tool result exceeds the transport budget"
    );
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
                    let normalized = codex_utils_image::load_data_url_for_prompt(
                        image_url,
                        codex_utils_image::PromptImageMode::Original,
                        codex_utils_image::PromptImageDecoder::Legacy,
                    )?
                    .into_data_url();
                    let (metadata, data) = normalized
                        .split_once(',')
                        .context("Native result requires inline image data")?;
                    let mime_type = metadata
                        .strip_prefix("data:")
                        .and_then(|value| value.strip_suffix(";base64"))
                        .context("Native result requires base64 image data")?;
                    Ok(json!({"type":"image", "mimeType":mime_type, "data":data}))
                }
                _ => bail!("Native tool returned unsupported media"),
            })
            .collect::<anyhow::Result<Vec<_>>>()?,
    };
    Ok(json!({"isError":output.success == Some(false), "content":content}))
}
