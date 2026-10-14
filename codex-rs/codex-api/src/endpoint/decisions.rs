use codex_http_client::HttpClient;
use serde_json::Value;
use serde_json::json;
use thiserror::Error;

pub const DECISIONS_URL: &str = "https://api.openai.com/v1/decisions";
const TYPESAFE_URL: &str = "https://api.typesafe.ai/v1/systemone";
const MAX_RESPONSE_BYTES: usize = 16 * 1024;
// A byte cap also bounds tokens per input item without a tokenizer on the step path.
const MAX_INPUT_ITEM_BYTES: usize = 8192;

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum DecisionsError {
    #[error("Decision backend credentials are missing or empty")]
    Credentials,
    #[error("Decision request parameters are invalid")]
    InvalidRequest,
    #[error("Decision transport failed")]
    Transport,
    #[error("Decision backend returned HTTP {0}")]
    Http(u16),
    #[error("Decision response exceeds the response limit")]
    ResponseTooLarge,
    #[error("Decision backend returned an invalid answer")]
    InvalidResponse,
}

#[derive(Clone, Copy)]
enum DecisionBackend {
    OpenAi,
    TypeSafe,
}

// No Debug implementation: credentials and classification evidence are private.
pub struct DecisionsClient {
    client: HttpClient,
    api_key: String,
    backend: DecisionBackend,
}

impl DecisionsClient {
    pub fn new(client: HttpClient, api_key: String) -> Result<Self, DecisionsError> {
        if api_key.trim().is_empty() {
            return Err(DecisionsError::Credentials);
        }
        Ok(Self {
            client: client.without_request_logging(),
            api_key,
            backend: DecisionBackend::OpenAi,
        })
    }

    pub fn new_typesafe(client: HttpClient, api_key: String) -> Result<Self, DecisionsError> {
        let mut client = Self::new(client, api_key)?;
        client.backend = DecisionBackend::TypeSafe;
        Ok(client)
    }

    pub fn backend_name(&self) -> &'static str {
        match self.backend {
            DecisionBackend::OpenAi => "openai",
            DecisionBackend::TypeSafe => "typesafe",
        }
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
        let url = match self.backend {
            DecisionBackend::OpenAi => DECISIONS_URL,
            DecisionBackend::TypeSafe => TYPESAFE_URL,
        };
        self.choose_effort_at(url, model, input, instructions, choices)
            .await
    }

    #[tracing::instrument(skip_all)]
    async fn choose_effort_at(
        &self,
        url: &str,
        model: &str,
        input: &str,
        instructions: &str,
        choices: &[String],
    ) -> Result<String, DecisionsError> {
        if model.trim().is_empty()
            || input.trim().is_empty()
            || instructions.trim().is_empty()
            || !(2..=255).contains(&choices.len())
            || choices.iter().any(|choice| choice.trim().is_empty())
            || choices
                .iter()
                .enumerate()
                .any(|(index, choice)| choices[..index].contains(choice))
        {
            return Err(DecisionsError::InvalidRequest);
        }
        let mut chunks = Vec::new();
        let mut remaining = input;
        while !remaining.is_empty() {
            let mut end = remaining.floor_char_boundary(remaining.len().min(MAX_INPUT_ITEM_BYTES));
            if remaining.len() > MAX_INPUT_ITEM_BYTES
                && let Some(newline) = remaining[..end].rfind('\n')
            {
                end = newline + 1;
            }
            let (text, rest) = remaining.split_at(end);
            chunks.push(text);
            remaining = rest;
        }
        let body = match self.backend {
            DecisionBackend::OpenAi => {
                let input_items: Vec<_> = chunks.iter().map(|text|
                    json!({"role": "user", "content": [{"type": "input_text", "text": text}]})
                ).collect();
                tracing::debug!(
                    input_bytes = input.len(),
                    input_items = input_items.len(),
                    "Decisions input bounded"
                );
                json!({
                    "model": model,
                    "input": input_items,
                    "questions": [{
                        "type": "choice",
                        "name": "reasoning_effort",
                        "instructions": instructions,
                        "choices": choices.iter().map(|value| json!({"value": value})).collect::<Vec<_>>()
                    }]
                })
            }
            DecisionBackend::TypeSafe => {
                let criteria: serde_json::Map<String, Value> = choices
                    .iter()
                    .map(|choice| (choice.clone(), Value::Null))
                    .collect();
                json!({
                    "model": model,
                    "state": if chunks.len() == 1 { json!(input) } else { json!(chunks) },
                    "questions": {
                        "reasoning_effort": {
                            "type": "choice",
                            "instructions": instructions,
                            "criteria": criteria
                        }
                    }
                })
            }
        };
        let mut response = self
            .client
            .post(url)
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
        let answer = match self.backend {
            DecisionBackend::OpenAi => {
                let answers = body["answers"]
                    .as_array()
                    .ok_or(DecisionsError::InvalidResponse)?;
                if answers.len() != 1 || answers[0]["name"] != "reasoning_effort" {
                    return Err(DecisionsError::InvalidResponse);
                }
                &answers[0]
            }
            DecisionBackend::TypeSafe => {
                let answers = body["answers"]
                    .as_object()
                    .ok_or(DecisionsError::InvalidResponse)?;
                if answers.len() != 1 {
                    return Err(DecisionsError::InvalidResponse);
                }
                answers
                    .get("reasoning_effort")
                    .ok_or(DecisionsError::InvalidResponse)?
            }
        };
        if answer["type"] != "choice" {
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

#[cfg(test)]
#[path = "decisions_tests.rs"]
mod tests;
