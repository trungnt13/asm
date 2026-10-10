use super::*;
use crate::session::input_queue::UserInputMetadata;
use crate::session::tests::HeldStepTask;
use crate::session::tests::make_session_and_context_with_auth_and_config_and_rx;
use crate::state::TaskKind;
use codex_features::Feature;
use codex_http_client::HttpClient;
use codex_login::CodexAuth;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseItem;
use codex_protocol::user_input::UserInput;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use test_case::test_case;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use tokio::time::timeout;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls;
use tokio_util::task::AbortOnDropHandle;

struct CapturedDecisionRequest {
    headers: String,
    body: Value,
    reply: oneshot::Sender<(u16, Value)>,
}

struct DecisionProxy {
    client: Arc<DecisionsClient>,
    requests: mpsc::UnboundedReceiver<CapturedDecisionRequest>,
    _server: AbortOnDropHandle<()>,
}

impl DecisionProxy {
    #[tracing::instrument(skip_all)]
    async fn start() -> Self {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let certificate = rcgen::generate_simple_self_signed(vec!["api.openai.com".to_string()])
            .expect("generate test certificate");
        let tls = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![certificate.cert.der().clone()],
                rustls::pki_types::PrivatePkcs8KeyDer::from(
                    certificate.signing_key.serialize_der(),
                )
                .into(),
            )
            .expect("test TLS configuration");
        let acceptor = TlsAcceptor::from(Arc::new(tls));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let client = reqwest::Client::builder()
            .no_proxy()
            .proxy(reqwest::Proxy::https(format!("http://{address}")).unwrap())
            .use_rustls_tls()
            .add_root_certificate(reqwest::Certificate::from_der(certificate.cert.der()).unwrap())
            .build()
            .unwrap();
        let client = Arc::new(
            DecisionsClient::new(HttpClient::new(client), "synthetic-test-key".to_string())
                .unwrap(),
        );
        let (requests_tx, requests) = mpsc::unbounded_channel();
        let server = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    incoming = listener.accept() => {
                        let (stream, _) = incoming.unwrap();
                        connections.spawn(serve_decision(stream, acceptor.clone(), requests_tx.clone()));
                    }
                    result = connections.join_next(), if !connections.is_empty() => {
                        result.unwrap().expect("test proxy connection");
                    }
                }
            }
        });
        Self {
            client,
            requests,
            _server: AbortOnDropHandle::new(server),
        }
    }

    #[tracing::instrument(skip_all)]
    async fn request(&mut self) -> CapturedDecisionRequest {
        timeout(Duration::from_secs(/*secs*/ 5), self.requests.recv())
            .await
            .expect("decision request reached local proxy")
            .expect("test proxy remains alive")
    }
}

