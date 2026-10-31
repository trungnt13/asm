//! Child-only local JSON bridge to the canonical ASM messaging and message-board tools.

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
use codex_message_board::MessageBoardTool;
use codex_prompts::ResolvedModelMessages;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ResponseItem;
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
    tools: Vec<ToolName>,
    execution: Arc<Semaphore>,
    tracker: crate::tools::context::SharedTurnDiffTracker,
    mail_delivery: tokio::sync::mpsc::Sender<tokio::sync::oneshot::Sender<Vec<String>>>,
}

#[derive(PartialEq)]
struct MessageTool {
    name: ToolName,
    spec: Value,
}

impl ExternalMessageBridge {
    #[tracing::instrument(level = "trace", skip_all, err)]
    pub(super) async fn start(max_wait: Duration) -> anyhow::Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let authority = listener.local_addr()?.to_string();
        let stop = CancellationToken::new();
        let (mail_tx, mail_delivery) = tokio::sync::mpsc::channel(/*buffer*/ 1);
        let state = Arc::new(BridgeState {
            mail_delivery: mail_tx,
            authority,
            token: Uuid::new_v4().to_string(),
            turn: Mutex::new(/*t*/ None),
            requests: Semaphore::new(ADMITTED_REQUESTS as usize),
            stop: stop.clone(),
            message_tools: Mutex::new(/*t*/ None),
        });
        let app = Router::new()
            .route("/tools", post(serve_json))
            .route("/call", post(serve_json))
            .with_state(Arc::clone(&state));
        let directory = tempfile::Builder::new()
            .prefix("asm-native-tools-")
            .tempdir()?;
        let descriptor = json!({"url":format!("http://{}",state.authority), "token":state.token,
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
        if step.turn.parent_thread_id.is_none()
            || !step.turn.session_source.is_non_root_agent()
            || step.turn.multi_agent_version != MultiAgentVersion::V2
        {
            bail!("Native messaging is available only to native V2 child agents");
        }
        let (router, message_tools) = build_message_tools(&session, &step)?;
        // The bridge owns a fresh captured step. Inference metadata must not hide
        // caller-authorized messaging on a native child with no native model catalog.
        Arc::get_mut(&mut step)
            .context("Native messaging requires an exclusively owned captured step")?
            .tool_router = Arc::new(router);
        let tools = message_tools.iter().map(|tool| tool.name.clone()).collect();
        {
            let mut original = self.state.message_tools.lock().await;
            if original
                .as_ref()
                .is_some_and(|original| *original != message_tools)
            {
                bail!("Native messaging definitions changed; start a new thread");
            }
            *original = Some(message_tools);
        }
        let mut turn = self.state.turn.lock().await;
        if turn.is_some() {
            bail!("Native messaging bridge already has an active turn");
        }
        *turn = Some(Arc::new(BridgeTurn {
            session: Arc::downgrade(&session),
            mail_delivery: self.state.mail_delivery.clone(),
            stop: self.stop.clone(),
            step,
            cancel: cancel.child_token(),
            tools,
            execution: Arc::new(Semaphore::new(/*permits*/ 1)),
            tracker: Arc::new(Mutex::new(TurnDiffTracker::new())),
        }));
        Ok(())
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
                bail!("Native messaging cleanup exceeded 5 seconds; bridge cannot be reused");
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
    let Some(turn) = state.turn.lock().await.clone() else {
        return (StatusCode::CONFLICT, "No active ASM turn").into_response();
    };
    if turn.cancel.is_cancelled() {
        return (StatusCode::CONFLICT, "ASM turn cancelled").into_response();
    }
    // Calls survive client disconnect until ASM cancels or drains the active turn.
    let result = tokio::spawn(invoke_message_tool(turn, params))
        .await
        .unwrap_or_else(|error| Err(error.into()));
    match result {
        Ok(result) => Json(result).into_response(),
        Err(error) => {
            Json(json!({"isError":true,"content":[{"type":"text","text":error.to_string()}]}))
                .into_response()
        }
    }
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
            spec: json!({"name":wire_name,"description":tool.description,"inputSchema":tool.parameters}),
        });
        specs.push(spec);
    }
    let router = ToolRouter::from_parts(
        registry,
        specs,
        ToolMode::Direct,
        Default::default(),
        None,
        &[],
    );
    Ok((router, tools))
}

#[tracing::instrument(level = "trace", skip_all, err)]
async fn invoke_message_tool(turn: Arc<BridgeTurn>, params: Value) -> anyhow::Result<Value> {
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
        .iter()
        .find(|tool| match &tool.namespace {
            Some(namespace) => name == format!("{namespace}__{}", tool.name),
            None => name == tool.name,
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
    let payload = ToolPayload::Function {
        arguments: arguments.clone(),
    };
    let item = ResponseItem::FunctionCall {
        encrypted_content: None,
        status: None,
        id: None,
        name: tool.name.clone(),
        namespace: tool.namespace.clone(),
        arguments,
        encrypted_function_args: None,
        call_id: call_id.clone(),
        internal_chat_message_metadata_passthrough: None,
    };
    session
        .record_conversation_items(
            &turn.step.turn,
            &turn.step.settings.model_info,
            std::slice::from_ref(&item),
        )
        .await;
    let call = ToolCall {
        tool_name: tool.clone(),
        call_id,
        payload,
        encrypted_function_args: None,
        native_call: Some(Arc::new(item)),
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
    let mut result = convert_output(&output.item)?;
    session
        .record_conversation_items(
            &turn.step.turn,
            &turn.step.settings.model_info,
            &[output.item],
        )
        .await;
    if tool.name == "wait_agent"
        && turn
            .step
            .settings
            .model_info
            .slug
            .starts_with("cursor-agent/")
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
    Ok(result)
}

fn convert_output(item: &ResponseItem) -> anyhow::Result<Value> {
    let ResponseItem::FunctionCallOutput { output, .. } = item else {
        bail!("Unsupported native messaging result");
    };
    let content = match &output.body {
        FunctionCallOutputBody::Text(text) => vec![json!({"type":"text","text":text})],
        FunctionCallOutputBody::ContentItems(items) => items
            .iter()
            .map(|item| match item {
                FunctionCallOutputContentItem::InputText { text } => {
                    Ok(json!({"type":"text","text":text}))
                }
                _ => bail!("Native messaging returned non-text content"),
            })
            .collect::<anyhow::Result<Vec<_>>>()?,
    };
    Ok(json!({"isError":output.success == Some(false), "content":content}))
}
