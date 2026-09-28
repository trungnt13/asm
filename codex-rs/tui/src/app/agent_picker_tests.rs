use super::*;
use crate::app::test_support::make_test_app;
use crate::chatwidget::tests::helpers::render_bottom_popup;
use crate::model_catalog::ModelCatalog;
use codex_protocol::config_types::SERVICE_TIER_DEFAULT_REQUEST_VALUE;
use codex_protocol::openai_models::ModelServiceTier;
use codex_protocol::openai_models::ReasoningEffort;
use insta::assert_snapshot;
use pretty_assertions::assert_eq;
use std::sync::Arc;

#[tokio::test]
async fn subagent_picker_shows_configured_model_effort_and_tier() {
    let mut app = make_test_app().await;
    app.config
        .features
        .enable(Feature::FastMode)
        .expect("enable fast mode");
    app.config.service_tier = Some(ServiceTier::Fast.request_value().to_string());
    app.config
        .subagent_service_tiers
        .entry("gpt-6-sol".to_string())
        .or_default()
        .extend([
            (ReasoningEffort::High, "fast".to_string()),
            (
                ReasoningEffort::Max,
                SERVICE_TIER_DEFAULT_REQUEST_VALUE.to_string(),
            ),
        ]);
    let mut model = app
        .model_catalog
        .models
        .first()
        .expect("model preset")
        .clone();
    model.model = "gpt-6-sol".to_string();
    model.default_reasoning_effort = ReasoningEffort::Medium;
    model.service_tiers = vec![ModelServiceTier {
        id: ServiceTier::Fast.request_value().to_string(),
        name: "Fast".to_string(),
        description: String::new(),
    }];
    app.model_catalog = Arc::new(ModelCatalog::new(vec![model]));

    let root_id = ThreadId::from_string("00000000-0000-0000-0000-000000000001").unwrap();
    let fast_id = ThreadId::from_string("00000000-0000-0000-0000-000000000002").unwrap();
    let standard_id = ThreadId::from_string("00000000-0000-0000-0000-000000000003").unwrap();
    app.primary_thread_id = Some(root_id);
    app.active_thread_id = Some(root_id);
    for thread_id in [root_id, fast_id, standard_id] {
        app.agent_navigation.upsert(
            thread_id, /*agent_nickname*/ None, /*agent_role*/ None,
            /*is_closed*/ false,
        );
    }
    app.agent_navigation
        .set_agent_path(fast_id, Some("/root/sol_fast".to_string()));
    app.agent_navigation
        .set_agent_path(standard_id, Some("/root/sol_standard".to_string()));
    app.agent_navigation.set_model_settings(
        fast_id,
        Some("gpt-6-sol".to_string()),
        Some(ReasoningEffort::High),
    );
    app.agent_navigation.set_model_settings(
        standard_id,
        Some("gpt-6-sol".to_string()),
        Some(ReasoningEffort::Max),
    );

    let params = app.agent_picker_selection_view_params(None);
    assert_eq!(
        params
            .items
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "Main [default]",
            "/root/sol_fast gpt-6-sol-high-fast",
            "/root/sol_standard gpt-6-sol-max",
        ]
    );
    app.chat_widget.show_selection_view(params);
    assert_snapshot!(
        "subagent_picker_model_effort_tiers",
        render_bottom_popup(&app.chat_widget, /*width*/ 110)
    );

    app.agent_navigation.set_model_settings(
        standard_id,
        Some("gpt-6-sol".to_string()),
        Some(ReasoningEffort::Low),
    );
    assert_eq!(
        app.agent_picker_model_label(standard_id, /*is_primary*/ false),
        Some("gpt-6-sol-low-fast".to_string())
    );
    app.agent_navigation.set_model_settings(
        standard_id,
        Some("gpt-6-sol".to_string()),
        /*reasoning_effort*/ None,
    );
    assert_eq!(
        app.agent_picker_model_label(standard_id, /*is_primary*/ false),
        Some("gpt-6-sol-medium-fast".to_string())
    );
}
