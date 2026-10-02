//! Counts distinct subagents observed running during live turns.
//! Completed counts stay client-local so switching views cannot change a finished footer.

use super::App;
use super::app_server_event_targets::ServerNotificationThreadTarget;
use super::app_server_event_targets::server_notification_thread_target;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::ThreadStatus;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SubAgentSource;
use std::collections::HashMap;
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Default)]
pub(super) struct TurnAgentCounts {
    active: HashMap<ThreadId, ActiveTurnAgents>,
    completed: HashMap<(ThreadId, String), usize>,
    // The refresh ID protects live activity received while a metadata read is in flight.
    pub(super) running: HashMap<ThreadId, (bool, Option<Uuid>)>,
}

struct ActiveTurnAgents {
    turn_id: String,
    agents: HashSet<ThreadId>,
}

impl TurnAgentCounts {
    pub(super) fn count(&self, thread_id: ThreadId, turn_id: &str) -> Option<usize> {
        self.completed
            .get(&(thread_id, turn_id.to_owned()))
            .copied()
    }
}

impl App {
    pub(super) fn track_turn_agents(&mut self, notification: &ServerNotification) {
        let ServerNotificationThreadTarget::Thread(thread_id) =
            server_notification_thread_target(notification)
        else {
            return;
        };
        let refresh_id = self.agents_overview.request_id;
        let state = &mut self.turn_agent_counts;
        match notification {
            ServerNotification::TurnStarted(started) => {
                if state
                    .completed
                    .contains_key(&(thread_id, started.turn.id.clone()))
                {
                    return;
                }
                state.running.insert(thread_id, (true, refresh_id));
                if state
                    .active
                    .get(&thread_id)
                    .is_none_or(|turn| turn.turn_id != started.turn.id)
                {
                    state.active.insert(
                        thread_id,
                        ActiveTurnAgents {
                            turn_id: started.turn.id.clone(),
                            agents: HashSet::new(),
                        },
                    );
                }
            }
            ServerNotification::TurnCompleted(completed) => {
                if state
                    .active
                    .get(&thread_id)
                    .is_some_and(|turn| turn.turn_id != completed.turn.id)
                {
                    return;
                }
                state.running.insert(thread_id, (false, refresh_id));
            }
            ServerNotification::ThreadStatusChanged(status) => {
                state.running.insert(
                    thread_id,
                    (
                        matches!(status.status, ThreadStatus::Active { .. }),
                        refresh_id,
                    ),
                );
            }
            ServerNotification::ThreadClosed(_)
            | ServerNotification::ThreadArchived(_)
            | ServerNotification::ThreadDeleted(_) => {
                state.running.insert(thread_id, (false, refresh_id));
                state.active.remove(&thread_id);
            }
            ServerNotification::ThreadStarted(_) => {}
            _ => return,
        }

        self.sample_turn_agents();

        let state = &mut self.turn_agent_counts;
        if let ServerNotification::TurnCompleted(completed) = notification
            && let Some(turn) = state.active.remove(&thread_id)
        {
            state
                .completed
                .insert((thread_id, completed.turn.id.clone()), turn.agents.len());
        }
    }

    pub(super) fn sample_turn_agents(&mut self) {
        let state = &mut self.turn_agent_counts;
        let threads = &self.agents_overview.threads;
        for (parent_id, turn) in &mut state.active {
            for (agent_id, thread) in threads {
                let Some(thread) = thread else { continue };
                let running = state.running.get(agent_id).map_or_else(
                    || matches!(thread.status, ThreadStatus::Active { .. }),
                    |(running, _)| *running,
                );
                if agent_id == parent_id || !running {
                    continue;
                }
                let mut ancestor = thread;
                // Follow only subagent edges, bounding traversal in case ancestry contains a cycle.
                for _ in 0..threads.len() {
                    let SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                        parent_thread_id,
                        ..
                    }) = &ancestor.source
                    else {
                        break;
                    };
                    if parent_thread_id == parent_id {
                        turn.agents.insert(*agent_id);
                        break;
                    }
                    let Some(Some(parent)) = threads.get(parent_thread_id) else {
                        break;
                    };
                    ancestor = parent;
                }
            }
        }
    }
}
