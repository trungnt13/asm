//! Exercises registered external backends through native V2 child turns.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use anyhow::Result;
use codex_core::config::AgentRoleConfig;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ExternalAgentBackend;
use codex_extension_api::ExternalAgentEvent;
use codex_extension_api::ExternalAgentInput;
use codex_extension_api::ExternalAgentLaunch;
use codex_extension_api::ExternalAgentLaunchError;
use codex_extension_api::ExternalAgentRuntime;
use codex_extension_api::ExternalObservation;
use codex_features::Feature;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::EventMsg;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call_with_namespace;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use futures::future::BoxFuture;
use pretty_assertions::assert_eq;
use serde_json::json;
use test_case::test_case;

const STARTUP_ERROR: &str = "fixture startup failed: executable unavailable";

#[derive(Clone, Copy, Debug)]
enum LaunchOutcome {
    Rejected,
    Unsettled,
    Completed,
}

struct FixtureBackend {
    outcome: LaunchOutcome,
    opens: AtomicUsize,
}

impl ExternalAgentBackend for FixtureBackend {
    fn open(
        &self,
        launch: ExternalAgentLaunch,
    ) -> BoxFuture<'_, Result<Arc<dyn ExternalAgentRuntime>, ExternalAgentLaunchError>> {
        self.opens.fetch_add(/*val*/ 1, Ordering::SeqCst);
        assert_eq!(launch.model, "external-test-model");
        Box::pin(async move {
            match self.outcome {
                LaunchOutcome::Rejected => {
                    Err(ExternalAgentLaunchError::Rejected(STARTUP_ERROR.into()))
                }
                LaunchOutcome::Unsettled => {
                    Err(ExternalAgentLaunchError::Unsettled(STARTUP_ERROR.into()))
                }
                LaunchOutcome::Completed => {
                    Ok(Arc::new(FixtureRuntime(Mutex::new(/*t*/ None)))
                        as Arc<dyn ExternalAgentRuntime>)
                }
            }
        })
    }
}

struct FixtureRuntime(Mutex<Option<ExternalAgentEvent>>);

impl ExternalAgentRuntime for FixtureRuntime {
    fn submit(&self, input: ExternalAgentInput) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async move {
            *self.0.lock().expect("fixture event lock") = Some(ExternalAgentEvent {
                id: "fixture-completed".into(),
                turn_id: input.turn_id,
                kind: ExternalObservation::ReadyToFinish {
                    text: "external response".into(),
                },
            });
            Ok(())
        })
    }

    fn next_event(&self) -> BoxFuture<'_, Result<Option<ExternalAgentEvent>, String>> {
        Box::pin(async { Ok(self.0.lock().expect("fixture event lock").take()) })
    }

    fn interrupt<'a>(&'a self, _turn_id: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }

    fn shutdown(&self) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }
}

#[test_case(LaunchOutcome::Rejected; "clean rejection retains cause without quarantine")]
#[test_case(LaunchOutcome::Unsettled; "unknown launch retains cause and quarantine")]
#[test_case(LaunchOutcome::Completed; "external completion needs no native child request")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn v2_external_backend_turn(outcome: LaunchOutcome) -> Result<()> {
    let server = start_mock_server().await;
    let requests = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("spawn"),
                ev_function_call_with_namespace(
                    "spawn-external",
                    "collaboration",
                    "spawn_agent",
                    &json!({
                        "task_name": "external", "agent_type": "fixture",
                        "external_message": "Reply without using native inference",
                        "fork_turns": "none",
                    })
                    .to_string(),
                ),
                ev_completed("spawn"),
            ]),
            sse(vec![
                ev_assistant_message("root-done", "spawned"),
                ev_completed("root-done"),
            ]),
        ],
    )
    .await;
    let backend = Arc::new(FixtureBackend {
        outcome,
        opens: AtomicUsize::default(),
    });
    let mut extensions = ExtensionRegistryBuilder::new();
    extensions
        .register_external_agent_backend("fixture".into(), backend.clone())
        .expect("register fixture backend");
    let test = test_codex()
        .with_model("gpt-5.6-sol")
        .with_extensions(Arc::new(extensions.build()))
        .with_config(|config| {
            for feature in [Feature::Collab, Feature::MultiAgentV2] {
                config.features.enable(feature).expect("enable V2");
            }
            config
                .permissions
                .set_permission_profile(PermissionProfile::Disabled)
                .expect("allow external runtime");
            let role_path = config.codex_home.join("external-fixture.toml");
            std::fs::write(&role_path, "model = \"external-test-model\"\n")
                .expect("write external role");
            config.agent_roles.insert(
                "fixture".into(),
                AgentRoleConfig {
                    execution_backend: Some("fixture".into()),
                    description: Some("Fixture external backend".into()),
                    config_file: Some(role_path.to_path_buf()),
                    nickname_candidates: None,
                },
            );
        })
        .build_with_auto_env(&server)
        .await?;
    let mut created = test.thread_manager.subscribe_thread_created();
    test.submit_turn("Spawn the external fixture").await?;
    assert_eq!(
        test.thread_manager.list_thread_ids().await.len(),
        2,
        "{}",
        requests
            .function_call_output_text("spawn-external")
            .unwrap_or_default()
    );
    let child = test
        .thread_manager
        .get_thread(created.recv().await?)
        .await?;
    let completion =
        wait_for_event(&child, |event| matches!(event, EventMsg::TurnComplete(_))).await;
    let EventMsg::TurnComplete(completion) = completion else {
        unreachable!()
    };
    match outcome {
        LaunchOutcome::Rejected | LaunchOutcome::Unsettled => {
            let error = completion.error.expect("failed external turn");
            assert!(error.message.contains(STARTUP_ERROR), "{}", error.message);
            assert_eq!(
                error.message.contains("quarantined"),
                matches!(outcome, LaunchOutcome::Unsettled)
            );
        }
        LaunchOutcome::Completed => {
            assert_eq!(
                (completion.error, completion.last_agent_message),
                (None, Some("external response".into()))
            );
        }
    }
    assert_eq!(backend.opens.load(Ordering::SeqCst), 1);
    assert_eq!(
        requests.requests().len(),
        2,
        "only the parent invokes native inference"
    );
    let shutdown = test
        .thread_manager
        .request_agent_tree_shutdown(test.session_configured.thread_id)
        .await?;
    assert_eq!(
        shutdown.wait_detailed().await.is_err(),
        matches!(outcome, LaunchOutcome::Unsettled)
    );
    Ok(())
}
