use codex_config::types::AuthCredentialsStoreMode;
use codex_protocol::auth::AuthMode;
use pretty_assertions::assert_eq;
use serial_test::serial;
use tempfile::tempdir;

use super::*;
use crate::auth::AuthKeyringBackendKind;
use crate::auth::AuthManager;
use crate::auth::CodexAuth;
use crate::auth::storage::AuthDotJson;
use crate::auth::storage::AuthStorageBackend;
use crate::auth::storage::FileAuthStorage;

fn api_key_auth() -> AuthDotJson {
    AuthDotJson {
        auth_mode: Some(AuthMode::ApiKey),
        openai_api_key: Some("sk-test-key".to_string()),
        tokens: None,
        last_refresh: None,
        agent_identity: None,
        personal_access_token: None,
        bedrock_api_key: None,
        bedrock_access_keys: None,
    }
}

fn bedrock_only_auth() -> AuthDotJson {
    AuthDotJson {
        auth_mode: None,
        openai_api_key: None,
        tokens: None,
        last_refresh: None,
        agent_identity: None,
        personal_access_token: None,
        bedrock_api_key: Some(bedrock_auth()),
        bedrock_access_keys: None,
    }
}

fn bedrock_auth() -> BedrockApiKeyAuth {
    BedrockApiKeyAuth {
        api_key: "bedrock-api-key-test".to_string(),
        region: "us-east-1".to_string(),
    }
}

#[test]
fn bedrock_api_key_debug_redacts_secret() {
    let auth = bedrock_auth();

    assert_eq!(
        format!("{auth:?}"),
        r#"BedrockApiKeyAuth { api_key: "<redacted>", region: "us-east-1" }"#
    );
    assert_eq!(
        format!("{:?}", CodexAuth::BedrockApiKey(auth)),
        r#"BedrockApiKey(BedrockApiKeyAuth { api_key: "<redacted>", region: "us-east-1" })"#
    );
}

#[test]
fn login_with_bedrock_api_key_rejects_without_replacing_openai_auth() -> anyhow::Result<()> {
    let home = tempdir()?;
    let storage = FileAuthStorage::new(home.path().to_path_buf());
    storage.save(&api_key_auth())?;
    let error = login_with_bedrock_api_key(
        home.path(),
        "key",
        "us-east-1",
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    )
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
    assert_eq!(storage.load()?, Some(api_key_auth()));
    Ok(())
}

#[tokio::test]
#[serial(codex_auth_env)]
async fn logout_removes_bedrock_auth() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let storage = FileAuthStorage::new(codex_home.path().to_path_buf());
    std::fs::write(
        codex_home.path().join("auth.json"),
        serde_json::to_vec(
            &serde_json::json!({"auth_mode": "bedrockApiKey", "bedrock_api_key": {"api_key": "bedrock-api-key-test", "region": "us-east-1"}}),
        )?,
    )?;
    let auth_manager = AuthManager::new(
        codex_home.path().to_path_buf(),
        /*enable_codex_api_key_env*/ false,
        AuthCredentialsStoreMode::File,
        /*forced_chatgpt_workspace_id*/ None,
        /*chatgpt_base_url*/ None,
        AuthKeyringBackendKind::default(),
        crate::test_support::transport_default_auth_route_config(),
    )
    .await;

    assert!(auth_manager.logout().await?);

    assert_eq!(storage.load()?, None);
    assert_eq!(auth_manager.auth_cached(), None);
    Ok(())
}