#[tracing::instrument(skip_all)]
async fn serve_decision(
    mut stream: TcpStream,
    acceptor: TlsAcceptor,
    requests: mpsc::UnboundedSender<CapturedDecisionRequest>,
) {
    let (connect, _) = read_http_request(&mut stream).await;
    assert!(connect.starts_with("CONNECT api.openai.com:443 HTTP/1.1\r\n"));
    stream
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await
        .unwrap();
    let mut stream = acceptor.accept(stream).await.unwrap();
    let (headers, body) = read_http_request(&mut stream).await;
    let (reply, response) = oneshot::channel();
    requests
        .send(CapturedDecisionRequest {
            headers,
            body: serde_json::from_slice(&body).unwrap(),
            reply,
        })
        .ok()
        .expect("request observer remains alive");
    let Ok((status, body)) = response.await else {
        return;
    };
    let body = body.to_string();
    let headers = format!(
        "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(headers.as_bytes()).await;
    let _ = stream.write_all(body.as_bytes()).await;
    let _ = stream.flush().await;
}

#[tracing::instrument(skip_all)]
async fn read_http_request(stream: &mut (impl AsyncRead + Unpin)) -> (String, Vec<u8>) {
    let mut bytes = Vec::new();
    let mut chunk = [0; 1024];
    let header_end = loop {
        if let Some(end) = bytes
            .windows(/*size*/ 4)
            .position(|window| window == b"\r\n\r\n")
        {
            break end + 4;
        }
        let count = stream.read(&mut chunk).await.unwrap();
        assert!(
            count != 0 && bytes.len() < 32 * 1024,
            "bounded HTTP headers"
        );
        bytes.extend_from_slice(&chunk[..count]);
    };
    let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
    let length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap_or_default();
    assert!(length <= 16 * 1024, "bounded decision body");
    while bytes.len() < header_end + length {
        let count = stream.read(&mut chunk).await.unwrap();
        assert_ne!(count, 0, "complete HTTP body");
        bytes.extend_from_slice(&chunk[..count]);
    }
    (headers, bytes[header_end..header_end + length].to_vec())
}

#[tracing::instrument(skip_all)]
async fn adaptive_fixture(
    client: Arc<DecisionsClient>,
    configure: impl FnOnce(&mut Config),
) -> (
    Arc<Session>,
    Arc<TurnContext>,
    async_channel::Receiver<codex_protocol::protocol::Event>,
) {
    let (session, turn, events) = make_session_and_context_with_auth_and_config_and_rx(
        CodexAuth::from_api_key("synthetic-primary-key"),
        Vec::new(),
        |config| {
            config.model = Some("gpt-6-astra".to_string());
            config.model_catalog = Some(codex_models_manager::bundled_models_response().unwrap());
            config.model_reasoning_effort = Some(ReasoningEffort::Low);
            config.adaptive_reasoning.enabled = true;
            config.adaptive_reasoning.decision_timeout_ms = 5_000;
            config.adaptive_reasoning.min_effort = ReasoningEffort::Low;
            config.adaptive_reasoning.max_effort = ReasoningEffort::High;
            config.adaptive_reasoning.rubric_instructions = Some("Test rubric only.".to_string());
            for feature in [
                Feature::StepModelSwitching,
                Feature::ReasoningEffortOverride,
            ] {
                config.features.enable(feature).unwrap();
            }
            configure(config);
        },
    )
    .await;
    if turn.config.adaptive_reasoning.enabled
        && turn
            .config
            .features
            .enabled(Feature::ReasoningEffortOverride)
        && turn
            .initial_settings
            .effective_reasoning_effort()
            .is_some_and(|effort| ORDINARY_EFFORTS.contains(&effort))
    {
        assert!(
            session
                .services
                .model_client
                .reasoning_effort_override_enabled(&turn.initial_settings.model_info),
            "positive fixture has enabled OpenAI reasoning updates"
        );
        assert!(
            allowed_efforts(
                &turn.config.adaptive_reasoning,
                &turn.initial_settings.model_info
            )
            .len()
                >= 2,
            "positive fixture advertises multiple choices inside adaptive bounds"
        );
    }
    session
        .services
        .thread_extension_data
        .get_or_init(AdaptiveSession::default)
        .client
        .set(Ok(client))
        .ok()
        .expect("seed synthetic Decisions client");
    session
        .spawn_task(
            Arc::clone(&turn),
            Vec::new(),
            HeldStepTask {
                kind: TaskKind::Compact,
                finish: Arc::new(tokio::sync::Notify::new()),
            },
        )
        .await;
    (session, turn, events)
}

fn user_task(text: &str) -> Vec<TurnInput> {
    vec![TurnInput::UserInput {
        content: vec![UserInput::Text {
            text: text.to_string(),
            text_elements: Vec::new(),
        }],
        client_id: None,
        metadata: UserInputMetadata::default(),
    }]
}

fn selected_effort(turn: &TurnContext) -> Option<ReasoningEffort> {
    turn.next_step_settings.load().effective_reasoning_effort()
}

fn assert_pending_job(turn: &TurnContext) {
    let slot = turn
        .extension_data
        .get::<Mutex<Option<PendingAdaptiveDecision>>>()
        .expect("eligible decision created pending slot");
    assert!(
        slot.try_lock().unwrap().is_some(),
        "genuine background decision was scheduled"
    );
}

fn start_adapting(
    session: &Arc<Session>,
    turn: &Arc<TurnContext>,
    input: &[TurnInput],
    trigger: AdaptiveReasoningTrigger,
    cancellation: &CancellationToken,
) -> tokio::task::JoinHandle<()> {
    let session = Arc::clone(session);
    let turn = Arc::clone(turn);
    let input = input.to_vec();
    let cancellation = cancellation.clone();
    tokio::spawn(async move {
        session
            .adapt_reasoning(&turn, &input, trigger, &cancellation)
            .await;
    })
}

#[tracing::instrument(skip_all)]
async fn finish_adaptation(adaptation: &mut tokio::task::JoinHandle<()>) {
    timeout(Duration::from_secs(/*secs*/ 5), adaptation)
        .await
        .expect("bounded pre-step decision returned")
        .expect("adaptive caller completed");
}

#[tokio::test]
async fn adaptive_step_barrier_awaits_publication_and_preserves_captured_steps() {
    let mut proxy = DecisionProxy::start().await;
    let (session, turn, events) = adaptive_fixture(Arc::clone(&proxy.client), |_| {}).await;
    let cancellation = CancellationToken::new();
    let first = session
        .capture_step_context(Arc::clone(&turn), &cancellation)
        .await
        .unwrap();
    let mut adaptation = start_adapting(
        &session,
        &turn,
        &user_task("Check a harmless arithmetic result."),
        AdaptiveReasoningTrigger::TurnStart,
        &cancellation,
    );
    let request = proxy.request().await;
    assert!(
        !adaptation.is_finished(),
        "pre-step barrier awaits held HTTP response"
    );
    assert!(
        request
            .headers
            .starts_with("POST /v1/decisions HTTP/1.1\r\n")
    );
    assert!(
        request
            .headers
            .to_ascii_lowercase()
            .contains("authorization: bearer synthetic-test-key\r\n")
    );
    assert_eq!(
        request.body["questions"],
        json!([{
            "type": "choice", "name": "reasoning_effort", "instructions": "Test rubric only.",
            "choices": [{"value": "low"}, {"value": "medium"}, {"value": "high"}]
        }])
    );
    assert_eq!(
        request.body["input"][0]["content"][0]["text"],
        "Current user task:\nCheck a harmless arithmetic result.\nRecent evidence (newest first):\n"
    );
    request
        .reply
        .send((
            200,
            json!({"answers": [{"type":"choice", "name":"reasoning_effort", "choice":"high"}]}),
        ))
        .unwrap();
    finish_adaptation(&mut adaptation).await;
    assert_pending_job(&turn);
    assert_eq!(selected_effort(&turn), Some(ReasoningEffort::High));
    let next = session
        .capture_step_context(Arc::clone(&turn), &cancellation)
        .await
        .unwrap();
    assert_eq!(
        (
            first.settings.effective_reasoning_effort(),
            next.settings.effective_reasoning_effort()
        ),
        (Some(ReasoningEffort::Low), Some(ReasoningEffort::High))
    );
    assert_eq!(
        session
            .state
            .lock()
            .await
            .session_configuration
            .step_settings
            .collaboration_mode
            .settings
            .reasoning_effort,
        Some(ReasoningEffort::Low)
    );
    session.report_reasoning_effort(&first).await;
    session.report_reasoning_effort(&next).await;
    let reported = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event.msg {
            codex_protocol::protocol::EventMsg::ReasoningEffortUpdated(update) => Some(update),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        reported,
        vec![
            codex_protocol::protocol::ReasoningEffortUpdatedEvent {
                turn_id: turn.sub_id.clone(),
                reasoning_effort: Some(ReasoningEffort::Low),
                adaptive: true,
            },
            codex_protocol::protocol::ReasoningEffortUpdatedEvent {
                turn_id: turn.sub_id.clone(),
                reasoning_effort: Some(ReasoningEffort::High),
                adaptive: true,
            },
        ]
    );
    session.stop_adaptive_reasoning().await;
}

#[test_case("assistant")]
#[test_case("tool")]
#[test_case("user")]
#[test_case("deadline")]
#[test_case("manual")]
#[tokio::test]
async fn adaptive_job_rejects_newer_evidence_or_manual_settings(scenario: &str) {
    let mut proxy = DecisionProxy::start().await;
    let (session, turn, events) = adaptive_fixture(Arc::clone(&proxy.client), |config| {
        if scenario == "deadline" {
            config.adaptive_reasoning.decision_timeout_ms = 500;
        }
    })
    .await;
    let cancellation = CancellationToken::new();
    let first = session
        .capture_step_context(Arc::clone(&turn), &cancellation)
        .await
        .unwrap();
    assert_eq!(
        first.settings.effective_reasoning_effort(),
        Some(ReasoningEffort::Low)
    );
    let mut adaptation = start_adapting(
        &session,
        &turn,
        &user_task("Check a harmless arithmetic result."),
        AdaptiveReasoningTrigger::TurnStart,
        &cancellation,
    );
    let request = proxy.request().await;
    assert!(
        !adaptation.is_finished(),
        "pre-step barrier awaits held HTTP response"
    );
    match scenario {
        "assistant" => {
            session
                .record_conversation_items(
                    &turn,
                    &turn.capture_current_model_info(),
                    &[ResponseItem::Message {
                        id: None,
                        role: "assistant".to_string(),
                        content: vec![ContentItem::OutputText {
                            text: "The result is four.".to_string(),
                        }],
                        phase: None,
                        internal_chat_message_metadata_passthrough: None,
                    }],
                )
                .await
        }
        "tool" => {
            session
                .record_conversation_items(
                    &turn,
                    &turn.capture_current_model_info(),
                    &[ResponseItem::FunctionCallOutput {
                        id: None,
                        call_id: Some("synthetic-call".to_string()),
                        name: None,
                        namespace: None,
                        output: FunctionCallOutputPayload::from_text("4".to_string()),
                        internal_chat_message_metadata_passthrough: None,
                    }],
                )
                .await
        }
        "user" => {
            session.reserve_user_input_order().await;
        }
        "deadline" => {
            tokio::time::sleep(Duration::from_millis(
                turn.config.adaptive_reasoning.decision_timeout_ms + 1,
            ))
            .await;
        }
        "manual" => {
            session.pause_adaptive_reasoning();
        }
        _ => unreachable!("defined test scenarios"),
    }
    request
        .reply
        .send((
            200,
            json!({"answers": [{"type":"choice", "name":"reasoning_effort", "choice":"high"}]}),
        ))
        .unwrap();
    finish_adaptation(&mut adaptation).await;
    assert_eq!(selected_effort(&turn), Some(ReasoningEffort::Low));
    let next = session
        .capture_step_context(Arc::clone(&turn), &cancellation)
        .await
        .unwrap();
    assert_eq!(
        next.settings.effective_reasoning_effort(),
        Some(ReasoningEffort::Low)
    );
    session.report_reasoning_effort(&next).await;
    let update = std::iter::from_fn(|| events.try_recv().ok()).find_map(|event| match event.msg {
        codex_protocol::protocol::EventMsg::ReasoningEffortUpdated(update) => Some(update),
        _ => None,
    });
    assert_eq!(
        update,
        Some(codex_protocol::protocol::ReasoningEffortUpdatedEvent {
            turn_id: turn.sub_id.clone(),
            reasoning_effort: Some(ReasoningEffort::Low),
            adaptive: scenario != "manual",
        })
    );
    assert_pending_job(&turn);
    session.stop_adaptive_reasoning().await;
}

#[test_case(503, json!({"error":"synthetic"}))]
#[test_case(200, json!({"answers": [{"type":"choice", "name":"reasoning_effort", "choice":"unsupported"}]}))]
#[tokio::test]
async fn adaptive_job_failure_retains_effort(status: u16, body: Value) {
    let mut proxy = DecisionProxy::start().await;
    let (session, turn, _events) = adaptive_fixture(Arc::clone(&proxy.client), |_| {}).await;
    let mut adaptation = start_adapting(
        &session,
        &turn,
        &user_task("Check a harmless arithmetic result."),
        AdaptiveReasoningTrigger::TurnStart,
        &CancellationToken::new(),
    );
    let request = proxy.request().await;
    assert!(
        !adaptation.is_finished(),
        "failure response has not released the barrier yet"
    );
    request.reply.send((status, body)).unwrap();
    finish_adaptation(&mut adaptation).await;
    assert_pending_job(&turn);
    assert_eq!(selected_effort(&turn), Some(ReasoningEffort::Low));
    session.stop_adaptive_reasoning().await;
}

#[test_case("disabled")]
#[test_case("override disabled")]
#[test_case("empty triggers")]
#[test_case("unsupported model")]
#[test_case("empty intersection")]
#[test_case("single intersection")]
#[test_case("ultra")]
#[test_case("persistent")]
#[test_case("feature worker")]
#[test_case("child")]
#[test_case("review")]
#[test_case("memory")]
#[tokio::test]
async fn ineligible_sessions_do_not_schedule_warmup_or_decisions(scenario: &str) {
    let mut proxy = DecisionProxy::start().await;
    let (session, turn, events) =
        adaptive_fixture(Arc::clone(&proxy.client), |config| match scenario {
            "disabled" => config.adaptive_reasoning.enabled = false,
            "empty triggers" => config.adaptive_reasoning.update_on.clear(),
            "override disabled" => {
                config
                    .features
                    .disable(Feature::ReasoningEffortOverride)
                    .unwrap();
            }
            "ultra" => config.model_reasoning_effort = Some(ReasoningEffort::Ultra),
            "persistent" => config.model_reasoning_effort = Some(ReasoningEffort::Persistent),
            _ => {}
        })
        .await;
    let mut selected = (*turn.next_step_settings.load_full()).clone();
    match scenario {
        "unsupported model" => {
            Arc::make_mut(&mut selected.model_info).supports_reasoning_effort_updates = false
        }
        "empty intersection" => Arc::make_mut(&mut selected.model_info)
            .supported_reasoning_levels
            .clear(),
        "single intersection" => {
            Arc::make_mut(&mut selected.model_info)
                .supported_reasoning_levels
                .retain(|preset| preset.effort == ReasoningEffort::Low);
            assert_eq!(selected.model_info.supported_reasoning_levels.len(), 1);
        }
        "feature worker" => {
            session
                .state
                .lock()
                .await
                .session_configuration
                .thread_source = Some(ThreadSource::Feature("thread_title".to_string()))
        }
        "child" => {
            session
                .state
                .lock()
                .await
                .session_configuration
                .thread_source = Some(ThreadSource::Subagent)
        }
        "review" => {
            session
                .state
                .lock()
                .await
                .session_configuration
                .thread_source = Some(ThreadSource::GuardianReview)
        }
        "memory" => {
            session
                .state
                .lock()
                .await
                .session_configuration
                .thread_source = Some(ThreadSource::MemoryConsolidation)
        }
        _ => {}
    }
    turn.next_step_settings.store(Arc::new(selected));
    session
        .services
        .thread_extension_data
        .insert((*turn.capture_current_model_info()).clone());
    let baseline = turn.next_step_settings.load_full();
    let source = session.state.lock().await.session_configuration.clone();
    session.start_adaptive_reasoning_warmup(&turn.config, &source);
    let mut adaptation = start_adapting(
        &session,
        &turn,
        &user_task("Check a harmless arithmetic result."),
        AdaptiveReasoningTrigger::TurnStart,
        &CancellationToken::new(),
    );
    finish_adaptation(&mut adaptation).await;
    assert!(
        timeout(Duration::from_millis(/*millis*/ 25), proxy.requests.recv())
            .await
            .is_err(),
        "ineligible sessions make no HTTP request"
    );
    if let Some(slot) = turn
        .extension_data
        .get::<Mutex<Option<PendingAdaptiveDecision>>>()
    {
        assert!(
            slot.try_lock().unwrap().is_none(),
            "no real job was scheduled"
        );
    }
    let adaptive = session
        .services
        .thread_extension_data
        .get::<AdaptiveSession>()
        .unwrap();
    assert!(
        !adaptive.warmup_started.load(Ordering::Acquire),
        "warmup was never scheduled"
    );
    assert!(adaptive.warmup.lock().unwrap().is_none());
    assert!(Arc::ptr_eq(&baseline, &turn.next_step_settings.load_full()));
    let step = session
        .capture_step_context(Arc::clone(&turn), &CancellationToken::new())
        .await
        .unwrap();
    session.report_reasoning_effort(&step).await;
    let updates = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event.msg {
            codex_protocol::protocol::EventMsg::ReasoningEffortUpdated(update) => Some(update),
            _ => None,
        })
        .collect::<Vec<_>>();
    let expected = match scenario {
        "disabled" | "feature worker" | "child" | "review" | "memory" => Vec::new(),
        "override disabled"
        | "empty triggers"
        | "unsupported model"
        | "empty intersection"
        | "single intersection"
        | "ultra"
        | "persistent" => vec![codex_protocol::protocol::ReasoningEffortUpdatedEvent {
            turn_id: turn.sub_id.clone(),
            reasoning_effort: step.settings.effective_reasoning_effort(),
            adaptive: false,
        }],
        _ => unreachable!("defined test scenarios"),
    };
    assert_eq!(updates, expected);
    session.stop_adaptive_reasoning().await;
}

