use codex_http_client::HttpClient;
use serde_json::Value;
use serde_json::json;
use thiserror::Error;

pub const DECISIONS_URL: &str = "https://api.openai.com/v1/decisions";
const MAX_RESPONSE_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum DecisionsError {
    #[error("Decisions credentials are missing or empty")]
    Credentials,
    #[error("Decisions request parameters are invalid")]
    InvalidRequest,
    #[error("Decisions transport failed")]
    Transport,
    #[error("Decisions returned HTTP {0}")]
    Http(u16),
    #[error("Decisions response exceeds the response limit")]
    ResponseTooLarge,
    #[error("Decisions returned an invalid answer")]
    InvalidResponse,
}

// No Debug implementation: credentials and classification evidence are private.
pub struct DecisionsClient {
    client: HttpClient,
    api_key: String,
}

impl DecisionsClient {
    pub fn new(client: HttpClient, api_key: String) -> Result<Self, DecisionsError> {
        if api_key.trim().is_empty() {
            return Err(DecisionsError::Credentials);
        }
        Ok(Self {
            client: client.without_request_logging(),
            api_key,
        })
    }

    // Callers bound input size and own the total deadline, cancellation and fallback policy.
    #[tracing::instrument(skip_all)]
    pub async fn choose_effort(
        &self,
        model: &str,
        input: &str,
        instructions: &str,
        choices: &[String],
    ) -> Result<String, DecisionsError> {
        if model.trim().is_empty()
            || input.trim().is_empty()
            || instructions.trim().is_empty()
            || choices.is_empty()
            || choices.iter().any(|choice| choice.trim().is_empty())
        {
            return Err(DecisionsError::InvalidRequest);
        }
        let body = json!({
            "model": model,
            "input": [{"role": "user", "content": [{"type": "input_text", "text": input}]}],
            "questions": [{
                "type": "choice",
                "name": "reasoning_effort",
                "instructions": instructions,
                "choices": choices.iter().map(|value| json!({"value": value})).collect::<Vec<_>>()
            }]
        });
        let mut response = self
            .client
            .post(DECISIONS_URL)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|_| DecisionsError::Transport)?;
        if !response.status().is_success() {
            return Err(DecisionsError::Http(response.status().as_u16()));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| DecisionsError::Transport)?
        {
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(DecisionsError::ResponseTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        let body: Value =
            serde_json::from_slice(&bytes).map_err(|_| DecisionsError::InvalidResponse)?;
        let answers = body["answers"]
            .as_array()
            .ok_or(DecisionsError::InvalidResponse)?;
        if answers.len() != 1 {
            return Err(DecisionsError::InvalidResponse);
        }
        let answer = &answers[0];
        if answer["type"] != "choice" || answer["name"] != "reasoning_effort" {
            return Err(DecisionsError::InvalidResponse);
        }
        let choice = answer["choice"]
            .as_str()
            .ok_or(DecisionsError::InvalidResponse)?;
        choices
            .iter()
            .find(|allowed| allowed.as_str() == choice)
            .cloned()
            .ok_or(DecisionsError::InvalidResponse)
    }
}
