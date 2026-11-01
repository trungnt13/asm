use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ThreadStartParams;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::time::Duration;
use tempfile::TempDir;
use test_case::test_case;
use tokio::time::timeout;

const TIMEOUT: Duration = Duration::from_secs(15);

#[test_case("ordinary"; "saved ordinary fork")]
#[test_case("saved_with_block"; "saved fork with copied side instructions")]
#[test_case("near_match"; "unknown client wording")]
#[test_case("prediction"; "composer prediction")]
#[tokio::test]
async fn desktop_side_defaults_leave_other_forks_unchanged(kind: &str) -> Result<()> {
    let server = responses::start_mock_server().await;
    let response = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_assistant_message("msg", "done"),
            responses::ev_completed("resp"),
        ]),
    )
    .await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri())
        .with_root_config(
            "side_model = \"mock-model-2\"\nside_reasoning_effort = \"low\"\nside_service_tier = \"priority\"",
        )
        .write(home.path())?;
    let mut mcp = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let parent = mcp.start_thread(ThreadStartParams::default()).await?;
    timeout(
        TIMEOUT,
        mcp.start_turn_and_wait_for_completion(serde_json::from_value(json!({
            "threadId":parent.thread.id,"input":[{"type":"text","text":"parent"}]
        }))?),
    )
    .await??;
    mcp.clear_message_buffer();
    let instructions = std::fs::read_to_string(codex_utils_cargo_bin::find_resource!(
        "src/request_processors/desktop_side_instructions.txt"
    )?)?;
    let boundary = std::fs::read_to_string(codex_utils_cargo_bin::find_resource!(
        "src/request_processors/desktop_side_boundary.txt"
    )?)?;
    let mut params = json!({"threadId":parent.thread.id,"excludeTurns":true});
    match kind {
        "near_match" => {
            params["ephemeral"] = json!(true);
            params["developerInstructions"] = json!(format!("{instructions} altered"));
        }
        "saved_with_block" => params["developerInstructions"] = json!(instructions),
        "prediction" => {
            params["ephemeral"] = json!(true);
            params["experimentalPredictionMode"] = json!(true);
            params["threadSource"] = json!("composer_predictions");
        }
        "ordinary" => {}
        other => panic!("unknown scenario: {other}"),
    }
    let request = mcp.send_raw_request("thread/fork", Some(params)).await?;
    let fork: Value = timeout(TIMEOUT, mcp.read_response(request)).await??;
    assert_eq!(
        (
            fork["model"].clone(),
            fork["reasoningEffort"].clone(),
            fork["serviceTier"].clone()
        ),
        (
            json!(parent.model),
            json!(parent.reasoning_effort),
            json!(parent.service_tier)
        )
    );
    let request = mcp
        .send_raw_request(
            "thread/inject_items",
            Some(json!({"threadId":fork["thread"]["id"],"items":[{
                "type":"message","role":"user",
                "content":[{"type":"input_text","text":boundary}]
            }]})),
        )
        .await?;
    let _: Value = timeout(TIMEOUT, mcp.read_response(request)).await??;
    assert!(
        !mcp.pending_notification_methods()
            .iter()
            .any(|method| method == "thread/settings/updated"),
        "ordinary forks must not emit a side settings synchronization"
    );
    assert_eq!(
        response.single_request().body_json()["model"],
        json!(parent.model)
    );
    Ok(())
}

#[tokio::test]
async fn desktop_side_unset_effort_preserves_null_parent_effort() -> Result<()> {
    let server = responses::start_mock_server().await;
    let body = responses::sse(vec![
        responses::ev_assistant_message("msg", "done"),
        responses::ev_completed("resp"),
    ]);
    let requests = responses::mount_sse_sequence(&server, vec![body.clone(), body]).await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri())
        .with_model("gpt-6-astra")
        .with_root_config("model_reasoning_effort = \"high\"\nside_service_tier = \"priority\"")
        .write(home.path())?;
    app_test_support::write_models_cache(home.path()).await?;
    let instructions = std::fs::read_to_string(codex_utils_cargo_bin::find_resource!(
        "src/request_processors/desktop_side_instructions.txt"
    )?)?;
    let mut mcp = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let ordinary = mcp.start_thread(ThreadStartParams::default()).await?;
    let mut turn = json!({
        "threadId":ordinary.thread.id,"input":[{"type":"text","text":"question"}],
        "model":null,"effort":null,"serviceTier":null,
        "collaborationMode":{"mode":"default","settings":{"model":"gpt-6-astra",
            "reasoning_effort":null,"developer_instructions":null}}
    });
    timeout(
        TIMEOUT,
        mcp.start_turn_and_wait_for_completion(serde_json::from_value(turn.clone())?),
    )
    .await??;
    let side = mcp
        .start_thread(serde_json::from_value(json!({
            "developerInstructions":instructions,"model":"gpt-6-astra","serviceTier":null
        }))?)
        .await?;
    let inject = mcp
        .send_raw_request(
            "thread/inject_items",
            Some(json!({
                "threadId":side.thread.id,"items":[{"type":"message","role":"user",
                "content":[{"type":"input_text","text":"Generated parent context"}]}]
            })),
        )
        .await?;
    let _: Value = timeout(TIMEOUT, mcp.read_response(inject)).await??;
    turn["threadId"] = json!(side.thread.id);
    timeout(
        TIMEOUT,
        mcp.start_turn_and_wait_for_completion(serde_json::from_value(turn)?),
    )
    .await??;
    let baseline = requests.requests()[0].body_json();
    let body = requests.requests()[1].body_json();
    assert_eq!(
        (
            body["reasoning"]["effort"].clone(),
            body["service_tier"].clone()
        ),
        (baseline["reasoning"]["effort"].clone(), json!("priority"))
    );
    Ok(())
}
