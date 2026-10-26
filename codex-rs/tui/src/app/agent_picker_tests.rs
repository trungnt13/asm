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
    app.config.service_tier = Some(SERVICE_TIER_DEFAULT_REQUEST_VALUE.to_string());
    app.config.model_context_window = Some(128_000);
    app.config.subagent_model_context_windows.extend([
        ("gpt-6-astra".to_string(), 272_000),
        ("gpt-6.1-sol".to_string(), 272_000),
    ]);
    app.config
        .subagent_service_tiers
        .entry("gpt-6.1-sol".to_string())
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
    model.model = "gpt-6.1-sol".to_string();
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
    assert_eq!(
        app.agent_picker_model_label(root_id, /*is_primary*/ true),
        None
    );
    app.agent_navigation.set_model_settings(
        root_id,
        Some("gpt-6-astra".to_string()),
        Some(ReasoningEffort::XHigh),
    );
    app.agent_navigation.set_model_settings(
        fast_id,
        Some("gpt-6.1-sol".to_string()),
        Some(ReasoningEffort::High),
    );
    app.agent_navigation.set_model_settings(
        standard_id,
        Some("gpt-6.1-sol".to_string()),
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
            "Main [default] ast6-xhi-128k",
            "/root/sol_fast sol61-hig-fast-272k",
            "/root/sol_standard sol61-max-272k",
        ]
    );
    app.chat_widget.show_selection_view(params);
    assert_snapshot!(
        "subagent_picker_model_effort_tiers",
        render_bottom_popup(&app.chat_widget, /*width*/ 110)
    );

    app.config.service_tier = Some(ServiceTier::Fast.request_value().to_string());
    assert_eq!(
        app.agent_picker_model_label(standard_id, /*is_primary*/ false),
        Some("sol61-max-272k".to_string())
    );
    app.agent_navigation.set_model_settings(
        standard_id,
        Some("gpt-6.1-sol".to_string()),
        Some(ReasoningEffort::Low),
    );
    assert_eq!(
        app.agent_picker_model_label(standard_id, /*is_primary*/ false),
        Some("sol61-low-fast-272k".to_string())
    );
    app.agent_navigation.set_model_settings(
        standard_id,
        Some("gpt-6.1-sol".to_string()),
        /*reasoning_effort*/ None,
    );
    assert_eq!(
        app.agent_picker_model_label(standard_id, /*is_primary*/ false),
        Some("sol61-med-fast-272k".to_string())
    );

    // Local backend flags do not change the identity of an already spawned V2 child.
    app.config
        .features
        .disable(Feature::MultiAgentV2)
        .expect("disable local v2 flag");
    assert_eq!(
        app.agent_picker_model_label(fast_id, /*is_primary*/ false),
        Some("sol61-hig-fast-272k".to_string())
    );
    // Missing paths cannot establish V2 identity, so matched maps must not apply.
    let no_path_id = ThreadId::from_string("00000000-0000-0000-0000-000000000004").unwrap();
    app.agent_navigation.upsert(
        no_path_id, /*agent_nickname*/ None, /*agent_role*/ None,
        /*is_closed*/ false,
    );
    app.agent_navigation.set_model_settings(
        no_path_id,
        Some("gpt-6.1-sol".to_string()),
        /*reasoning_effort*/ None,
    );
    assert_eq!(
        app.agent_picker_model_label(no_path_id, /*is_primary*/ false),
        Some("sol61-med-fast-128k".to_string())
    );
    app.config.model_context_window = None;
    assert_eq!(
        app.agent_picker_model_label(no_path_id, /*is_primary*/ false),
        Some("sol61-med-fast".to_string())
    );
    for capacity in [0, -1] {
        app.config.model_context_window = Some(capacity);
        app.config
            .subagent_model_context_windows
            .insert("gpt-6.1-sol".to_string(), capacity);
        assert_eq!(
            app.agent_picker_model_label(fast_id, /*is_primary*/ false),
            Some("sol61-hig-fast".to_string())
        );
        assert_eq!(
            app.agent_picker_model_label(root_id, /*is_primary*/ true),
            Some("ast6-xhi-fast".to_string())
        );
    }
    app.config.model_context_window = Some(32_768);
    app.agent_navigation.set_model_settings(
        fast_id,
        Some("custom-2".to_string()),
        Some(ReasoningEffort::Low),
    );
    assert_eq!(
        app.agent_picker_model_label(fast_id, /*is_primary*/ false),
        Some("cus2-low-fast-32.8k".to_string())
    );
    app.config.model_context_window = None;
    assert_eq!(
        app.agent_picker_model_label(fast_id, /*is_primary*/ false),
        Some("cus2-low-fast".to_string())
    );
    app.config
        .subagent_model_context_windows
        .insert("custom-2".to_string(), /*v*/ 272_000);
    assert_eq!(
        app.agent_picker_model_label(fast_id, /*is_primary*/ false),
        Some("cus2-low-fast-272k".to_string())
    );
}
