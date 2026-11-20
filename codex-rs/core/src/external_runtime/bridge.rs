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
use codex_message_board::MessageBoardTool;
use codex_prompts::ResolvedModelMessages;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ImageReference;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::NativeRuntime;
use codex_protocol::openai_models::ToolMode;
use codex_protocol::protocol::MultiAgentVersion;
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

use super::active_handoff::HandoffInvocation;
use crate::session::session::Session;
use crate::session::step_context::StepContext;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::extension_tools::ExtensionToolAdapter;
use crate::tools::parallel::ToolCallRuntime;
use crate::tools::registry::ToolExposure;
use crate::tools::registry::ToolRegistry;
use crate::tools::router::ToolCall;
use crate::tools::router::ToolRouter;
use crate::tools::spec_plan::extension_tool_executors;
use crate::tools::spec_plan::register_multi_agent_v2_communication_tools;
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
    pub(super) mail_delivery:
        tokio::sync::mpsc::Receiver<tokio::sync::oneshot::Sender<Vec<String>>>,
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
    mail_delivery: tokio::sync::mpsc::Sender<tokio::sync::oneshot::Sender<Vec<String>>>,
}

struct BridgeTurn {
    session: Weak<Session>,
    stop: CancellationToken,
    step: Arc<StepContext>,
    cancel: CancellationToken,
    tools: Vec<MessageTool>,
    execution: Arc<Semaphore>,
    tracker: crate::tools::context::SharedTurnDiffTracker,
    worker: Option<Mutex<crate::tools::code_mode::CodeModeDispatchWorker>>,
    mail_delivery: tokio::sync::mpsc::Sender<tokio::sync::oneshot::Sender<Vec<String>>>,
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
        let (mail_tx, mail_delivery) = tokio::sync::mpsc::channel(/*buffer*/ 1);
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
            mail_delivery: mail_tx,
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
            "unboundedCalls":runtime == NativeRuntime::ClaudeCode,
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
            mail_delivery,
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
        mut step: Arc<StepContext>,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        if self.stop.is_cancelled() {
            bail!("Native messaging bridge is stopped");
        }
        let managed = step.settings.model_info.slug.starts_with("claude-code/");
        if !managed
            && (step.turn.parent_thread_id.is_none()
                || !step.turn.session_source.is_non_root_agent()
                || step.turn.multi_agent_version != MultiAgentVersion::V2)
        {
            bail!("Native messaging is available only to native V2 child agents");
        }
        let mut message_tools = if managed {
            bridge_tools(step.tool_router.model_visible_specs().as_ref())?
        } else {
            let (router, tools) = build_message_tools(&session, &step)?;
            Arc::get_mut(&mut step)
                .context("Native messaging requires an exclusively owned captured step")?
                .tool_router = Arc::new(router);
            tools
        };
        let tools = message_tools.clone();
        if managed {
            message_tools.retain(|tool| {
                matches!(
                    tool.name.name.as_str(),
                    codex_code_mode::PUBLIC_TOOL_NAME | codex_code_mode::WAIT_TOOL_NAME
                )
            });
            message_tools.push(MessageTool {
                name: ToolName::plain("direct_tool"), custom: false,
                spec: json!({"name":"direct_tool","description":"Call with {} (omit name) to discover CURRENT direct-tool schemas and Code Mode exec definitions. ALL_TOOLS and tool_search cover only tools nested inside Code Mode; direct-only tools, including configured agent tools, are absent there. Discover here before declaring a tool unavailable. Call a listed direct tool using its exact name and arguments; other tools use exec/wait.","inputSchema":{"type":"object","properties":{"name":{"type":"string","description":"Omit to discover the current catalog; otherwise use an exact listed direct-tool name."},"arguments":{"type":"object","description":"Arguments matching the selected direct tool schema."}},"additionalProperties":false}}),
            });
        }
        {
            let mut original = self.state.message_tools.lock().await;
            if original.as_ref().is_some_and(|original| {
                if managed {
                    original
                        .iter()
                        .map(|tool| (&tool.name, tool.custom))
                        .ne(message_tools.iter().map(|tool| (&tool.name, tool.custom)))
                } else {
                    *original != message_tools
                }
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
            mail_delivery: self.state.mail_delivery.clone(),
            stop: self.stop.clone(),
            step,
            cancel: cancel.child_token(),
            tools,
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
            if state.runtime == NativeRuntime::ClaudeCode {
                bounded_claude_response(result).unwrap_or_else(|error| {
                    Json(json!({"isError":true,"content":[{"type":"text","text":error.to_string()}]}))
                        .into_response()
                })
            } else {
                Json(result).into_response()
            }
        }
    }
}

fn bounded_claude_response(result: Value) -> anyhow::Result<Response> {
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
        "Claude requires ASM Code Mode tools"
    );
    Ok(tools)
}