#[tokio::test]
async fn warmup_is_one_shot_and_does_not_change_history_or_settings() {
    let mut proxy = DecisionProxy::start().await;
    let (session, turn, _events) = adaptive_fixture(Arc::clone(&proxy.client), |_| {}).await;
    session
        .services
        .thread_extension_data
        .insert((*turn.capture_current_model_info()).clone());
    let source = session.state.lock().await.session_configuration.clone();
    let baseline = turn.next_step_settings.load_full();
    let history = session.state.lock().await.history.clone();
    session.start_adaptive_reasoning_warmup(&turn.config, &source);
    session.start_adaptive_reasoning_warmup(&turn.config, &source);
    let request = proxy.request().await;
    assert_eq!(
        request.body["input"][0]["content"][0]["text"],
        "Acknowledge a greeting."
    );
    assert_eq!(
        request.body["questions"][0]["instructions"],
        "Choose the lowest effort sufficient for this routine task."
    );
    request
        .reply
        .send((
            200,
            json!({"answers": [{"type":"choice", "name":"reasoning_effort", "choice":"medium"}]}),
        ))
        .unwrap();
    let mut warmup = session
        .services
        .thread_extension_data
        .get::<AdaptiveSession>()
        .unwrap()
        .warmup
        .lock()
        .unwrap()
        .take()
        .unwrap();
    timeout(Duration::from_secs(/*secs*/ 5), &mut warmup.handle)
        .await
        .unwrap()
        .unwrap();
    session.start_adaptive_reasoning_warmup(&turn.config, &source);
    assert!(
        timeout(Duration::from_millis(/*millis*/ 25), proxy.requests.recv())
            .await
            .is_err()
    );
    assert!(Arc::ptr_eq(&baseline, &turn.next_step_settings.load_full()));
    assert_eq!(
        session.state.lock().await.history.annotated_items(),
        history.annotated_items()
    );
    session.stop_adaptive_reasoning().await;
}

