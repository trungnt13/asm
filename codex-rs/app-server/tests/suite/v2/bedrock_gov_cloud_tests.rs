use anyhow::Result;
use app_test_support::TestAppServer;
use codex_app_server_protocol::BedrockCheckGovCloudRequirementsParams;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

async fn check(server: &mut TestAppServer) -> Result<Value> {
    server
        .request(
            |request_id| ClientRequest::BedrockCheckGovCloudRequirements {
                request_id,
                params: BedrockCheckGovCloudRequirementsParams {},
            },
        )
        .await
}

#[tokio::test]
async fn preserves_non_bedrock_response_and_configuration_errors() -> Result<()> {
    let home = TempDir::new()?;
    std::fs::write(home.path().join("config.toml"), "")?;
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    assert_eq!(
        check(&mut server).await?,
        json!({"isGovCloud": false, "shouldWarn": false})
    );
    std::fs::write(home.path().join("requirements.toml"), "[invalid toml")?;
    let request_id = server
        .send_request("account/bedrock/checkGovCloudRequirements", Some(json!({})))
        .await?;
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(/*secs*/ 10),
        server.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert_eq!((error.error.code, error.error.data), (-32603, None));
    assert!(
        error
            .error
            .message
            .starts_with("failed to load configuration:")
    );
    Ok(())
}
