//! Counts distinct subagents observed running during live turns.
//! Completed counts stay client-local so switching views cannot change a finished footer.

use super::App;
use super::app_server_event_targets::ServerNotificationThreadTarget;
use super::app_server_event_targets::server_notification_thread_target;
use crate::multi_agents::sub_agent_activity_display;
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
    // V2 spawn activity can arrive without a child thread metadata notification.
    parents: HashMap<ThreadId, ThreadId>,
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

    fn record_active_agent(&mut self, agent_id: ThreadId) {
        let mut ancestor = agent_id;
        // Follow only subagent edges, bounding traversal in case ancestry contains a cycle.
        for _ in 0..self.parents.len() {
            let Some(parent_id) = self.parents.get(&ancestor) else {
                break;
            };
            if *parent_id == agent_id {
                break;
            }
            if let Some(turn) = self.active.get_mut(parent_id) {
                turn.agents.insert(agent_id);
            }
            ancestor = *parent_id;
        }
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
        let activity = match notification {
            ServerNotification::ItemStarted(item) => Some((&item.turn_id, &item.item)),
            ServerNotification::ItemCompleted(item) => Some((&item.turn_id, &item.item)),
            _ => None,
        }
        .and_then(|(turn_id, item)| {
            sub_agent_activity_display(item).map(|activity| (turn_id, activity))
        });
        if let Some((turn_id, activity)) = activity {
            let agent_id = activity.thread_id;
            if activity.is_running_hint {
                state.parents.insert(agent_id, thread_id);
                // A fast child may have finished before its parent's spawn item arrives.
                state.running.entry(agent_id).or_insert((true, refresh_id));
                if state
                    .active
                    .get(&thread_id)
                    .is_none_or(|turn| &turn.turn_id == turn_id)
                {
                    state.record_active_agent(agent_id);
                }
            } else if !state.active.contains_key(&agent_id) {
                // Do not stop a newer child turn because its earlier completion arrived late.
                state.running.insert(agent_id, (false, refresh_id));
            }
            self.sample_turn_agents();
            return;
        }
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
        for (agent_id, thread) in threads {
            if let Some(thread) = thread
                && let SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                    parent_thread_id, ..
                }) = &thread.source
            {
                state.parents.insert(*agent_id, *parent_thread_id);
            }
        }
        let running_agents: Vec<_> = state
            .parents
            .keys()
            .copied()
            .filter(|agent_id| {
                state.running.get(agent_id).map_or_else(
                    || {
                        threads
                            .get(agent_id)
                            .and_then(Option::as_ref)
                            .is_some_and(|thread| {
                                matches!(thread.status, ThreadStatus::Active { .. })
                            })
                    },
                    |(running, _)| *running,
                )
            })
            .collect();
        for agent_id in running_agents {
            state.record_active_agent(agent_id);
        }
    }
}
