use anyhow::Result;
use app_test_support::TestAppServer;
use codex_app_server_protocol::RequestId;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn discover_bedrock_profiles_and_environment_credentials() -> Result<()> {
    let home = TempDir::new()?;
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let id = server
        .send_request("account/bedrock/discover", Some(json!({})))
        .await?;
    let error = server
        .read_stream_until_error_message(RequestId::Integer(id))
        .await?;
    assert_eq!(
        error.error.message,
        "Amazon Bedrock is unsupported in this build."
    );
    Ok(())
}

#[tokio::test]
async fn setup_bedrock_profile_and_environment() -> Result<()> {
    let home = TempDir::new()?;
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    for params in [
        json!({"type": "profile", "profile": "engineering", "region": "us-west-2"}),
        json!({"type": "environment", "region": "us-west-2"}),
    ] {
        let id = server
            .send_request("account/bedrock/setup", Some(params))
            .await?;
        let error = server
            .read_stream_until_error_message(RequestId::Integer(id))
            .await?;
        assert_eq!(
            error.error.message,
            "Amazon Bedrock is unsupported in this build."
        );
    }
    assert!(!home.path().join("auth.json").exists());
    assert!(!home.path().join("config.toml").exists());
    Ok(())
}