#[test_case("turn cancelled")]
#[test_case("session shutdown")]
#[tokio::test]
async fn unfinished_adaptive_job_is_cancelled_without_settings_effect(scenario: &str) {
    let mut proxy = DecisionProxy::start().await;
    let (session, turn, _events) = adaptive_fixture(Arc::clone(&proxy.client), |_| {}).await;
    let cancellation = CancellationToken::new();
    let mut adaptation = start_adapting(
        &session,
        &turn,
        &user_task("Check a harmless arithmetic result."),
        AdaptiveReasoningTrigger::TurnStart,
        &cancellation,
    );
    let request = proxy.request().await;
    assert!(
        !adaptation.is_finished(),
        "pre-step barrier awaits held HTTP response"
    );
    match scenario {
        "turn cancelled" => {
            cancellation.cancel();
            finish_adaptation(&mut adaptation).await;
        }
        "session shutdown" => session.stop_adaptive_reasoning().await,
        _ => unreachable!("defined test scenarios"),
    }
    if scenario == "session shutdown" {
        finish_adaptation(&mut adaptation).await;
    }
    session.stop_adaptive_reasoning().await;
    assert_eq!(selected_effort(&turn), Some(ReasoningEffort::Low));
    assert!(
        turn.extension_data
            .get::<Mutex<Option<PendingAdaptiveDecision>>>()
            .unwrap()
            .lock()
            .await
            .is_none()
    );
    drop(request.reply);
    session.stop_adaptive_reasoning().await;
}

