use anyhow::Result;
use app_test_support::TestAppServer;
use codex_app_server_protocol::BedrockCheckGovCloudRequirementsParams;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn checks_current_provider_after_login_without_restart() -> Result<()> {
    let home = TempDir::new()?;
    std::fs::write(home.path().join("config.toml"), "")?;
    std::fs::write(
        home.path().join("auth.json"),
        serde_json::to_vec(&json!({
            "auth_mode": "bedrockApiKey",
            "bedrock_api_key": {"api_key": "test-key", "region": "us-gov-west-1"}
        }))?,
    )?;
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let response: serde_json::Value = server
        .request(
            |request_id| ClientRequest::BedrockCheckGovCloudRequirements {
                request_id,
                params: BedrockCheckGovCloudRequirementsParams {},
            },
        )
        .await?;
    assert_eq!(response, json!({"isGovCloud": false, "shouldWarn": false}));
    std::fs::write(
        home.path().join("config.toml"),
        "model_provider = 'amazon-bedrock'\n",
    )?;
    let id = server
        .send_request("account/bedrock/checkGovCloudRequirements", Some(json!({})))
        .await?;
    let error = server
        .read_stream_until_error_message(RequestId::Integer(id))
        .await?;
    assert_eq!(
        error.error.message,
        "Amazon Bedrock is unsupported in this build."
    );
    std::fs::write(home.path().join("config.toml"), "")?;
    let response: serde_json::Value = server
        .request(
            |request_id| ClientRequest::BedrockCheckGovCloudRequirements {
                request_id,
                params: BedrockCheckGovCloudRequirementsParams {},
            },
        )
        .await?;
    assert_eq!(response, json!({"isGovCloud": false, "shouldWarn": false}));
    Ok(())
}

#[tokio::test]
async fn check_preserves_current_login_when_saved_credentials_are_unreadable() -> Result<()> {
    let home = TempDir::new()?;
    std::fs::write(
        home.path().join("config.toml"),
        "model_provider = 'amazon-bedrock'\ncli_auth_credentials_store = 'file'\n",
    )?;
    std::fs::write(
        home.path().join("auth.json"),
        serde_json::to_vec(&json!({
            "auth_mode": "bedrockApiKey",
            "bedrock_api_key": {"api_key": "test-key", "region": "us-gov-west-1"}
        }))?,
    )?;
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let path = home.path().join("auth.json");
    std::fs::write(&path, "invalid auth")?;
    let id = server
        .send_request("account/bedrock/checkGovCloudRequirements", Some(json!({})))
        .await?;
    let error = server
        .read_stream_until_error_message(RequestId::Integer(id))
        .await?;
    assert_eq!(
        error.error.message,
        "Amazon Bedrock is unsupported in this build."
    );
    assert_eq!(std::fs::read_to_string(path)?, "invalid auth");
    Ok(())
}
