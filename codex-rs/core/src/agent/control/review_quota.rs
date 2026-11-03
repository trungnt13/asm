use super::LocalAgentControl;
use crate::config::Config;
use codex_protocol::AgentPath;
use codex_protocol::ThreadId;
use codex_protocol::error::CodexErr;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::error::Result;
use codex_protocol::protocol::SessionSource;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;

#[derive(Default)]
pub(super) struct ReviewQuota(Mutex<QuotaState>);

#[derive(Default)]
struct QuotaState {
    limits: Option<(usize, usize)>,
    members: HashMap<ThreadId, Member>,
}

struct Member {
    owner: Option<ThreadId>,
    phase: Phase,
    closing: bool,
    closed: bool,
}

#[derive(PartialEq)]
enum Phase {
    Starting,
    Running,
    Failed,
}

#[derive(Clone)]
pub(crate) struct ReviewStartupQuota(Arc<StartupLease>);

struct StartupLease {
    quota: Arc<ReviewQuota>,
    id: ThreadId,
}

impl ReviewQuota {
    fn lock(&self) -> MutexGuard<'_, QuotaState> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(super) fn reserve_for_source(
        self: &Arc<Self>,
        config: &Config,
        source: Option<&SessionSource>,
        root: Option<ThreadId>,
        id: ThreadId,
    ) -> Result<Option<ReviewStartupQuota>> {
        let Some(source) = source else {
            return Ok(None);
        };
        let mut state = self.lock();
        let parent = source.parent_thread_id();
        let owner = parent.filter(|parent| state.members.contains_key(parent));
        let helper = crate::protected_reviewer::is_helper(source);
        let reviewer = crate::protected_reviewer::is_reviewer(source);
        if owner.is_some_and(|parent| state.members[&parent].owner.is_some() || !helper)
            || (helper && owner.is_none())
            || (reviewer && (parent.is_none() || parent != root))
        {
            return Err(CodexErr::InvalidRequest("only root may create reviewers; reviewers may create only trusted explorers without descendants".to_string()));
        }
        if !helper && !reviewer {
            return Ok(None);
        }
        let limits = (
            config.reviewer_subagent.max_concurrent_reviews.get(),
            config.reviewer_subagent.max_explorers,
        );
        if state.limits.is_some_and(|frozen| frozen != limits) {
            return Err(CodexErr::InvalidRequest(
                "reviewer quotas are frozen for this tree".to_string(),
            ));
        }
        state.limits = Some(limits);
        if let Some(owner) = owner {
            let review = &state.members[&owner];
            if review.closing
                || review.closed
                || state
                    .members
                    .values()
                    .filter(|member| member.owner == Some(owner))
                    .count()
                    >= limits.1
            {
                return Err(CodexErr::InvalidRequest(
                    "reviewer lifetime explorer quota reached".to_string(),
                ));
            }
        } else if state
            .members
            .values()
            .filter(|member| member.owner.is_none() && !member.closed)
            .count()
            >= limits.0
        {
            return Err(CodexErr::InvalidRequest(
                "reviewer quota reached; close the previous reviewer subtree first".to_string(),
            ));
        }
        state.members.insert(
            id,
            Member {
                owner,
                phase: Phase::Starting,
                closing: false,
                closed: false,
            },
        );
        Ok(Some(ReviewStartupQuota(Arc::new(StartupLease {
            quota: Arc::clone(self),
            id,
        }))))
    }

    pub(super) fn owner(&self, member: ThreadId) -> Option<ThreadId> {
        self.lock()
            .members
            .get(&member)
            .map(|entry| entry.owner.unwrap_or(member))
    }

    pub(super) fn begin_close(&self, owner: ThreadId) -> bool {
        let mut state = self.lock();
        let Some(member) = state
            .members
            .get_mut(&owner)
            .filter(|member| member.owner.is_none())
        else {
            return false;
        };
        member.closing = true;
        true
    }
}

impl ReviewStartupQuota {
    pub(super) fn commit(&self) -> Result<()> {
        let mut state = self.0.quota.lock();
        let owner = state.members[&self.0.id].owner;
        if owner.is_some_and(|owner| state.members[&owner].closing) {
            return Err(CodexErr::InvalidRequest(
                "reviewer closed during explorer startup".to_string(),
            ));
        }
        state
            .members
            .get_mut(&self.0.id)
            .expect("startup lease retained")
            .phase = Phase::Running;
        Ok(())
    }
    pub(crate) fn fail_closed(&self) {
        self.0
            .quota
            .lock()
            .members
            .get_mut(&self.0.id)
            .expect("startup lease retained")
            .phase = Phase::Failed;
    }
}

impl Drop for StartupLease {
    fn drop(&mut self) {
        let mut state = self.quota.lock();
        if state
            .members
            .get(&self.id)
            .is_some_and(|member| member.phase == Phase::Starting)
        {
            state.members.remove(&self.id);
        }
    }
}

impl LocalAgentControl {
    #[tracing::instrument(skip_all)]
    pub(crate) async fn close_review_agent(
        &self,
        caller: ThreadId,
        target: ThreadId,
    ) -> Result<()> {
        if self.runtime.review_quota.owner(target) != Some(target)
            || self.runtime.registry.agent_id_for_path(&AgentPath::root()) != Some(caller)
        {
            return Err(CodexErr::InvalidRequest(
                "only the tree root can close its reviewer subtree".to_string(),
            ));
        }
        self.shutdown_agent_tree(target).await.map(|_| ())
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn shutdown_review_tree(&self, agent_id: ThreadId) -> Result<String> {
        let result = self.stop_review_member(agent_id).await;
        let quota = &self.runtime.review_quota;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let (pending, failed) = {
                let state = quota.lock();
                state
                    .members
                    .iter()
                    .filter(|(id, member)| **id == agent_id || member.owner == Some(agent_id))
                    .fold((false, false), |(pending, failed), (_, member)| {
                        (
                            pending || member.phase == Phase::Starting,
                            failed || member.phase == Phase::Failed,
                        )
                    })
            };
            if failed {
                return Err(CodexErr::InvalidRequest(
                    "review startup cleanup failed; review quota retained".to_string(),
                ));
            }
            if !pending {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(CodexErr::InvalidRequest(
                    "reviewer startup cleanup did not drain; review quota retained".to_string(),
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let helpers: Vec<_> = quota
            .lock()
            .members
            .iter()
            .filter_map(|(id, member)| (member.owner == Some(agent_id)).then_some(*id))
            .collect();
        for helper in helpers {
            self.stop_review_member(helper).await?;
        }
        if result.is_ok()
            && let Some(member) = quota.lock().members.get_mut(&agent_id)
        {
            member.closed = true;
        }
        result
    }

    #[tracing::instrument(skip_all)]
    async fn stop_review_member(&self, member: ThreadId) -> Result<String> {
        match self.shutdown_live_agent(member).await {
            Err(error)
                if matches!(
                    error.details(),
                    CodexErrorDetails::ThreadNotFound(_) | CodexErrorDetails::InternalAgentDied
                ) =>
            {
                Ok(String::new())
            }
            result => result,
        }
    }
}
