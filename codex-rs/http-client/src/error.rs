//! Errors returned by the shared Codex HTTP transport.

use crate::client::HttpError;
use crate::retry_after::RetryAfter;
use http::HeaderMap;
use http::StatusCode;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TransportError {
    #[error(transparent)]
    Policy(#[from] crate::NetworkPolicyDenied),
    #[error("http {status}: {body:?}")]
    Http {
        status: StatusCode,
        url: Option<String>,
        headers: Option<HeaderMap>,
        body: Option<String>,
        retry_after: Option<RetryAfter>,
    },
    #[error("retry limit reached")]
    RetryLimit,
    #[error("timeout")]
    Timeout,
    #[error("connection failed: {0}")]
    Connection(#[source] HttpError),
    #[error("network error: {0}")]
    Network(String),
    #[error("request build error: {0}")]
    Build(String),
    #[error("response body exceeds the {max_bytes} byte limit")]
    ResponseTooLarge { max_bytes: usize },
}

impl TransportError {
    /// Identifies structured overloads that callers can retry outside the HTTP transport.
    pub fn is_server_overloaded(&self) -> bool {
        match self {
            Self::Http { status, body, .. } if *status == StatusCode::SERVICE_UNAVAILABLE => body
                .as_deref()
                .and_then(|body| serde_json::from_str::<serde_json::Value>(body).ok())
                .is_some_and(|body| body["error"]["code"] == "server_is_overloaded"),
            Self::Http { .. }
            | Self::Timeout
            | Self::Connection(_)
            | Self::Network(_)
            | Self::Build(_)
            | Self::RetryLimit
            | Self::Policy(_)
            | Self::ResponseTooLarge { .. } => false,
        }
    }

    /// Returns retry advice captured when this error response arrived.
    pub fn retry_after(&self) -> Option<RetryAfter> {
        match self {
            Self::Http { retry_after, .. } => *retry_after,
            Self::Policy(_)
            | Self::RetryLimit
            | Self::Timeout
            | Self::Connection(_)
            | Self::Network(_)
            | Self::Build(_)
            | Self::ResponseTooLarge { .. } => None,
        }
    }
}

#[derive(Debug, Error)]
pub enum StreamError {
    #[error("stream failed: {0}")]
    Stream(String),
    #[error("timeout")]
    Timeout,
}
