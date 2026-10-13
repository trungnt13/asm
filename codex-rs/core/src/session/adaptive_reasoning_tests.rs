use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;
use test_case::test_case;
use tokio::time::timeout;

#[path = "adaptive_reasoning_test_support.rs"]
mod support;
use support::DecisionProxy;
use support::adaptive_fixture;
use support::user_task;

#[test_case("success"; "selected_effort_precedes_each_request")]
#[test_case("failure"; "http_failure_continues_with_current_effort")]
#[test_case("timeout"; "timeout_continues_with_current_effort")]
#[tokio::test]
async fn basic_regular_turn_waits_for_each_decision_before_sampling(scenario: &str) {
    use crate::tasks::RegularTask;
    use codex_protocol::protocol::TurnAbortReason;
    use core_test_support::responses;

    let mut proxy = DecisionProxy::start().await;
    let primary = responses::start_mock_server().await;
    let requests = responses::mount_sse_sequence(
        &primary,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "plan-call",
                    "update_plan",
                    &json!({
                        "plan": [{"step":"Check the arithmetic", "status":"completed"}]
                    })
                    .to_string(),
                ),
                responses::ev_completed("plan-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("final-message", "The result is four."),
                responses::ev_completed("final-response"),
            ]),
        ],
    )
    .await;
    let (session, turn, _events) = adaptive_fixture(Arc::clone(&proxy.client), |config| {
        config.model_provider.base_url = Some(format!("{}/v1", primary.uri()));
        config.model_provider.supports_websockets = false;
        config.update_plan_enabled = true;
        if scenario == "timeout" {
            config.adaptive_reasoning.decision_timeout_ms = 500;
        }
    })
    .await;
    session.abort_all_tasks(TurnAbortReason::Interrupted).await;
    let task = "Check a harmless arithmetic result.";
    let mut input = user_task(task);
    let TurnInput::UserInput { metadata, .. } = &mut input[0] else {
        unreachable!("accepted user task")
    };
    metadata.acceptance_order = Some(session.reserve_user_input_order().await);
    session
        .spawn_task(Arc::clone(&turn), input, RegularTask::new())
        .await;
    let first = proxy.request().await;
    assert!(
        first.body["input"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains(task),
        "the real Decisions request contains this accepted task before main inference"
    );
    tokio::time::sleep(Duration::from_millis(/*millis*/ 100)).await;
    assert!(
        requests.requests().is_empty(),
        "main inference waits for initial decision"
    );
    first
        .reply
        .send((
            200,
            json!({"answers": [{"type":"choice", "name":"reasoning_effort", "choice":"high"}]}),
        ))
        .unwrap();
    let second = proxy.request().await;
    tokio::time::sleep(Duration::from_millis(/*millis*/ 100)).await;
    assert_eq!(
        requests.requests().len(),
        1,
        "tool continuation waits for its decision"
    );
    assert_eq!(
        turn.next_step_settings.load().effective_reasoning_effort(),
        Some(ReasoningEffort::High)
    );
    assert!(
        second.body["input"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Plan updated")
    );
    let mut reply = Some(second.reply);
    match scenario {
        "success" => reply
            .take()
            .unwrap()
            .send((
                200,
                json!({"answers": [{"type":"choice", "name":"reasoning_effort", "choice":"low"}]}),
            ))
            .unwrap(),
        "failure" => reply
            .take()
            .unwrap()
            .send((503, json!({"error":"synthetic"})))
            .unwrap(),
        "timeout" => {}
        _ => unreachable!("defined test scenarios"),
    }
    timeout(Duration::from_secs(/*secs*/ 5), async {
        while session.active_turn.lock().await.is_some() {
            tokio::time::sleep(Duration::from_millis(/*millis*/ 1)).await;
        }
    })
    .await
    .expect("normal turn continues after decision success or bounded fallback");
    drop(reply);
    let requests = requests.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1].function_call_output_text("plan-call"),
        Some("Plan updated".to_string())
    );
    let updates = requests
        .iter()
        .map(|request| {
            request
                .input()
                .into_iter()
                .filter(|item| item["type"] == "configuration_update")
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let high = json!({"type":"configuration_update", "reasoning":{"effort":"high"}});
    let low = json!({"type":"configuration_update", "reasoning":{"effort":"low"}});
    let expected = match scenario {
        "success" => vec![vec![high.clone()], vec![high, low]],
        "failure" | "timeout" => vec![vec![high.clone()], vec![high]],
        _ => unreachable!("defined test scenarios"),
    };
    assert_eq!(updates, expected);
    assert_eq!(
        requests
            .iter()
            .map(|request| request.body_json()["reasoning"]["effort"].clone())
            .collect::<Vec<_>>(),
        vec![json!("high"), json!("high")]
    );
    let first_input = requests[0].input();
    let second_input = requests[1].input();
    assert_eq!(&second_input[..first_input.len()], first_input.as_slice());
    assert_eq!(
        session
            .state
            .lock()
            .await
            .session_configuration
            .step_settings
            .collaboration_mode
            .settings
            .reasoning_effort,
        Some(ReasoningEffort::Low)
    );
    assert!(
        proxy.requests.try_recv().is_err(),
        "two main steps require exactly two real decisions"
    );
    session.stop_adaptive_reasoning().await;
}
