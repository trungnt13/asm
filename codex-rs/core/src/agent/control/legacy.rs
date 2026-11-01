use super::*;
use crate::agent::api::AgentInfo;
use codex_protocol::error::CodexErrorDetails;
use codex_thread_store::PersistContext;

impl LocalAgentControl {
    /// Submit a shutdown request for a live agent without marking it explicitly closed in
    /// persisted spawn-edge state.
    pub(crate) async fn shutdown_live_agent(&self, agent_id: ThreadId) -> CodexResult<String> {
        let state = self.runtime.upgrade()?;
        let result = if let Ok(thread) = state.get_thread(agent_id).await {
            thread
                .session
                .ensure_rollout_materialized(PersistContext::Standard)
                .await;
            thread.session.flush_rollout().await?;
            let result = if matches!(thread.agent_status().await, AgentStatus::Shutdown) {
                Ok(String::new())
            } else {
                state
                    .send_op(
                        agent_id,
                        Op::Shutdown {},
                        /*parent_turn_id*/ None,
                        /*root_turn_id*/ None,
                        /*message_delivery*/ None,
                    )
                    .await
            };
            thread.wait_until_terminated().await;
            result
        } else {
            state
                .send_op(
                    agent_id,
                    Op::Shutdown {},
                    /*parent_turn_id*/ None,
                    /*root_turn_id*/ None,
                    /*message_delivery*/ None,
                )
                .await
        };
        let _ = state.remove_thread(&agent_id).await;
        self.forget_v2_residency(agent_id);
        self.runtime.registry.release_spawned_thread(agent_id);
        self.runtime.mailboxes.remove(agent_id);
        result
    }

    /// Mark `agent_id` as explicitly closed in persisted spawn-edge state, then shut down the
    /// agent and any live descendants reached from the in-memory tree.
    pub(crate) async fn close_agent(&self, agent_id: ThreadId) -> CodexResult<AgentInfo> {
        let state = self.runtime.upgrade()?;
        let metadata = self.get_agent_metadata(agent_id);
        let known_agent = metadata.is_some();
        let snapshot = match state.get_thread(agent_id).await {
            Ok(thread) => {
                let agent = LiveAgent {
                    thread_id: agent_id,
                    metadata: metadata.unwrap_or_default(),
                    status: thread.agent_status().await,
                };
                let config = Box::new(thread.config_snapshot().await);
                if !config.ephemeral
                    && let Some(agent_graph_store) = state.agent_graph_store()
                    && let Err(err) = agent_graph_store
                        .set_thread_spawn_edge_status(
                            agent_id,
                            codex_agent_graph_store::ThreadSpawnEdgeStatus::Closed,
                        )
                        .await
                {
                    warn!("failed to persist thread-spawn edge status for {agent_id}: {err}");
                }
                AgentInfo::Loaded { agent, config }
            }
            Err(err)
                if known_agent && matches!(err.details(), CodexErrorDetails::ThreadNotFound(_)) =>
            {
                if let Some(agent_graph_store) = state.agent_graph_store()
                    && let Err(err) = agent_graph_store
                        .set_thread_spawn_edge_status(
                            agent_id,
                            codex_agent_graph_store::ThreadSpawnEdgeStatus::Closed,
                        )
                        .await
                {
                    return Err(CodexErr::Fatal(format!(
                        "failed to persist stale thread-spawn edge status for {agent_id}: {err}"
                    )));
                }
                AgentInfo::Unloaded(metadata.unwrap_or_default())
            }
            Err(err) => return Err(err),
        };
        match Box::pin(self.shutdown_agent_tree(agent_id)).await {
            Err(err)
                if known_agent
                    && matches!(
                        err.details(),
                        CodexErrorDetails::ThreadNotFound(_) | CodexErrorDetails::InternalAgentDied
                    ) =>
            {
                Ok(snapshot)
            }
            result => result.map(|_| snapshot),
        }
    }

    #[tracing::instrument(skip_all)]
    pub(crate) async fn close_review_agent(
        &self,
        caller: ThreadId,
        target: ThreadId,
    ) -> CodexResult<()> {
        let owner = self.runtime.review_quota.owner(target).ok_or_else(|| {
            CodexErr::InvalidRequest("target is not a protected review member".to_string())
        })?;
        let allowed = if owner == target {
            self.runtime.registry.agent_id_for_path(&AgentPath::root()) == Some(caller)
        } else {
            owner == caller
        };
        if !allowed {
            return Err(CodexErr::InvalidRequest(
                "only the tree root can close a reviewer; only its reviewer can close a helper"
                    .to_string(),
            ));
        }
        self.shutdown_agent_tree(target).await.map(|_| ())
    }

    /// Shut down `agent_id` and any live descendants reachable from the in-memory spawn tree.
    pub(crate) async fn shutdown_agent_tree(&self, agent_id: ThreadId) -> CodexResult<String> {
        let protected_close = self.runtime.review_quota.begin_close(agent_id);
        let descendant_ids = if protected_close {
            Vec::new()
        } else {
            self.runtime.live_thread_spawn_descendants(agent_id).await?
        };
        let result = self.shutdown_live_agent(agent_id).await;
        let descendant_ids = if protected_close {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
            while self.runtime.review_quota.pending_helpers(agent_id) {
                if tokio::time::Instant::now() >= deadline {
                    return Err(CodexErr::InvalidRequest(
                        "reviewer startup cleanup did not drain; review quota retained".to_string(),
                    ));
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            self.runtime.review_quota.helper_ids(agent_id)
        } else {
            descendant_ids
        };
        for descendant_id in descendant_ids {
            match self.shutdown_live_agent(descendant_id).await {
                Ok(_) => {}
                Err(err)
                    if matches!(
                        err.details(),
                        CodexErrorDetails::ThreadNotFound(_) | CodexErrorDetails::InternalAgentDied
                    ) => {}
                Err(err) => return Err(err),
            }
        }
        if result.is_ok() {
            self.runtime.review_quota.subtree_closed(agent_id);
        }
        result
    }
}
