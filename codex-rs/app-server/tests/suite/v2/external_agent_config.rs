use anyhow::Result;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ExternalAgentConfigImportHistoriesReadResponse;
use codex_app_server_protocol::ExternalAgentConfigImportHistoryRecordResponse;
use pretty_assertions::assert_eq;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::timeout;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
#[tokio::test]
async fn external_agent_config_records_externally_completed_import_history() -> Result<()> {
    let codex_home = TempDir::new()?;
    let sqlite_home = TempDir::new()?;
    let home_dir = codex_home.path().display().to_string();
    let sqlite_home_dir = sqlite_home.path().display().to_string();
    let mut mcp = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_auto_env()
        .with_env_overrides(&[
            ("HOME", Some(home_dir.as_str())),
            ("CODEX_SQLITE_HOME", Some(sqlite_home_dir.as_str())),
        ])
        .build_initialized_with_timeout(DEFAULT_TIMEOUT)
        .await?;

    let request_id = mcp
        .send_raw_request(
            "externalAgentConfig/import/recordHistory",
            Some(serde_json::json!({
                "providerId": "external-provider",
                "itemTypeResults": [{
                    "itemType": "SESSIONS",
                    "successes": [{
                        "itemType": "SESSIONS",
                        "cwd": "/repo",
                        "source": "/source/session.jsonl",
                        "target": "thread-1",
                    }],
                    "failures": [],
                }],
            })),
        )
        .await?;
    let record_response: ExternalAgentConfigImportHistoryRecordResponse =
        timeout(DEFAULT_TIMEOUT, mcp.read_response(request_id)).await??;
    assert!(!record_response.import_id.is_empty());

    let request_id = mcp
        .send_raw_request(
            "externalAgentConfig/import/readHistories",
            /*params*/ None,
        )
        .await?;
    let history_response: ExternalAgentConfigImportHistoriesReadResponse =
        timeout(DEFAULT_TIMEOUT, mcp.read_response(request_id)).await??;
    let entry = history_response
        .data
        .iter()
        .find(|entry| entry.import_id == record_response.import_id)
        .expect("externally completed import history entry should be available");
    assert_eq!(entry.provider_id.as_deref(), Some("external-provider"));
    assert!(entry.completed_at_ms > 0);
    assert_eq!(
        serde_json::to_value(&entry.successes)?,
        serde_json::json!([{
            "itemType": "SESSIONS",
            "cwd": "/repo",
            "source": "/source/session.jsonl",
            "target": "thread-1",
            "title": null,
        }])
    );
    assert_eq!(entry.failures, Vec::new());

    Ok(())
}
