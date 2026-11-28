//! OpenAI-only native agent review preserves encrypted history across reviews and resumes.

use codex_core::config::Constrained;
use codex_features::Feature;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::AskForApproval;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::json;
use test_case::test_case;

#[test_case("openai", true, true; "openai_treatment")]
#[test_case("openai", false, false; "openai_control")]
#[test_case("litellm-hybrid", true, false; "litellm")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_agent_review_requires_openai_on_start_and_resume(
    provider_id: &'static str,
    requested: bool,
    expected_review: bool,
) -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = responses::start_mock_server().await;
    let mut previous = None;
    for phase in ["fresh", "resumed"] {
        let mut builder = test_codex()
            .with_model("gpt-5.5")
            .with_config(move |config| {
                super::configure_scenario_catalog(config);
                config.model_provider_id = provider_id.to_owned();
                // Keep the mock Responses transport and OpenAI display name for every provider ID.
                config
                    .model_providers
                    .insert(provider_id.to_owned(), config.model_provider.clone());
                for feature in [Feature::Collab, Feature::MultiAgentV2] {
                    config
                        .features
                        .enable(feature)
                        .expect("enable multi-agent feature");
                }
                config
                    .features
                    .set_enabled(Feature::GuardianNativeMultiAgentHistory, requested)
                    .expect("set native agent review flag");
                config.multi_agent_v2.tool_namespace = Some("collaboration".to_owned());
                config.approvals_reviewer = ApprovalsReviewer::AutoReview;
                config.permissions.approval_policy =
                    Constrained::allow_any(AskForApproval::OnRequest);
                config
                    .permissions
                    .set_permission_profile(PermissionProfile::read_only())
                    .expect("set read-only permissions");
            });
        let test = if let Some(previous) = &previous {
            builder.restart_with_auto_env(&server, previous).await?
        } else {
            builder.build_with_auto_env(&server).await?
        };
        let call_id = format!("send-{phase}");
        let mut call = responses::ev_function_call_with_namespace(
            &call_id,
            "collaboration",
            "send_message",
            r#"{"target":"/root/missing","message":"opaque-message"}"#,
        );
        call["item"]["encrypted_content"] = json!("opaque-replay");
        call["item"]["encrypted_function_args"] = json!(["message"]);
        let mut stream = vec![responses::sse(vec![call, responses::ev_completed("send")])];
        if expected_review {
            stream.push(responses::sse(vec![
                responses::ev_assistant_message("assessment", r#"{"outcome":"allow","rationale":"Mock approval.","risk_level":"low","user_authorization":"high"}"#),
                responses::ev_completed("review"),
            ]));
        }
        stream.push(responses::sse(vec![
            responses::ev_assistant_message("done", "The worker is unavailable."),
            responses::ev_completed("done"),
        ]));
        let mock = responses::mount_sse_sequence(&server, stream).await;
        test.submit_text_turn("Ask the missing worker to confirm receipt.")
            .await?;
        let requests = mock.requests();
        let reviews = requests
            .iter()
            .filter(|request| {
                request.body_json()["client_metadata"]["x-openai-subagent"] == "guardian"
            })
            .collect::<Vec<_>>();
        assert_eq!(
            reviews.len(),
            usize::from(expected_review),
            "{phase}: {provider_id}"
        );
        if let Some(review) = reviews.first() {
            assert!(
                review
                    .inputs_of_type("function_call")
                    .iter()
                    .any(|call| call["call_id"] == call_id),
                "{phase}: native review evidence"
            );
        }
        previous = Some(test);
    }
    Ok(())
}
