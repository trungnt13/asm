use super::*;
use crate::session::input_queue::UserInputMetadata;
use crate::session::tests::HeldStepTask;
use crate::session::tests::make_session_and_context_with_auth_and_config_and_rx;
use crate::state::TaskKind;
use codex_features::Feature;
use codex_http_client::HttpClient;
use codex_login::CodexAuth;
use codex_protocol::user_input::UserInput;
use serde_json::Value;
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

use pretty_assertions::assert_eq;
use serde_json::json;
use test_case::test_case;

struct CapturedDecisionRequest {
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
    let (_, body) = read_http_request(&mut stream).await;
    let (reply, response) = oneshot::channel();
    requests
        .send(CapturedDecisionRequest {
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
    assert!(length <= 64 * 1024, "bounded decision body");
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
            config.adaptive_reasoning.decision_every_n_steps = 1;
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
    session
        .services
        .thread_extension_data
        .get_or_init(AdaptiveSession::default)
        .client
        .set(client)
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

#[test_case("success"; "selected_effort_precedes_each_request")]
#[test_case("failure"; "http_failure_continues_with_current_effort")]
#[test_case("timeout"; "timeout_continues_with_current_effort")]
#[tokio::test]
async fn basic_regular_turn_waits_for_each_decision_before_sampling(scenario: &str) {
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
    let (session, turn, _events) = adaptive_fixture(Arc::clone(&proxy.client), |config| {
        config.model_provider.base_url = Some(format!("{}/v1", primary.uri()));
        config.model_provider.supports_websockets = false;
        config.update_plan_enabled = true;
        if scenario == "timeout" {
            config.adaptive_reasoning.decision_timeout_ms = 500;
        }
    })
    .await;
    session.abort_all_tasks(TurnAbortReason::Interrupted).await;
    let task = "Check a harmless arithmetic result.";
    let mut input = user_task(task);
    let TurnInput::UserInput { metadata, .. } = &mut input[0] else {
        unreachable!("accepted user task")
    };
    metadata.acceptance_order = Some(session.reserve_user_input_order().await);
    session
        .spawn_task(Arc::clone(&turn), input, RegularTask::new())
        .await;
    let first = proxy.request().await;
    assert!(
        first.body["input"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains(task),
        "the real Decisions request contains this accepted task before main inference"
    );
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
    assert_eq!(
        turn.next_step_settings.load().effective_reasoning_effort(),
        Some(ReasoningEffort::High)
    );
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
