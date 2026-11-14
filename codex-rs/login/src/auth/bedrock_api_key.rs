use std::fmt;
use std::path::Path;

use codex_config::types::AuthCredentialsStoreMode;
use serde::Deserialize;
use serde::Serialize;

use super::storage::AuthKeyringBackendKind;

/// Managed Amazon Bedrock API key persisted in `auth.json`.
#[derive(Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct BedrockApiKeyAuth {
    pub api_key: String,
    pub region: String,
}

impl fmt::Debug for BedrockApiKeyAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BedrockApiKeyAuth")
            .field("api_key", &"<redacted>")
            .field("region", &self.region)
            .finish()
    }
}

/// Retains the login API without writing unsupported Bedrock credentials.
pub fn login_with_bedrock_api_key(
    _codex_home: &Path,
    _api_key: &str,
    _region: &str,
    _auth_credentials_store_mode: AuthCredentialsStoreMode,
    _keyring_backend_kind: AuthKeyringBackendKind,
) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Amazon Bedrock is unsupported in this build.",
    ))
}

#[cfg(test)]
#[path = "bedrock_api_key_tests.rs"]
mod tests;
