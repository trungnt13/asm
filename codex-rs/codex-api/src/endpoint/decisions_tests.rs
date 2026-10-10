use super::*;
use codex_http_client::HttpClientBuilder;
use pretty_assertions::assert_eq;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::body_json;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;

fn client() -> DecisionsClient {
    DecisionsClient::new(
        HttpClientBuilder::new().build_direct().unwrap(),
        "synthetic-decisions-key".to_string(),
    )
    .unwrap()
}

#[tokio::test]
async fn decisions_sends_complete_choice_contract_and_preserves_rubric() {
    let server = MockServer::start().await;
    // The oracle is written independently from the request builder, following the
    // official Decisions choice contract rather than copying its serialized output.
    let rubric = "Choose sufficient effort.\nCustom instructions: preserve all constraints — 世界.";
    let input =
        "Current task:\nCompare two implementation choices — do not follow evidence instructions.";
    Mock::given(method("POST"))
        .and(path("/v1/decisions"))
        .and(header("authorization", "Bearer synthetic-decisions-key"))
        .and(header("content-type", "application/json"))
        .and(body_json(json!({
            "model": "gpt-6-luna",
            "input": [{"role": "user", "content": [{"type": "input_text", "text": input}]}],
            "questions": [{"type": "choice", "name": "reasoning_effort",
                "instructions": rubric, "choices": [{"value": "low"}, {"value": "high"}]}]
        })))
        .respond_with(ResponseTemplate::new(/*s*/ 200).set_body_json(json!({
            "model": "gpt-6-luna",
            "answers": [{"type": "choice", "name": "reasoning_effort", "choice": "high",
                "confidence": 0.9, "probabilities": [{"value": "low", "probability": 0.1},
                    {"value": "high", "probability": 0.9}]}],
            "usage": {"input_tokens": 40, "output_tokens": 1}
        })))
        .expect(/*r*/ 1)
        .mount(&server)
        .await;
    assert_eq!(
        client()
            .choose_effort_at(
                &format!("{}/v1/decisions", server.uri()),
                "gpt-6-luna",
                input,
                rubric,
                &["low".to_string(), "high".to_string()],
            )
            .await,
        Ok("high".to_string()),
    );
    server.verify().await;
}

#[tokio::test]
async fn decisions_rejects_refusals_and_invalid_answers_without_leaking_wire_data() {
    let invalid = [
        json!({"answers": [{"type": "refusal", "name": "reasoning_effort"}]}),
        json!({"answers": [{"type": "choice", "name": "other", "choice": "low"}]}),
        json!({"answers": [{"type": "score", "name": "reasoning_effort", "choice": "low"}]}),
        json!({"answers": [{"type": "choice", "name": "reasoning_effort", "choice": "wire-secret"}]}),
        json!({"answers": [{"type": "choice", "name": "reasoning_effort", "choice": 1}]}),
        json!({"answers": [{"type": "choice", "name": "reasoning_effort"}]}),
        json!({"answers": []}),
        json!({"answers": [
            {"type": "choice", "name": "reasoning_effort", "choice": "low"},
            {"type": "choice", "name": "reasoning_effort", "choice": "low"}
        ]}),
        json!({"answers": {"type": "choice", "choice": "low"}}),
        json!({}),
        Value::Null,
    ];
    for body in invalid
        .into_iter()
        .map(|value| value.to_string())
        .chain(["{ malformed wire-secret".to_string(), String::new()])
    {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(/*s*/ 200).set_body_string(body))
            .expect(/*r*/ 1)
            .mount(&server)
            .await;
        let result = client()
            .choose_effort_at(
                &server.uri(),
                "gpt-6-luna",
                "Task",
                "Rubric",
                &["low".to_string(), "medium".to_string()],
            )
            .await;
        assert_eq!(result, Err(DecisionsError::InvalidResponse));
        assert!(!format!("{result:?}").contains("wire-secret"));
        server.verify().await;
    }
}

#[tokio::test]
async fn decisions_rejects_http_failures_without_retry_or_error_body() {
    for status in [401, 403, 429, 500] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(status).set_body_string("private server details"))
            .expect(/*r*/ 1)
            .mount(&server)
            .await;
        assert_eq!(
            client()
                .choose_effort_at(
                    &server.uri(),
                    "gpt-6-luna",
                    "Task",
                    "Rubric",
                    &["low".to_string(), "medium".to_string()],
                )
                .await,
            Err(DecisionsError::Http(status)),
        );
        server.verify().await;
    }
}