#[tokio::test]
async fn stalled_warmup_shutdown_does_not_wait_for_http_or_change_history() {
    let mut proxy = DecisionProxy::start().await;
    let (session, turn, _events) = adaptive_fixture(Arc::clone(&proxy.client), |_| {}).await;
    session
        .services
        .thread_extension_data
        .insert((*turn.capture_current_model_info()).clone());
    let source = session.state.lock().await.session_configuration.clone();
    let baseline = turn.next_step_settings.load_full();
    let history = session.state.lock().await.history.clone();
    session.start_adaptive_reasoning_warmup(&turn.config, &source);
    let request = proxy.request().await;
    timeout(
        Duration::from_millis(/*millis*/ 500),
        session.stop_adaptive_reasoning(),
    )
    .await
    .expect("shutdown cancels stalled warmup rather than waiting for HTTP deadline");
    assert!(
        session
            .services
            .thread_extension_data
            .get::<AdaptiveSession>()
            .unwrap()
            .warmup
            .lock()
            .unwrap()
            .is_none()
    );
    assert!(Arc::ptr_eq(&baseline, &turn.next_step_settings.load_full()));
    assert_eq!(
        session.state.lock().await.history.annotated_items(),
        history.annotated_items()
    );
    drop(request.reply);
}

#[tokio::test]
async fn busy_settings_publication_drops_completed_decision_without_waiting() {
    let mut proxy = DecisionProxy::start().await;
    let (session, turn, _events) = adaptive_fixture(Arc::clone(&proxy.client), |_| {}).await;
    let mut adaptation = start_adapting(
        &session,
        &turn,
        &user_task("Check a harmless arithmetic result."),
        AdaptiveReasoningTrigger::TurnStart,
        &CancellationToken::new(),
    );
    let request = proxy.request().await;
    assert!(
        !adaptation.is_finished(),
        "pre-step barrier awaits held HTTP response"
    );
    let permit = session.thread_settings_persistence.acquire().await.unwrap();
    request
        .reply
        .send((
            200,
            json!({"answers": [{"type":"choice", "name":"reasoning_effort", "choice":"high"}]}),
        ))
        .unwrap();
    timeout(
        Duration::from_millis(/*millis*/ 500),
        finish_adaptation(&mut adaptation),
    )
    .await
    .expect("automatic publisher does not wait for busy settings permit");
    assert_eq!(selected_effort(&turn), Some(ReasoningEffort::Low));
    drop(permit);
    session.stop_adaptive_reasoning().await;
}

