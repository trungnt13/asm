use std::path::Path;

use codex_config::types::AuthCredentialsStoreMode;
use serde::Deserialize;
use serde::Serialize;

use super::storage::AuthKeyringBackendKind;

/// Managed Amazon Bedrock AWS access keys persisted in auth storage.
#[derive(Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct BedrockAccessKeysAuth {
    pub access_key_id: String,
    pub secret_access_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
}

impl std::fmt::Debug for BedrockAccessKeysAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BedrockAccessKeysAuth")
            .field("access_key_id", &"<redacted>")
            .field("secret_access_key", &"<redacted>")
            .field(
                "session_token",
                &self.session_token.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

/// Retains the login API without writing unsupported Bedrock credentials.
pub fn login_with_bedrock_access_keys(
    _codex_home: &Path,
    _access_key_id: &str,
    _secret_access_key: &str,
    _session_token: Option<&str>,
    _auth_credentials_store_mode: AuthCredentialsStoreMode,
    _keyring_backend_kind: AuthKeyringBackendKind,
) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Amazon Bedrock is unsupported in this build.",
    ))
}
