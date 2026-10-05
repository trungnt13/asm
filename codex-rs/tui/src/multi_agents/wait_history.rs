//! Pair wait previews for grouped counting without changing their original history order.

use super::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

#[derive(Clone, Copy, Debug)]
enum WaitGroupState {
    Running,
    Completed,
    KeepVisible,
}

#[derive(Default, Debug)]
pub(crate) struct AgentWaitHistory {
    pending: HashMap<String, Arc<RwLock<WaitGroupState>>>,
}

impl AgentWaitHistory {
    pub(crate) fn cell(
        &mut self,
        item: &ThreadItem,
        agent_metadata: impl FnMut(ThreadId) -> AgentMetadata,
    ) -> Option<AgentWaitHistoryCell> {
        let ThreadItem::CollabAgentToolCall {
            id,
            tool: CollabAgentTool::Wait,
            status,
            agents_states,
            ..
        } = item
        else {
            return None;
        };
        let next = match status {
            CollabAgentToolCallStatus::InProgress => WaitGroupState::Running,
            CollabAgentToolCallStatus::Completed if agents_states.is_empty() => {
                WaitGroupState::Completed
            }
            CollabAgentToolCallStatus::Completed
            | CollabAgentToolCallStatus::Failed
            | CollabAgentToolCallStatus::Interrupted => WaitGroupState::KeepVisible,
        };
        let state = if *status == CollabAgentToolCallStatus::InProgress {
            Arc::clone(
                self.pending
                    .entry(id.clone())
                    .or_insert_with(|| Arc::new(RwLock::new(next))),
            )
        } else if let Some(state) = self.pending.remove(id) {
            *state
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = next;
            state
        } else {
            Arc::new(RwLock::new(next))
        };
        Some(AgentWaitHistoryCell {
            preview: tool_call_history_cell(
                item,
                /*cached_spawn_request*/ None,
                agent_metadata,
            )?,
            count_key: format!("agent-wait:{id}"),
            state,
        })
    }

    pub(crate) fn clear(&mut self) {
        for (_, state) in self.pending.drain() {
            *state
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = WaitGroupState::KeepVisible;
        }
    }
}

impl Drop for AgentWaitHistory {
    fn drop(&mut self) {
        self.clear();
    }
}

#[derive(Debug)]
pub(crate) struct AgentWaitHistoryCell {
    preview: PlainHistoryCell,
    // One namespaced identity retains group disclosure and deduplicates paired preview counts.
    count_key: String,
    state: Arc<RwLock<WaitGroupState>>,
}

impl HistoryCell for AgentWaitHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.preview.display_lines(width)
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        self.preview.raw_lines()
    }

    fn activity_ids(&self) -> Vec<String> {
        if self.tool_call_summary().is_some() {
            vec![self.count_key.clone()]
        } else {
            Vec::new()
        }
    }

    fn tool_call_summary(&self) -> Option<ToolCallSummary> {
        let state = *self
            .state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match state {
            WaitGroupState::KeepVisible => None,
            WaitGroupState::Running | WaitGroupState::Completed => Some(ToolCallSummary {
                count: 1,
                count_key: Some(self.count_key.clone()),
                running: matches!(state, WaitGroupState::Running),
                names: vec!["wait_agent".to_string()],
            }),
        }
    }

    fn activity_disclosure(&self, _width: u16) -> Option<crate::history_cell::ActivityDisclosure> {
        None
    }

    fn supports_individual_disclosure(&self) -> bool {
        false
    }

    fn has_stable_transcript_height(&self) -> bool {
        false
    }
}