#[test_case("superseded")]
#[test_case("manual")]
#[test_case("shutdown")]
#[tokio::test]
async fn completed_http_response_cannot_publish_after_state_barrier_invalidates_it(scenario: &str) {
    let mut proxy = DecisionProxy::start().await;
    let (session, turn, _events) = adaptive_fixture(Arc::clone(&proxy.client), |_| {}).await;
    let mut adaptation = start_adapting(
        &session,
        &turn,
        &user_task("Check a harmless arithmetic result."),
        AdaptiveReasoningTrigger::TurnStart,
        &CancellationToken::new(),
    );
    let request = proxy.request().await;
    assert!(
        !adaptation.is_finished(),
        "pre-step barrier awaits held HTTP response"
    );
    let mut state = session.state.lock().await;
    request
        .reply
        .send((
            200,
            json!({"answers": [{"type":"choice", "name":"reasoning_effort", "choice":"high"}]}),
        ))
        .unwrap();
    timeout(Duration::from_millis(/*millis*/ 500), async {
        while session.thread_settings_persistence.available_permits() != 0 {
            tokio::time::sleep(Duration::from_millis(/*millis*/ 1)).await;
        }
    })
    .await
    .expect("HTTP completed and publisher reached serialized settings preparation");
    match scenario {
        "superseded" => session.invalidate_adaptive_reasoning(),
        "manual" => session.pause_adaptive_reasoning(),
        "shutdown" => state.shutting_down = true,
        _ => unreachable!("defined test scenarios"),
    }
    drop(state);
    finish_adaptation(&mut adaptation).await;
    assert_eq!(selected_effort(&turn), Some(ReasoningEffort::Low));
    session.stop_adaptive_reasoning().await;
}

