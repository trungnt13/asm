//! Stateless projection from canonical paginated rollout records to thread-history changes.
//!
//! This module is only for the new paginated rollout format that persists canonical
//! `ItemCompleted(TurnItem)` records, not legacy event-only rollouts.

use codex_protocol::protocol::EventMsg;
use codex_rollout::RolloutItem;
use codex_rollout::RolloutLine;

use crate::protocol::thread_history::ThreadHistoryChangeSet;
use crate::protocol::thread_history::ThreadHistoryItemChange;
use crate::protocol::thread_history::ThreadHistoryTurnMetadata;
use crate::protocol::v2::ThreadItem;
use crate::protocol::v2::TurnError;
use crate::protocol::v2::TurnStatus;

/// Project one durable rollout line without reconstructing earlier history.
///
/// Callers that replay a JSONL suffix should invoke it once per line, in ordinal order, so storage
/// can preserve the first and latest timestamps for repeated item snapshots independently.
pub fn project_rollout_line(line: &RolloutLine) -> ThreadHistoryChangeSet {
    match &line.item {
        RolloutItem::EventMsg(EventMsg::TurnStarted(event)) => ThreadHistoryChangeSet {
            changed_turns: vec![ThreadHistoryTurnMetadata {
                token_usage: None,
                turn_id: event.turn_id.clone(),
                root_turn_id: event.root_turn_id.clone(),
                status: TurnStatus::InProgress,
                error: None,
                started_at: event.started_at,
                completed_at: None,
                duration_ms: None,
            }],
            ..Default::default()
        },
        RolloutItem::EventMsg(EventMsg::TurnComplete(event)) => ThreadHistoryChangeSet {
            changed_turns: vec![ThreadHistoryTurnMetadata {
                token_usage: None,
                turn_id: event.turn_id.clone(),
                root_turn_id: event.root_turn_id.clone(),
                status: if event.error.is_some() {
                    TurnStatus::Failed
                } else {
                    TurnStatus::Completed
                },
                error: event.error.as_ref().map(|error| TurnError {
                    misalignment: error.misalignment.clone().map(Into::into),
                    message: error.message.clone(),
                    codex_error_info: error.codex_error_info.clone().map(Into::into),
                    additional_details: None,
                }),
                started_at: event.started_at,
                completed_at: event.completed_at,
                duration_ms: event.duration_ms,
            }],
            ..Default::default()
        },
        RolloutItem::EventMsg(EventMsg::TurnAborted(event)) => {
            let Some(turn_id) = event.turn_id.as_ref() else {
                return ThreadHistoryChangeSet::default();
            };
            ThreadHistoryChangeSet {
                changed_turns: vec![ThreadHistoryTurnMetadata {
                    token_usage: None,
                    turn_id: turn_id.clone(),
                    root_turn_id: event.root_turn_id.clone(),
                    status: TurnStatus::Interrupted,
                    error: event.error.as_ref().map(|error| TurnError {
                        message: error.message.clone(),
                        codex_error_info: error.codex_error_info.clone().map(Into::into),
                        misalignment: error.misalignment.clone().map(Into::into),
                        additional_details: None,
                    }),
                    started_at: event.started_at,
                    completed_at: event.completed_at,
                    duration_ms: event.duration_ms,
                }],
                ..Default::default()
            }
        }
        RolloutItem::EventMsg(EventMsg::ItemCompleted(event)) => ThreadHistoryChangeSet {
            changed_items: vec![ThreadHistoryItemChange {
                turn_id: event.turn_id.clone(),
                item: ThreadItem::from(event),
            }],
            ..Default::default()
        },
        RolloutItem::ResponseItem(response) => project_inter_agent_message(
            line,
            &response.item,
            response
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.spawn_arguments.as_deref()),
        ),
        RolloutItem::InterAgentCommunication(communication) => {
            project_inter_agent_message(line, &communication.to_model_input_item(), None)
        }
        RolloutItem::SessionMeta(_)
        | RolloutItem::InterAgentCommunicationMetadata { .. }
        | RolloutItem::Compacted(_)
        | RolloutItem::TurnContext(_)
        | RolloutItem::TokenUsageRecord(_)
        | RolloutItem::WorldState(_)
        | RolloutItem::RealtimeItem(_)
        | RolloutItem::RetainedContext(_)
        | RolloutItem::SecurityRiskScore(_)
        | RolloutItem::EventMsg(_) => ThreadHistoryChangeSet::default(),
    }
}

fn project_inter_agent_message(
    line: &RolloutLine,
    response: &codex_protocol::models::ResponseItem,
    spawn_arguments: Option<&str>,
) -> ThreadHistoryChangeSet {
    let Some(turn_id) = response.turn_id() else {
        return ThreadHistoryChangeSet::default();
    };
    let fallback_id = format!("inter-agent-{}", line.ordinal.unwrap_or_default());
    let Some(item) = ThreadItem::from_inter_agent_message(response, &fallback_id, spawn_arguments)
    else {
        return ThreadHistoryChangeSet::default();
    };
    ThreadHistoryChangeSet {
        changed_items: vec![ThreadHistoryItemChange {
            turn_id: turn_id.to_string(),
            item,
        }],
        ..Default::default()
    }
}

#[cfg(test)]
#[path = "thread_history_projection_tests.rs"]
mod tests;
