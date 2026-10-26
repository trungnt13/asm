//! Session-only model selections leave durable defaults untouched. Explicit Astra selections retain
//! their originating task and are checked against its effective model when the app applies them.

use super::*;
use crate::bottom_pane::SelectionSecondaryAction;
use crate::keymap::ListAction;
use crate::model_catalog::LUNA_RESERVE_MODEL;

/// A final Astra selection whose model transition must be checked while the app applies it.
#[derive(Debug)]
pub(crate) enum AstraModelPickerAction {
    UpdateModel,
    ApplyModelSelection {
        effort: Option<ReasoningEffortConfig>,
    },
    ApplyAdvancedReasoning {
        effort: ReasoningEffortConfig,
    },
    SelectSessionModel {
        effort: Option<ReasoningEffortConfig>,
        service_tier: Option<String>,
    },
}

impl AstraModelPickerAction {
    pub(super) fn into_picker_event(self, thread_id: Option<ThreadId>, model: String) -> AppEvent {
        match thread_id {
            Some(thread_id) => AppEvent::AstraSelectedFromModelPicker {
                thread_id,
                model,
                action: self,
            },
            None => self.into_app_event(model),
        }
    }

    pub(crate) fn into_app_event(self, model: String) -> AppEvent {
        match self {
            Self::UpdateModel => AppEvent::UpdateModel(model),
            Self::ApplyModelSelection { effort } => AppEvent::ApplyModelSelection { model, effort },
            Self::ApplyAdvancedReasoning { effort } => {
                AppEvent::ApplyAdvancedReasoning { model, effort }
            }
            Self::SelectSessionModel {
                effort,
                service_tier,
            } => AppEvent::SelectSessionModel {
                model,
                effort,
                service_tier,
            },
        }
    }
}

impl ChatWidget {
    pub(super) fn model_selection_needs_new_session(&self, model: &str) -> bool {
        (codex_models_manager::provider_for_model(self.current_model())
            != codex_models_manager::provider_for_model(model))
            || (codex_protocol::openai_models::is_native_runtime_model(self.current_model())
                || codex_protocol::openai_models::is_native_runtime_model(&model))
                && self.current_model() != model
    }

    pub(crate) fn confirm_model_session(
        &mut self,
        model: String,
        effort: Option<ReasoningEffortConfig>,
    ) {
        let thread_id = self.thread_id();
        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Start a new session?".to_string()),
            subtitle: Some(format!("Use {model}. The current session stays saved; conversation history is not copied. The selected model uses its configured provider or native runtime.")),
            items: vec![SelectionItem {
                name: "Start new session".to_string(),
                dismiss_on_select: true,
                actions: vec![Box::new(move |tx| tx.send(AppEvent::StartModelSession {
                    thread_id,
                    model: model.clone(),
                    effort: effort.clone(),
                    confirmed: true,
                }))],
                ..Default::default()
            }],
            ..SelectionViewParams::picker()
        });
    }

    pub(super) fn session_model_selection_action(
        &self,
        model: String,
        effort: Option<ReasoningEffortConfig>,
    ) -> Option<SelectionSecondaryAction> {
        if codex_protocol::openai_models::is_native_runtime_model(&model)
            || self.model_selection_needs_new_session(&model)
        {
            return None;
        }
        // Reserve selections are already session-only.
        if model == LUNA_RESERVE_MODEL {
            return None;
        }
        let key = key_hint::plain(KeyCode::Char('s'));
        let keymap = self.bottom_pane.list_keymap();
        let mut hints = Vec::new();
        if let Some(accept) = keymap.primary_hint(ListAction::Accept) {
            let label = if effort == Some(ReasoningEffortConfig::Ultra) {
                " apply · "
            } else {
                " default · "
            };
            hints.extend([accept.into(), label.dim()]);
        }
        hints.extend([key.into(), " session".dim()]);
        if let Some(cancel) = keymap.primary_hint(ListAction::Cancel) {
            hints.extend([" · ".dim(), cancel.into(), " back".dim()]);
        }
        let warning = effort
            .as_ref()
            .and_then(|effort| self.ultra_reasoning_concurrency_warning(effort));
        let sparkle_thread = self.sparkle_thread_for_picker_action(&model);
        Some(SelectionSecondaryAction {
            key,
            footer_hint: hints.into(),
            action: Box::new(move |tx| {
                tx.send(
                    AstraModelPickerAction::SelectSessionModel {
                        effort: effort.clone(),
                        service_tier: None,
                    }
                    .into_picker_event(sparkle_thread, model.clone()),
                );
                if let Some(warning) = warning.clone() {
                    tx.send(AppEvent::InsertHistoryCell(Box::new(
                        history_cell::new_warning_event(warning),
                    )));
                }
            }),
        })
    }
}