#[tokio::test]
async fn stalled_decision_does_not_keep_session_or_turn_alive() {
    let mut proxy = DecisionProxy::start().await;
    let (session, turn, _events) = adaptive_fixture(Arc::clone(&proxy.client), |_| {}).await;
    let adaptation = start_adapting(
        &session,
        &turn,
        &user_task("Check a harmless arithmetic result."),
        AdaptiveReasoningTrigger::TurnStart,
        &CancellationToken::new(),
    );
    let request = proxy.request().await;
    assert!(
        !adaptation.is_finished(),
        "pre-step barrier awaits held HTTP response"
    );
    adaptation.abort();
    let _ = adaptation.await;
    let session_weak = Arc::downgrade(&session);
    let turn_weak = Arc::downgrade(&turn);
    session
        .abort_all_tasks(codex_protocol::protocol::TurnAbortReason::Interrupted)
        .await;
    drop(turn);
    drop(session);
    timeout(Duration::from_millis(/*millis*/ 500), async {
        while session_weak.upgrade().is_some() || turn_weak.upgrade().is_some() {
            tokio::time::sleep(Duration::from_millis(/*millis*/ 1)).await;
        }
    })
    .await
    .expect("background HTTP target has no strong session/turn reference cycle");
    drop(request.reply);
}

