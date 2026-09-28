//! Root-scoped background refresh for the agent picker.

use super::*;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::SortDirection;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
use codex_app_server_protocol::ThreadSourceKind;
use codex_app_server_protocol::ThreadStatus;
use codex_features::Feature;
use codex_protocol::config_types::ServiceTier;
use std::collections::HashSet;

pub(super) const AGENT_PICKER_VIEW_ID: &str = "agent-picker";
const AGENT_PICKER_PAGE_SIZE: u32 = 100;
const AGENT_PICKER_MAX_THREADS: usize = 1_000;

impl App {
    /// Formats configured picker settings; this is not a report of an executed request.
    pub(super) fn agent_picker_model_label(
        &self,
        thread_id: ThreadId,
        is_primary: bool,
    ) -> Option<String> {
        let settings = self.agent_navigation.model_settings(&thread_id)?;
        let preset = self
            .model_catalog
            .models
            .iter()
            .find(|preset| preset.model == settings.model);
        let effort = settings
            .reasoning_effort
            .as_ref()
            .or_else(|| preset.map(|preset| &preset.default_reasoning_effort));
        let inherited_tier = self
            .primary_session_configured
            .as_ref()
            .map(|session| session.service_tier.as_deref())
            .unwrap_or_else(|| self.config.service_tier.as_deref());
        let selected_tier = if is_primary {
            inherited_tier
        } else {
            effort
                .and_then(|effort| {
                    self.config
                        .subagent_service_tiers
                        .get(&settings.model)?
                        .get(effort)
                })
                .map(String::as_str)
                .or(inherited_tier)
        };
        let fast = selected_tier.and_then(ServiceTier::from_request_value)
            == Some(ServiceTier::Fast)
            && self.config.features.enabled(Feature::FastMode)
            && preset.is_none_or(|preset| {
                crate::service_tier_resolution::model_supports_service_tier(
                    preset,
                    ServiceTier::Fast.request_value(),
                )
            });
        let mut label = settings.model.clone();
        if let Some(effort) = effort {
            label.push('-');
            label.push_str(effort.as_str());
        }
        if fast {
            label.push_str("-fast");
        }
        Some(label)
    }

    pub(super) fn refresh_agent_picker_threads(
        &mut self,
        app_server: &AppServerSession,
        root: ThreadId,
    ) {
        let Some(request_id) = self.agent_navigation.begin_picker_refresh(root) else {
            return;
        };
        let request_handle = app_server.request_handle();
        let app_event_tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            let result = async {
                let mut threads = Vec::new();
                let mut cursor = None;
                let mut seen_cursors = HashSet::new();
                while threads.len() < AGENT_PICKER_MAX_THREADS
                    && seen_cursors.insert(cursor.clone())
                {
                    let page = match request_handle
                        .request_typed::<ThreadListResponse>(ClientRequest::ThreadList {
                            request_id: RequestId::String(Uuid::new_v4().to_string()),
                            params: ThreadListParams {
                                originators: None,
                                cursor,
                                limit: Some(AGENT_PICKER_PAGE_SIZE),
                                sort_key: None,
                                sort_direction: Some(SortDirection::Desc),
                                model_providers: Some(vec![]),
                                source_kinds: Some(vec![ThreadSourceKind::SubAgentThreadSpawn]),
                                archived: None,
                                section_id: None,
                                project_id: None,
                                cwd: None,
                                use_state_db_only: true,
                                search_term: None,
                                parent_thread_id: None,
                                ancestor_thread_id: Some(root.to_string()),
                            },
                        })
                        .await
                    {
                        Ok(page) => page,
                        Err(err) if threads.is_empty() => return Err(err.to_string()),
                        Err(err) => {
                            tracing::warn!(%err, "failed to refresh remaining agent picker descendants");
                            break;
                        }
                    };
                    threads.extend(
                        page.data
                            .into_iter()
                            .take(AGENT_PICKER_MAX_THREADS - threads.len()),
                    );
                    let Some(next_cursor) = page.next_cursor else {
                        break;
                    };
                    cursor = Some(next_cursor);
                }
                threads.reverse();
                Ok(threads)
            }
            .await;

            app_event_tx.send(AppEvent::AgentPickerThreadsLoaded {
                primary_thread_id: root,
                request_id,
                result,
            });
        });
    }

    pub(super) fn apply_agent_picker_thread_refresh(
        &mut self,
        root: ThreadId,
        request_id: Uuid,
        result: Result<Vec<Thread>, String>,
    ) {
        if !self
            .agent_navigation
            .finish_picker_refresh(root, request_id)
            || self.primary_thread_id != Some(root)
        {
            return;
        }
        let threads = match result {
            Ok(threads) => threads,
            Err(err) => {
                tracing::warn!(%err, "failed to refresh agent picker descendants");
                return;
            }
        };
        let selected = self
            .chat_widget
            .selected_index_for_present_view(AGENT_PICKER_VIEW_ID);
        for thread in threads {
            let Ok(thread_id) = ThreadId::from_string(&thread.id) else {
                continue;
            };
            let live = self
                .thread_event_channels
                .get(&thread_id)
                .is_some_and(|channel| channel.attachment() == ThreadEventAttachment::Live);
            let previous = self.agent_navigation.get(&thread_id);
            let is_running = matches!(thread.status, ThreadStatus::Active { .. });
            let update_liveness = previous.is_none() || !is_running;
            let is_closed = !live && matches!(thread.status, ThreadStatus::NotLoaded);
            if !is_closed && previous.is_some_and(|entry| entry.is_closed) {
                continue;
            }
            let agent_path = crate::app_server_session::source_agent_path(&thread.source);
            let agent_nickname = thread
                .agent_nickname
                .or_else(|| previous.and_then(|entry| entry.agent_nickname.clone()));
            let agent_role = thread
                .agent_role
                .or_else(|| previous.and_then(|entry| entry.agent_role.clone()));
            if thread.can_accept_direct_input == Some(false) {
                self.agent_navigation.mark_parent_owned(thread_id);
            }
            self.upsert_agent_picker_thread(thread_id, agent_nickname, agent_role, is_closed);
            self.agent_navigation.set_model_settings(
                thread_id,
                thread.model,
                thread.reasoning_effort,
            );
            self.agent_navigation.set_agent_path(thread_id, agent_path);
            if !live && update_liveness {
                self.agent_navigation.set_running(thread_id, is_running);
            }
        }

        let params = self.agent_picker_selection_view_params(selected);
        self.chat_widget
            .replace_selection_view_if_present(AGENT_PICKER_VIEW_ID, params);
    }
}

#[cfg(test)]
#[path = "agent_picker_tests.rs"]
mod tests;
