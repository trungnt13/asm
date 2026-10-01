//! Preserve saved parallel threads when ordinary navigation discards a temporary side.

use super::parallel::CompanionKind;
use super::*;
use codex_app_server_protocol::ThreadUnsubscribeParams;
use codex_app_server_protocol::ThreadUnsubscribeResponse;
use codex_app_server_protocol::TurnInterruptParams;
use codex_app_server_protocol::TurnInterruptResponse;

impl App {
    pub(super) fn side_thread_to_discard_after_switch(
        &self,
        target_thread_id: ThreadId,
    ) -> Option<ThreadId> {
        let active_thread_id = self.current_displayed_thread_id()?;
        let (&side_thread_id, state) = self.side_threads.iter().next()?;
        if state.kind != CompanionKind::Side
            || target_thread_id == side_thread_id
            || target_thread_id == active_thread_id
        {
            return None;
        }

        (active_thread_id == side_thread_id || active_thread_id == state.parent_thread_id)
            .then_some(side_thread_id)
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn discard_side_thread_in_background(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) {
        self.abandoned_side_threads.insert(thread_id);
        self.pending_app_server_requests
            .cancel_thread_verification(&thread_id.to_string());
        let turn_id = self
            .active_turn_id_for_thread(thread_id)
            .await
            .unwrap_or_default();
        let request_handle = app_server.request_handle();
        let interrupt_request_id = app_server.next_request_id();
        let retry_interrupt_request_id = app_server.next_request_id();
        let unsubscribe_request_id = app_server.next_request_id();

        self.discard_thread_local_state(thread_id).await;

        tokio::spawn(async move {
            let interrupt_result = request_handle
                .request_typed::<TurnInterruptResponse>(ClientRequest::TurnInterrupt {
                    request_id: interrupt_request_id,
                    params: TurnInterruptParams {
                        thread_id: thread_id.to_string(),
                        turn_id: turn_id.clone(),
                    },
                })
                .await;
            let interrupt_result = if let Err(error) = &interrupt_result
                && let Some(actual_turn_id) = active_turn_interrupt_race(error)
            {
                request_handle
                    .request_typed::<TurnInterruptResponse>(ClientRequest::TurnInterrupt {
                        request_id: retry_interrupt_request_id,
                        params: TurnInterruptParams {
                            thread_id: thread_id.to_string(),
                            turn_id: actual_turn_id,
                        },
                    })
                    .await
            } else {
                interrupt_result
            };
            if let Err(error) = interrupt_result {
                tracing::warn!(%error, "failed to interrupt side conversation");
            }
            if let Err(error) = request_handle
                .request_typed::<ThreadUnsubscribeResponse>(ClientRequest::ThreadUnsubscribe {
                    request_id: unsubscribe_request_id,
                    params: ThreadUnsubscribeParams {
                        thread_id: thread_id.to_string(),
                    },
                })
                .await
            {
                tracing::warn!(%error, "failed to unsubscribe side conversation");
            }
        });
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn select_agent_thread_and_discard_side(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) -> Result<()> {
        let side_thread_to_discard = self.side_thread_to_discard_after_switch(thread_id);
        self.select_agent_thread(tui, app_server, thread_id).await?;
        if self.active_thread_id == Some(thread_id)
            && let Some(side_thread_id) = side_thread_to_discard
        {
            self.discard_side_thread_in_background(app_server, side_thread_id)
                .await;
            self.surface_pending_inactive_thread_interactive_requests()
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "side_navigation_tests.rs"]
mod tests;