#[test_case("success")]
#[test_case("failure")]
#[test_case("timeout")]
#[tokio::test]
async fn regular_turn_waits_for_each_decision_before_sampling(scenario: &str) {
    use crate::tasks::RegularTask;
    use codex_protocol::protocol::TurnAbortReason;
    use core_test_support::responses;

    let mut proxy = DecisionProxy::start().await;
    let primary = responses::start_mock_server().await;
    let requests = responses::mount_sse_sequence(
        &primary,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "plan-call",
                    "update_plan",
                    &json!({
                        "plan": [{"step":"Check the arithmetic", "status":"completed"}]
                    })
                    .to_string(),
                ),
                responses::ev_completed("plan-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("final-message", "The result is four."),
                responses::ev_completed("final-response"),
            ]),
        ],
    )
    .await;
    let (session, turn, events) = adaptive_fixture(Arc::clone(&proxy.client), |config| {
        config.model_provider.base_url = Some(format!("{}/v1", primary.uri()));
        config.model_provider.supports_websockets = false;
        config.update_plan_enabled = true;
        if scenario == "timeout" {
            config.adaptive_reasoning.decision_timeout_ms = 500;
        }
    })
    .await;
    session.abort_all_tasks(TurnAbortReason::Interrupted).await;
    let mut input = user_task("Check a harmless arithmetic result.");
    let TurnInput::UserInput { metadata, .. } = &mut input[0] else {
        unreachable!("accepted user task")
    };
    metadata.acceptance_order = Some(session.reserve_user_input_order().await);
    session
        .spawn_task(Arc::clone(&turn), input, RegularTask::new())
        .await;
    let first = proxy.request().await;
    tokio::time::sleep(Duration::from_millis(/*millis*/ 100)).await;
    assert!(
        requests.requests().is_empty(),
        "main inference waits for initial decision"
    );
    first
        .reply
        .send((
            200,
            json!({"answers": [{"type":"choice", "name":"reasoning_effort", "choice":"high"}]}),
        ))
        .unwrap();
    let second = proxy.request().await;
    tokio::time::sleep(Duration::from_millis(/*millis*/ 100)).await;
    assert_eq!(
        requests.requests().len(),
        1,
        "tool continuation waits for its decision"
    );
    assert_eq!(selected_effort(&turn), Some(ReasoningEffort::High));
    assert!(
        second.body["input"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Plan updated")
    );
    let mut reply = Some(second.reply);
    match scenario {
        "success" => reply
            .take()
            .unwrap()
            .send((
                200,
                json!({"answers": [{"type":"choice", "name":"reasoning_effort", "choice":"low"}]}),
            ))
            .unwrap(),
        "failure" => reply
            .take()
            .unwrap()
            .send((503, json!({"error":"synthetic"})))
            .unwrap(),
        "timeout" => {}
        _ => unreachable!("defined test scenarios"),
    }
    timeout(Duration::from_secs(/*secs*/ 5), async {
        while session.active_turn.lock().await.is_some() {
            tokio::time::sleep(Duration::from_millis(/*millis*/ 1)).await;
        }
    })
    .await
    .expect("normal turn continues after decision success or bounded fallback");
    drop(reply);
    let notifications = std::iter::from_fn(|| events.try_recv().ok())
        .map(|event| event.msg)
        .collect::<Vec<_>>();
    let captured = notifications
        .iter()
        .filter_map(|event| match event {
            codex_protocol::protocol::EventMsg::ReasoningEffortUpdated(update) => {
                Some(update.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let second_effort = match scenario {
        "success" => ReasoningEffort::Low,
        "failure" | "timeout" => ReasoningEffort::High,
        _ => unreachable!("defined test scenarios"),
    };
    assert_eq!(
        captured,
        vec![
            codex_protocol::protocol::ReasoningEffortUpdatedEvent {
                turn_id: turn.sub_id.clone(),
                reasoning_effort: Some(ReasoningEffort::High),
                adaptive: true,
            },
            codex_protocol::protocol::ReasoningEffortUpdatedEvent {
                turn_id: turn.sub_id.clone(),
                reasoning_effort: Some(second_effort),
                adaptive: true,
            },
        ]
    );
    let effort_positions = notifications
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            matches!(
                event,
                codex_protocol::protocol::EventMsg::ReasoningEffortUpdated(_)
            )
            .then_some(index)
        })
        .collect::<Vec<_>>();
    let model_positions = notifications
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            matches!(
                event,
                codex_protocol::protocol::EventMsg::RawResponseCompleted(_)
            )
            .then_some(index)
        })
        .collect::<Vec<_>>();
    assert_eq!(model_positions.len(), 2);
    assert!(effort_positions[0] < model_positions[0]);
    assert!(model_positions[0] < effort_positions[1]);
    assert!(effort_positions[1] < model_positions[1]);
    let requests = requests.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1].function_call_output_text("plan-call"),
        Some("Plan updated".to_string())
    );
    let updates = requests
        .iter()
        .map(|request| {
            request
                .input()
                .into_iter()
                .filter(|item| item["type"] == "configuration_update")
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let high = json!({"type":"configuration_update", "reasoning":{"effort":"high"}});
    let low = json!({"type":"configuration_update", "reasoning":{"effort":"low"}});
    let expected = match scenario {
        "success" => vec![vec![high.clone()], vec![high, low]],
        "failure" | "timeout" => vec![vec![high.clone()], vec![high]],
        _ => unreachable!("defined test scenarios"),
    };
    assert_eq!(updates, expected);
    assert_eq!(
        requests
            .iter()
            .map(|request| request.body_json()["reasoning"]["effort"].clone())
            .collect::<Vec<_>>(),
        vec![json!("high"), json!("high")]
    );
    let first_input = requests[0].input();
    let second_input = requests[1].input();
    assert_eq!(&second_input[..first_input.len()], first_input.as_slice());
    assert_eq!(
        session
            .state
            .lock()
            .await
            .session_configuration
            .step_settings
            .collaboration_mode
            .settings
            .reasoning_effort,
        Some(ReasoningEffort::Low)
    );
    assert!(
        proxy.requests.try_recv().is_err(),
        "two main steps require exactly two real decisions"
    );
    session.stop_adaptive_reasoning().await;
}
