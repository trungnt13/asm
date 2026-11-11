use super::*;
use crate::session::tests::make_session_and_context;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn native_messaging_capture_binds_and_rebinds_without_api_tools() {
    let (mut session, mut turn) = make_session_and_context().await;
    let manager = crate::ThreadManager::with_models_provider_for_tests(
        codex_login::CodexAuth::from_api_key("dummy"),
        turn.config.model_provider.clone(),
    );
    let control = manager.agent_control();
    session.services.local_agent_runtime = control.runtime.clone();
    session.services.agent_control = Arc::new(control);
    turn.parent_thread_id = Some(ThreadId::new());
    turn.session_source = SessionSource::SubAgent(SubAgentSource::Other("native-test".into()));
    turn.multi_agent_version = MultiAgentVersion::V2;
    let config = Arc::make_mut(&mut turn.config);
    config.multi_agent_v2.disable_native_tools = false;
    config.multi_agent_v2.disable_direct_message = false;
    config.multi_agent_v2.wait_agent_enabled = false;
    config.multi_agent_v2.tool_namespace = Some("agents".into());
    let session = Arc::new(session);
    let turn = Arc::new(turn);
    let cancel = CancellationToken::new();
    let mut bridge = ExternalMessageBridge::start(Duration::from_millis(/*millis*/ 100))
        .await
        .expect("start local bridge");
    let client = reqwest::Client::new();
    let mut previous = None;
    for _ in 0..2 {
        let step = session
            .capture_native_messaging_step_context(Arc::clone(&turn), &cancel)
            .await
            .expect("capture native step");
        assert!(step.tool_router.model_visible_specs().is_empty());
        bridge
            .set_turn(Arc::clone(&session), step, cancel.clone())
            .await
            .expect("bind native turn");
        let response: serde_json::Value = client
            .post(format!("http://{}/tools", bridge.state.authority))
            .bearer_auth(&bridge.state.token)
            .json(&json!({}))
            .send()
            .await
            .expect("read exposed tools")
            .error_for_status()
            .expect("authorized tools response")
            .json()
            .await
            .expect("tools JSON");
        let mut names = response["tools"]
            .as_array()
            .expect("tool definitions")
            .iter()
            .map(|tool| tool["name"].as_str().expect("tool name"))
            .collect::<Vec<_>>();
        names.sort_unstable();
        assert_eq!(
            names,
            vec![
                "agents__followup_task",
                "agents__interrupt_agent",
                "agents__list_agents",
                "agents__send_message",
            ]
        );
        let result: serde_json::Value = client
            .post(format!("http://{}/call", bridge.state.authority))
            .bearer_auth(&bridge.state.token)
            .json(&json!({"name":"agents__list_agents","arguments":{}}))
            .send()
            .await
            .expect("invoke canonical tool")
            .error_for_status()
            .expect("tool response")
            .json()
            .await
            .expect("tool output JSON");
        assert_eq!(result["isError"], json!(false), "{result}");
        if let Some(previous) = previous {
            assert_eq!(response, previous);
        }
        previous = Some(response);
        bridge.clear_turn().await.expect("clear native turn");
    }
    bridge.shutdown().await.expect("stop local bridge");
    let api_step = session
        .capture_speculative_step_context(turn, &cancel)
        .await
        .expect("capture API step after native binding");
    assert!(!api_step.tool_router.model_visible_specs().is_empty());
}
