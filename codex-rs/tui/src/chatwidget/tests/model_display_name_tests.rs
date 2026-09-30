//! Catalog display names are presentation only; model selection retains wire slugs.

use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn custom_model_display_name_in_pickers_preserves_selection_slug() {
    let slug = "us.openai.gpt-5.6-luna";
    let (mut chat, mut events, _ops) = make_chatwidget_manual(Some(slug)).await;
    let mut preset = get_available_model(&chat, "gpt-5.5");
    preset.id = slug.to_string();
    preset.model = slug.to_string();
    preset.display_name = "GPT-5.6 Luna".to_string();
    preset.description = "Custom provider model".to_string();
    preset.default_reasoning_effort = ReasoningEffortConfig::High;
    preset.supported_reasoning_efforts = vec![
        ReasoningEffortPreset {
            effort: ReasoningEffortConfig::Low,
            description: "Quick answers".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffortConfig::High,
            description: "Deeper reasoning".to_string(),
        },
    ];
    let mut auto = preset.clone();
    auto.id = "codex-auto-fast".to_string();
    auto.model = auto.id.clone();
    auto.display_name = "Auto Fast".to_string();
    chat.model_catalog = Arc::new(ModelCatalog::new(vec![auto, preset.clone()]));
    chat.set_reasoning_effort(Some(ReasoningEffortConfig::High));
    chat.open_model_popup_with_presets(chat.model_catalog.models.clone());
    assert_chatwidget_snapshot!(
        "custom_model_display_name_quick_picker",
        render_bottom_popup(&chat, /*width*/ 80)
    );
    chat.handle_key_event(KeyCode::Enter.into());
    assert_matches!(events.try_recv(), Ok(AppEvent::OpenAllModelsPopup));
    chat.open_all_models_popup();
    assert_chatwidget_snapshot!(
        "custom_model_display_name_all_models",
        render_bottom_popup(&chat, /*width*/ 80)
    );
    chat.handle_key_event(KeyCode::Enter.into());
    let selected =
        assert_matches!(events.try_recv(), Ok(AppEvent::OpenReasoningPopup { model }) => model);
    assert_eq!(selected, preset);
    chat.open_reasoning_popup(selected);
    assert_chatwidget_snapshot!(
        "custom_model_display_name_reasoning",
        render_bottom_popup(&chat, /*width*/ 80)
    );
    chat.handle_key_event(KeyCode::Enter.into());
    assert_matches!(events.try_recv(), Ok(AppEvent::UpdateModel(model)) if model == slug);
    assert_matches!(
        events.try_recv(),
        Ok(AppEvent::UpdateReasoningEffort(Some(
            ReasoningEffortConfig::High
        )))
    );
    let persisted = assert_matches!(events.try_recv(), Ok(AppEvent::PersistModelSelection { model, effort }) => (model, effort));
    assert_eq!(
        persisted,
        (slug.to_string(), Some(ReasoningEffortConfig::High))
    );
}

#[tokio::test]
async fn custom_model_display_name_in_status_line_and_fallback() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let slug = "us.openai.gpt-5.6-luna";
    let (mut chat, _events, _ops) = make_chatwidget_manual(Some(slug)).await;
    let mut preset = get_available_model(&chat, "gpt-5.5");
    preset.model = slug.to_string();
    preset.display_name = "GPT-5.6 Luna".to_string();
    preset.show_in_picker = false;
    chat.model_catalog = Arc::new(ModelCatalog::new(vec![preset]));
    chat.show_welcome_banner = false;
    chat.local_settings.tui.status_line = Some(vec![
        "model-name".to_string(),
        "model-with-reasoning".to_string(),
    ]);
    chat.set_reasoning_effort(Some(ReasoningEffortConfig::High));
    chat.refresh_status_line();
    let width = 80;
    let mut terminal = Terminal::new(TestBackend::new(width, chat.desired_height(width)))
        .expect("create terminal");
    terminal
        .draw(|frame| chat.render(frame.area(), frame.buffer_mut()))
        .expect("draw model status line");
    assert_chatwidget_snapshot!(
        "custom_model_display_name_status_line",
        normalized_backend_snapshot(terminal.backend())
    );

    Arc::make_mut(&mut chat.model_catalog).models.clear();
    assert_eq!(chat.model_display_name(), slug);
    chat.set_model(crate::model_catalog::LUNA_RESERVE_MODEL);
    assert_eq!(chat.model_display_name(), "Luna Reserve");
    chat.set_model("");
    assert_eq!(chat.model_display_name(), DEFAULT_MODEL_DISPLAY_NAME);
}

