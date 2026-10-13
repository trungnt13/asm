use super::super::*;
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

pub(super) struct CapturedDecisionRequest {
    pub(super) body: Value,
    pub(super) reply: oneshot::Sender<(u16, Value)>,
}

pub(super) struct DecisionProxy {
    pub(super) client: Arc<DecisionsClient>,
    pub(super) requests: mpsc::UnboundedReceiver<CapturedDecisionRequest>,
    _server: AbortOnDropHandle<()>,
}

impl DecisionProxy {
    #[tracing::instrument(skip_all)]
    pub(super) async fn start() -> Self {
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
    pub(super) async fn request(&mut self) -> CapturedDecisionRequest {
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
pub(super) async fn adaptive_fixture(
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

pub(super) fn user_task(text: &str) -> Vec<TurnInput> {
    vec![TurnInput::UserInput {
        content: vec![UserInput::Text {
            text: text.to_string(),
            text_elements: Vec::new(),
        }],
        client_id: None,
        metadata: UserInputMetadata::default(),
    }]
}