fn build_message_tools(
    session: &Session,
    step: &StepContext,
) -> anyhow::Result<(ToolRouter, Vec<MessageTool>)> {
    let config = &step.turn.config.multi_agent_v2;
    let mut registry = ToolRegistry::with_tool_policy(Arc::clone(&session.tool_policy));
    // Reuse the canonical collaboration gates, minus spawning. A native child's model
    // metadata must not hide them, so the model-catalog check does not apply here.
    if !config.disable_native_tools {
        register_multi_agent_v2_communication_tools(
            &step.turn,
            ResolvedModelMessages::from_model(&step.settings.model_info),
            ToolExposure::Direct,
            &mut registry,
        );
    }
    // The board extension binds its tools to this caller at thread start, when the board is
    // enabled for it. Expose only its discussion tools, never other extension tools.
    let namespace = config.tool_namespace.as_deref();
    let board_tools = MessageBoardTool::DISCUSSION
        .map(|tool| ToolName::new(namespace.map(str::to_owned), tool.name()));
    for executor in extension_tool_executors(session, &step.extension_data) {
        if board_tools.contains(&executor.tool_name()) {
            registry.register_external(Arc::new(ExtensionToolAdapter::new(executor)));
        }
    }
    if let Some(name) = registry.first_collision() {
        bail!("Messaging tool {name} is defined twice; ASM cannot tell which to expose");
    }
    let mut tools = Vec::new();
    let mut specs = Vec::new();
    for registered in registry.entries() {
        let name = registered.runtime.tool_name();
        let spec = registered.spec().as_ref().clone();
        let tool = match spec.clone() {
            ToolSpec::Function(tool) => tool,
            ToolSpec::Namespace(mut namespace) if namespace.tools.len() == 1 => {
                let Some(ResponsesApiNamespaceTool::Function(tool)) = namespace.tools.pop() else {
                    bail!("Messaging tool must use function arguments");
                };
                tool
            }
            _ => bail!("Messaging tool must use function arguments"),
        };
        let wire_name = match &name.namespace {
            Some(namespace) => format!("{namespace}__{}", name.name),
            None => name.name.clone(),
        };
        if wire_name.len() > 64
            || !wire_name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            bail!("Messaging namespace is not tool-name compatible");
        }
        tools.push(MessageTool {
            name,
            custom: false,
            spec: json!({"name":wire_name,"description":tool.description,"inputSchema":tool.parameters}),
        });
        specs.push(spec);
    }
    let router = ToolRouter::from_parts(
        registry,
        specs,
        ToolMode::Direct,
        Default::default(),
        /*exec_definitions*/ None,
        /*tool_namespaces_info*/ None,
        &[],
    );
    Ok((router, tools))
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
    let managed = turn
        .step
        .settings
        .model_info
        .slug
        .starts_with("claude-code/");
    let step = if managed {
        session
            .capture_step_context(Arc::clone(&turn.step.turn), &turn.cancel)
            .await?
    } else {
        Arc::clone(&turn.step)
    };
    let tools = if managed {
        bridge_tools(step.tool_router.model_visible_specs().as_ref())?
    } else {
        turn.tools.clone()
    };
    let mut name = params
        .get("name")
        .and_then(Value::as_str)
        .context("Missing tool name")?
        .to_owned();
    let handoff_lease = if managed
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
    } else {
        None
    };
    if name == "direct_tool" && step.settings.model_info.slug.starts_with("claude-code/") {
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
                        "spawn_guidance": "Omit model and reasoning_effort to inherit the current execution settings. When specifying model, use its canonical ASM slug, not a backend-only alias."
                    },
                    "tools": tools.iter().map(|tool| &tool.spec).collect::<Vec<_>>()
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
                bounded_claude_response(
                    json!({"isError":!success,"notDispatched":!success,"content":[{"type":"text","text":text}]}),
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
            let call_id = format!("claude-{}", Uuid::new_v4());
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
        .context("Only exposed messaging tools are allowed")?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if !arguments.is_object() {
        bail!("Tool arguments must be an object");
    }
    let call_id = format!("claude-{}", Uuid::new_v4());
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
    if native_runtime == NativeRuntime::ClaudeCode {
        let response = match result.and_then(bounded_claude_response) {
            Ok(response) => response,
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
                bounded_claude_response(json!({
                    "isError": payload.success == Some(false),
                    "content": [{"type":"text","text":text}]
                }))?
            }
        };
        session
            .record_conversation_items(&step.turn, &step.settings.model_info, &[item])
            .await;
        return Ok(response);
    }
    session
        .record_conversation_items(&step.turn, &step.settings.model_info, &[item])
        .await;
    let mut result = result?;
    anyhow::ensure!(
        serde_json::to_vec(&result)?.len() <= MAX_IMAGE_RESULT_BYTES,
        "Native tool result exceeds the image transport budget"
    );
    if tool.name.name == "wait_agent"
        && step.settings.model_info.slug.starts_with("cursor-agent/")
        && result["isError"] == false
    {
        let (reply, delivered) = tokio::sync::oneshot::channel();
        let delivery = async {
            turn.mail_delivery
                .send(reply)
                .await
                .context("Cursor mailbox delivery stopped")?;
            delivered.await.context("Cursor mailbox delivery stopped")
        };
        let messages = tokio::select! {
            biased;
            _ = turn.cancel.cancelled() => bail!("ASM turn cancelled"),
            messages = delivery => messages?,
        };
        for text in messages {
            result["content"]
                .as_array_mut()
                .context("Missing tool result content")?
                .push(json!({"type":"text","text":text}));
        }
    }
    Ok(Json(result).into_response())
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