#[tokio::test]
async fn compact_status_line_labels_preserve_model_and_title() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut snapshots = Vec::new();
    for (model, display_name, effort, expected) in [
        (
            "gpt-6.1-sol",
            "GPT-6.1-Sol",
            ReasoningEffortConfig::Medium,
            "sol61·med·sol61 med",
        ),
        (
            "gpt-6-astra",
            "GPT-6-Astra",
            ReasoningEffortConfig::High,
            "astra6·hig·astra6 hig",
        ),
        (
            "gpt-6-luna",
            "GPT-6-Luna",
            ReasoningEffortConfig::XHigh,
            "luna6·xhi·luna6 xhi",
        ),
        (
            "gpt-5.6-luna",
            "GPT-5.6 Luna",
            ReasoningEffortConfig::Low,
            "luna56·low·luna56 low",
        ),
        (
            "gpt-5.2",
            "GPT-5.2",
            ReasoningEffortConfig::None,
            "52·def·52 def",
        ),
        (
            "gpt-reserve",
            "Luna Reserve",
            ReasoningEffortConfig::Custom("deeper".into()),
            "luna reserve·dee·luna reserve dee",
        ),
        (
            "custom-model",
            "Custom.V2",
            ReasoningEffortConfig::Ultra,
            "customv2·ult·customv2 ult",
        ),
    ] {
        let (mut chat, _events, _ops) = make_chatwidget_manual(Some(model)).await;
        let mut preset = get_available_model(&chat, "gpt-5.5");
        preset.model = model.to_string();
        preset.display_name = display_name.to_string();
        chat.model_catalog = Arc::new(ModelCatalog::new(vec![preset]));
        chat.show_welcome_banner = false;
        chat.local_settings.tui.status_line = Some(vec![
            "model".into(),
            "reasoning".into(),
            "model-with-reasoning".into(),
        ]);
        chat.local_settings.tui.terminal_title = Some(vec!["model-with-reasoning".into()]);
        chat.set_reasoning_effort(Some(effort.clone()));
        chat.refresh_status_line();
        chat.refresh_terminal_title();
        let title_effort = match effort {
            ReasoningEffortConfig::None => "default",
            _ => effort.as_str(),
        };
        assert_eq!(
            (
                status_line_text(&chat),
                chat.last_terminal_title.clone(),
                chat.model_display_name(),
                chat.current_model()
            ),
            (
                Some(expected.to_string()),
                Some(format!("{display_name} {title_effort}")),
                display_name,
                model
            ),
        );
        let preview = chat
            .status_surface_preview_data()
            .status_line_for_items(
                [
                    StatusLineItem::ModelName,
                    StatusLineItem::Reasoning,
                    StatusLineItem::ModelWithReasoning,
                ],
                /*use_theme_colors*/ false,
            )
            .expect("status preview");
        assert_eq!(
            preview
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            expected,
        );
        let width = 80;
        let mut terminal = Terminal::new(TestBackend::new(width, chat.desired_height(width)))
            .expect("create terminal");
        terminal
            .draw(|frame| chat.render(frame.area(), frame.buffer_mut()))
            .expect("draw compact labels");
        snapshots.push(format!(
            "{display_name}\n{}",
            normalized_backend_snapshot(terminal.backend())
        ));
    }
    let placeholders = crate::bottom_pane::StatusSurfacePreviewData::default()
        .status_line_for_items(
            [
                StatusLineItem::ModelName,
                StatusLineItem::Reasoning,
                StatusLineItem::ModelWithReasoning,
            ],
            /*use_theme_colors*/ false,
        )
        .expect("placeholder preview")
        .to_string();
    assert_eq!(placeholders, "codex52·med·codex52 med");
    snapshots.push(format!("Placeholder preview\n{placeholders}"));
    assert_chatwidget_snapshot!("compact_status_line_labels", snapshots.join("\n\n"));
}
