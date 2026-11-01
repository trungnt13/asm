use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::write_models_cache;
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

async fn rpc(mcp: &mut TestAppServer, method: &str, params: Value) -> Result<Value> {
    let id = mcp.send_raw_request(method, Some(params)).await?;
    timeout(TIMEOUT, mcp.read_response(id)).await?
}

fn shipped_text(name: &str) -> Result<String> {
    let relative = format!("src/request_processors/desktop_side_{name}.txt");
    let path = codex_utils_cargo_bin::find_resource!(relative)?;
    Ok(std::fs::read_to_string(path)?)
}

async fn inject(mcp: &mut TestAppServer, id: &str, text: &str) -> Result<()> {
    rpc(
        mcp,
        "thread/inject_items",
        json!({"threadId":id,"items":[{
            "type":"message","role":"user","content":[{"type":"input_text","text":text}]
        }]}),
    )
    .await?;
    Ok(())
}

#[test_case("start", false; "persistent start")]
#[test_case("start", true; "picker before initialization")]
#[test_case("ephemeral", false; "temporary fork")]
#[test_case("attached", false; "attached persistent fork")]
#[tokio::test]
async fn desktop_side_defaults_sync_and_picker_choices_persist(
    kind: &str,
    pick_before_injection: bool,
) -> Result<()> {
    let server = responses::start_mock_server().await;
    let body = responses::sse(vec![
        responses::ev_assistant_message("msg", "done"),
        responses::ev_completed("resp"),
    ]);
    let requests =
        responses::mount_sse_sequence(&server, vec![body.clone(), body.clone(), body]).await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri()).with_root_config(
        "model_reasoning_effort = \"high\"\nside_model = \"gpt-6-astra\"\nside_reasoning_effort = \"low\"\nside_service_tier = \"priority\""
    ).write(home.path())?;
    write_models_cache(home.path()).await?;
    let mut mcp = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let parent = mcp
        .start_thread(ThreadStartParams {
            history_mode: Some(codex_app_server_protocol::ThreadHistoryMode::Paginated),
            ..Default::default()
        })
        .await?;
    timeout(
        TIMEOUT,
        mcp.start_turn_and_wait_for_completion(serde_json::from_value(json!({
            "threadId":parent.thread.id,"input":[{"type":"text","text":"parent"}]
        }))?),
    )
    .await??;
    mcp.clear_message_buffer();
    let developer = format!("Base instructions\n\n{}", shipped_text("instructions")?);
    let side = if kind == "start" {
        serde_json::to_value(
            mcp.start_thread(serde_json::from_value(json!({
                "developerInstructions":developer,"model":"mock-model","serviceTier":null
            }))?)
            .await?,
        )?
    } else {
        rpc(&mut mcp, "thread/fork", json!({
            "threadId":parent.thread.id,"developerInstructions":developer,
            "ephemeral":kind == "ephemeral","attachToSource":kind == "attached","excludeTurns":true
        })).await?
    };
    let id = side["thread"]["id"].as_str().unwrap();
    let expected_model = if pick_before_injection {
        "mock-model-4"
    } else {
        "gpt-6-astra"
    };
    if pick_before_injection {
        rpc(
            &mut mcp,
            "thread/settings/update",
            json!({"threadId":id,"model":expected_model}),
        )
        .await?;
        let _: Value = timeout(TIMEOUT, mcp.read_notification("thread/settings/updated")).await??;
    }
    let boundary = if kind == "start" {
        "Generated parent reference context".to_string()
    } else {
        shipped_text("boundary")?
    };
    inject(&mut mcp, id, &boundary).await?;
    let updated: Value =
        timeout(TIMEOUT, mcp.read_notification("thread/settings/updated")).await??;
    let settings = &updated["threadSettings"];
    assert_eq!(
        (
            settings["model"].clone(),
            settings["effort"].clone(),
            settings["serviceTier"].clone()
        ),
        (json!(expected_model), json!("low"), json!("priority"))
    );
    let mode = if kind == "start" {
        json!({"mode":"default","settings":{"model":"mock-model","reasoning_effort":null,"developer_instructions":null}})
    } else {
        settings["collaborationMode"].clone()
    };
    timeout(TIMEOUT, mcp.start_turn_and_wait_for_completion(serde_json::from_value(json!({
        "threadId":id,"input":[{"type":"text","text":"side"}],"model":null,"effort":null,
        "collaborationMode":mode,"serviceTier":if kind == "start" { Value::Null } else { json!("priority") }
    }))?)).await??;
    let first_side = requests.requests()[1].body_json();
    let expected_tier = (!pick_before_injection).then_some("priority");
    assert_eq!(
        (
            first_side["model"].clone(),
            first_side["reasoning"]["effort"].clone(),
            first_side["service_tier"].clone()
        ),
        (json!(expected_model), json!("low"), json!(expected_tier))
    );
    mcp.clear_message_buffer();
    rpc(
        &mut mcp,
        "thread/settings/update",
        json!({"threadId":id,"model":"mock-model-3","effort":"medium","serviceTier":null}),
    )
    .await?;
    let picked: Value =
        timeout(TIMEOUT, mcp.read_notification("thread/settings/updated")).await??;
    let picked = &picked["threadSettings"];
    assert_eq!(
        (
            picked["model"].clone(),
            picked["effort"].clone(),
            picked["serviceTier"].clone()
        ),
        (json!("mock-model-3"), json!("medium"), json!("default"))
    );
    inject(&mut mcp, id, &boundary).await?;
    timeout(
        TIMEOUT,
        mcp.start_turn_and_wait_for_completion(serde_json::from_value(json!({
            "threadId":id,"input":[{"type":"text","text":"chosen"}],"model":null,"effort":null,
            "collaborationMode":picked["collaborationMode"],"serviceTier":"default"
        }))?),
    )
    .await??;
    assert_eq!(
        requests.requests()[2].body_json()["model"],
        json!("mock-model-3")
    );
    if kind != "ephemeral" {
        mcp.shutdown_gracefully().await?;
        let mut mcp = TestAppServer::builder()
            .with_codex_home(home.path())
            .build_initialized()
            .await?;
        let resumed = rpc(
            &mut mcp,
            "thread/resume",
            json!({"threadId":id,"excludeTurns":true}),
        )
        .await?;
        assert_eq!(
            (
                resumed["model"].clone(),
                resumed["reasoningEffort"].clone(),
                resumed["serviceTier"].clone()
            ),
            (json!("mock-model-3"), json!("medium"), json!("default"))
        );
    }
    Ok(())
}