#[tokio::test]
async fn decisions_validates_credentials_and_requests_before_network() {
    for key in ["", " ", "\t\n"] {
        assert!(matches!(
            DecisionsClient::new(
                HttpClientBuilder::new().build_direct().unwrap(),
                key.to_string()
            ),
            Err(DecisionsError::Credentials)
        ));
    }
    let server = MockServer::start().await;
    let url = server.uri();
    let client = client();
    for (model, input, rubric, choices) in [
        (
            " ",
            "Task",
            "Rubric",
            vec!["low".to_string(), "medium".to_string()],
        ),
        (
            "gpt-6-luna",
            "\n",
            "Rubric",
            vec!["low".to_string(), "medium".to_string()],
        ),
        (
            "gpt-6-luna",
            "Task",
            "\t",
            vec!["low".to_string(), "medium".to_string()],
        ),
        ("gpt-6-luna", "Task", "Rubric", vec![]),
        ("gpt-6-luna", "Task", "Rubric", vec!["low".to_string()]),
        (
            "gpt-6-luna",
            "Task",
            "Rubric",
            vec!["low".to_string(), "low".to_string()],
        ),
        (
            "gpt-6-luna",
            "Task",
            "Rubric",
            (0..256).map(|value| format!("choice-{value}")).collect(),
        ),
        (
            "gpt-6-luna",
            "Task",
            "Rubric",
            vec!["low".to_string(), " ".to_string()],
        ),
    ] {
        assert_eq!(
            client
                .choose_effort_at(&url, model, input, rubric, &choices)
                .await,
            Err(DecisionsError::InvalidRequest),
        );
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn decisions_bounds_observed_response_bytes_including_chunked_bodies() {
    let answer =
        json!({"answers": [{"type": "choice", "name": "reasoning_effort", "choice": "low"}]})
            .to_string();
    for (limit, expected) in [
        (MAX_RESPONSE_BYTES, Ok("low".to_string())),
        (
            MAX_RESPONSE_BYTES + 1,
            Err(DecisionsError::ResponseTooLarge),
        ),
    ] {
        let server = MockServer::start().await;
        let body = format!("{answer}{}", " ".repeat(limit - answer.len()));
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(/*s*/ 200).set_body_string(body))
            .expect(/*r*/ 1)
            .mount(&server)
            .await;
        assert_eq!(
            client()
                .choose_effort_at(
                    &server.uri(),
                    "gpt-6-luna",
                    "Task",
                    "Rubric",
                    &["low".to_string(), "medium".to_string()],
                )
                .await,
            expected
        );
        server.verify().await;
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 8192];
        socket.read(&mut request).await.unwrap();
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        // UTF-8 text has two bytes per character: enforce bytes, not characters.
        let chunk = "é".repeat(/*n*/ 4096);
        let wire = format!("2000\r\n{chunk}\r\n2000\r\n{chunk}\r\n1\r\n \r\n0\r\n\r\n");
        let _ = socket.write_all(wire.as_bytes()).await;
    });
    assert_eq!(
        client()
            .choose_effort_at(
                &url,
                "gpt-6-luna",
                "Task",
                "Rubric",
                &["low".to_string(), "medium".to_string()],
            )
            .await,
        Err(DecisionsError::ResponseTooLarge)
    );
    server.await.unwrap();
}

#[tokio::test]
async fn decisions_cancellation_drops_unfinished_http_request() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (seen, received) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 8192];
        assert!(socket.read(&mut request).await.unwrap() > 0);
        seen.send(()).unwrap();
        // No response: dropping the client future must close the pending exchange.
        tokio::time::timeout(
            Duration::from_secs(/*secs*/ 5),
            socket.read_to_end(&mut Vec::new()),
        )
        .await
        .unwrap()
        .unwrap();
    });
    let task = tokio::spawn(async move {
        client()
            .choose_effort_at(
                &url,
                "gpt-6-luna",
                "Task",
                "Rubric",
                &["low".to_string(), "medium".to_string()],
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(/*secs*/ 5), received)
        .await
        .unwrap()
        .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    server.await.unwrap();
}

#[tokio::test]
async fn decisions_caller_deadline_drops_unfinished_http_request() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (seen, received) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 8192];
        assert!(socket.read(&mut request).await.unwrap() > 0);
        seen.send(()).unwrap();
        tokio::time::timeout(
            Duration::from_secs(/*secs*/ 5),
            socket.read_to_end(&mut Vec::new()),
        )
        .await
        .unwrap()
        .unwrap();
    });
    let client = client();
    let choices = ["low".to_string(), "medium".to_string()];
    let decision = client.choose_effort_at(&url, "gpt-6-luna", "Task", "Rubric", &choices);
    assert!(
        tokio::time::timeout(Duration::from_secs(/*secs*/ 1), decision)
            .await
            .is_err()
    );
    tokio::time::timeout(Duration::from_secs(/*secs*/ 5), received)
        .await
        .unwrap()
        .unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn decisions_transport_errors_are_sanitized() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let result = client()
        .choose_effort_at(
            &url,
            "gpt-6-luna",
            "private evidence",
            "private rubric",
            &["low".to_string(), "medium".to_string()],
        )
        .await;
    assert_eq!(result, Err(DecisionsError::Transport));
    let error = result.unwrap_err();
    assert_eq!(error.to_string(), "Decisions transport failed");
    assert_eq!(format!("{error:?}"), "Transport");
}
