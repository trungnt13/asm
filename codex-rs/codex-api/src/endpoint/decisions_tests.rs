use super::*;
use codex_http_client::HttpClientBuilder;
use pretty_assertions::assert_eq;
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
async fn basic_decisions_uses_custom_rubric_and_large_context() {
    let server = MockServer::start().await;
    // Basic: send the choice contract and custom rubric with one large Unicode context.
    // Expected chunks are independent of the request builder and preserve nearly 32 KiB.
    let rubric = "Choose sufficient effort.\nCustom instructions: preserve all constraints — 世界.";
    let input = "界".repeat(/*n*/ 10922);
    let chunks = [vec!["界".repeat(/*n*/ 2730); 4], vec!["界界".to_string()]].concat();
    Mock::given(method("POST"))
        .and(path("/v1/decisions"))
        .and(header("authorization", "Bearer synthetic-decisions-key"))
        .and(header("content-type", "application/json"))
        .and(body_json(json!({
            "model": "gpt-6-luna",
            "input": chunks.iter().map(|text| json!({
                "role": "user", "content": [{"type": "input_text", "text": text}]
            })).collect::<Vec<_>>(),
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
            .with_endpoint(format!("{}/v1/decisions", server.uri()))
            .unwrap()
            .choose_effort(
                "gpt-6-luna",
                &input,
                rubric,
                &["low".to_string(), "high".to_string()],
            )
            .await,
        Ok("high".to_string()),
    );
    server.verify().await;
}

#[tokio::test]
async fn basic_decisions_returns_low_effort() {
    let server = MockServer::start().await;
    // Basic: a valid ordinary request accepts a supported low-effort answer.
    Mock::given(method("POST"))
        .and(body_json(json!({
            "model": "jev-latest",
            "state": "Greet the user.",
            "questions": {"reasoning_effort": {
                "type": "choice", "instructions": "Choose sufficient effort.",
                "criteria": {"low": null, "medium": null}
            }}
        })))
        .respond_with(ResponseTemplate::new(/*s*/ 200).set_body_json(json!({
            "answers": {"reasoning_effort": {"type": "choice", "choice": "low"}}
        })))
        .expect(/*r*/ 1)
        .mount(&server)
        .await;
    let mut client = client();
    client.backend = DecisionBackend::TypeSafe;
    assert_eq!(
        client
            .with_endpoint(server.uri())
            .unwrap()
            .choose_effort(
                "jev-latest",
                "Greet the user.",
                "Choose sufficient effort.",
                &["low".to_string(), "medium".to_string()],
            )
            .await,
        Ok("low".to_string()),
    );
    server.verify().await;
}

#[tokio::test]
async fn special_decisions_rejects_unsupported_effort() {
    let server = MockServer::start().await;
    // Special: an unsupported answer is rejected without exposing its wire contents.
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(/*s*/ 200).set_body_json(json!({
            "answers": [{"type": "choice", "name": "reasoning_effort", "choice": "wire-secret"}]
        })))
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