#[tokio::test]
#[serial(codex_auth_env)]
async fn access_keys_auth_round_trips_and_logs_out() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let storage = FileAuthStorage::new(codex_home.path().to_path_buf());
    std::fs::write(
        codex_home.path().join("auth.json"),
        serde_json::to_vec(
            &serde_json::json!({"auth_mode": "bedrockAccessKeys", "bedrock_access_keys": {"access_key_id": "access-key-id", "secret_access_key": "secret-access-key", "session_token": Some("session-token")}}),
        )?,
    )?;
    let auth_manager = AuthManager::new(
        codex_home.path().to_path_buf(),
        /*enable_codex_api_key_env*/ false,
        AuthCredentialsStoreMode::File,
        Some(vec!["allowed-workspace".to_string()]),
        /*chatgpt_base_url*/ None,
        AuthKeyringBackendKind::default(),
        crate::test_support::transport_default_auth_route_config(),
    )
    .await;

    assert_eq!(auth_manager.auth_mode(), Some(AuthMode::BedrockAccessKeys));
    assert_eq!(
        auth_manager.auth_cached().and_then(|auth| match auth {
            CodexAuth::BedrockAccessKeys(auth) => Some(auth),
            CodexAuth::ApiKey(_)
            | CodexAuth::Chatgpt(_)
            | CodexAuth::ChatgptAuthTokens(_)
            | CodexAuth::Headers(_)
            | CodexAuth::AgentIdentity(_)
            | CodexAuth::PersonalAccessToken(_)
            | CodexAuth::BedrockApiKey(_) => None,
        }),
        Some(crate::auth::BedrockAccessKeysAuth {
            access_key_id: "access-key-id".to_string(),
            secret_access_key: "secret-access-key".to_string(),
            session_token: Some("session-token".to_string()),
        })
    );
    let auth_change_rx = auth_manager.auth_change_receiver();
    let auth_revision = *auth_change_rx.borrow();
    let mut auth_without_mode = storage.load()?.expect("access keys should be stored");
    auth_without_mode.auth_mode = None;
    storage.save(&auth_without_mode)?;
    assert!(!auth_manager.reload().await);
    assert_eq!(*auth_change_rx.borrow(), auth_revision);
    assert!(auth_manager.logout().await?);
    assert_eq!(storage.load()?, None);
    Ok(())
}

#[tokio::test]
#[serial(codex_auth_env)]
async fn bedrock_only_auth_storage_creates_primary_auth() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let storage = FileAuthStorage::new(codex_home.path().to_path_buf());
    storage.save(&bedrock_only_auth())?;

    let auth_manager = AuthManager::new(
        codex_home.path().to_path_buf(),
        /*enable_codex_api_key_env*/ false,
        AuthCredentialsStoreMode::File,
        /*forced_chatgpt_workspace_id*/ None,
        /*chatgpt_base_url*/ None,
        AuthKeyringBackendKind::default(),
        crate::test_support::transport_default_auth_route_config(),
    )
    .await;

    assert_eq!(auth_manager.auth_mode(), Some(AuthMode::BedrockApiKey));
    assert_eq!(
        auth_manager.auth_cached().and_then(|auth| match auth {
            CodexAuth::BedrockApiKey(auth) => Some(auth),
            CodexAuth::ApiKey(_)
            | CodexAuth::Chatgpt(_)
            | CodexAuth::ChatgptAuthTokens(_)
            | CodexAuth::Headers(_)
            | CodexAuth::AgentIdentity(_)
            | CodexAuth::PersonalAccessToken(_)
            | CodexAuth::BedrockAccessKeys(_) => None,
        }),
        Some(bedrock_auth())
    );
    Ok(())
}

#[tokio::test]
async fn login_with_api_key_clears_bedrock_api_key() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let storage = FileAuthStorage::new(codex_home.path().to_path_buf());
    std::fs::write(
        codex_home.path().join("auth.json"),
        serde_json::to_vec(
            &serde_json::json!({"auth_mode": "bedrockApiKey", "bedrock_api_key": {"api_key": "bedrock-api-key-test", "region": "us-east-1"}}),
        )?,
    )?;

    crate::auth::login_with_api_key(
        codex_home.path(),
        "sk-test-key",
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    )?;

    assert_eq!(storage.load()?, Some(api_key_auth()));
    Ok(())
}
