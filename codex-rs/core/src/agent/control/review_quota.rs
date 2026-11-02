use crate::config::Config;
use codex_protocol::ThreadId;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;

#[derive(Default)]
pub(super) struct ReviewQuota {
    state: Mutex<QuotaState>,
}

#[derive(Default)]
struct QuotaState {
    limits: Option<(usize, usize)>,
    pending_reviews: usize,
    reviews: HashMap<ThreadId, ReviewBudget>,
    helpers: HashMap<ThreadId, ThreadId>,
}

#[derive(Default)]
struct ReviewBudget {
    helpers_created: usize,
    pending_helpers: usize,
    closed: bool,
    closing: bool,
}

pub(super) struct ReviewReservation {
    quota: Arc<ReviewQuota>,
    owner: Option<ThreadId>,
    bound: Option<ThreadId>,
    committed: bool,
}

impl ReviewQuota {
    fn lock(&self) -> MutexGuard<'_, QuotaState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(super) fn reserve_for_source(
        self: &Arc<Self>,
        config: &Config,
        source: Option<&codex_protocol::protocol::SessionSource>,
        root: Option<ThreadId>,
    ) -> Result<Option<ReviewReservation>> {
        let Some(source) = source else {
            return Ok(None);
        };
        let mut state = self.lock();
        let parent = source.parent_thread_id();
        let parent_is_reviewer = parent.is_some_and(|id| state.reviews.contains_key(&id));
        let helper = crate::protected_reviewer::is_helper(source);
        let reviewer = crate::protected_reviewer::is_reviewer(source);
        if parent.is_some_and(|id| state.helpers.contains_key(&id))
            || (parent_is_reviewer && !helper)
            || (helper && !parent_is_reviewer)
            || (reviewer && (parent.is_none() || parent != root))
        {
            return Err(CodexErr::InvalidRequest(
                "only the tree root may create reviewers; reviewers may create only trusted explorers with no descendants".to_string(),
            ));
        }
        if !helper && !reviewer {
            return Ok(None);
        }
        let limits = (
            config.reviewer_subagent.max_concurrent_reviews.get(),
            config.reviewer_subagent.max_explorers,
        );
        limits
            .1
            .checked_add(1)
            .and_then(|helpers| helpers.checked_mul(limits.0))
            .ok_or_else(|| CodexErr::InvalidRequest("reviewer capacity overflow".to_string()))?;
        if state.limits.is_some_and(|frozen| frozen != limits) {
            return Err(CodexErr::InvalidRequest(
                "reviewer quotas are frozen for this tree".to_string(),
            ));
        }
        state.limits = Some(limits);
        let owner = helper.then(|| parent.expect("validated reviewer parent"));
        if let Some(owner) = owner {
            let review = state
                .reviews
                .get_mut(&owner)
                .expect("validated reviewer parent");
            if review.closed
                || review.closing
                || review
                    .helpers_created
                    .saturating_add(review.pending_helpers)
                    >= limits.1
            {
                return Err(CodexErr::InvalidRequest(
                    "reviewer lifetime explorer quota reached".to_string(),
                ));
            }
            review.pending_helpers += 1;
        } else {
            let active = state
                .reviews
                .values()
                .filter(|review| !review.closed || review.pending_helpers > 0)
                .count();
            if active.saturating_add(state.pending_reviews) >= limits.0 {
                return Err(CodexErr::InvalidRequest(
                    "reviewer quota reached; close the previous reviewer subtree first".to_string(),
                ));
            }
            state.pending_reviews += 1;
        }
        Ok(Some(ReviewReservation {
            quota: Arc::clone(self),
            owner,
            bound: None,
            committed: false,
        }))
    }

    pub(super) fn owner(&self, member: ThreadId) -> Option<ThreadId> {
        let state = self.lock();
        if state.reviews.contains_key(&member) {
            Some(member)
        } else {
            state.helpers.get(&member).copied()
        }
    }

    pub(super) fn capacity(&self) -> usize {
        let state = self.lock();
        state
            .limits
            .map_or(0, |(reviews, helpers)| reviews * (1 + helpers))
    }

    pub(super) fn begin_close(&self, owner: ThreadId) -> bool {
        let mut state = self.lock();
        let Some(review) = state.reviews.get_mut(&owner) else {
            return false;
        };
        review.closing = true;
        true
    }
}

impl ReviewReservation {
    pub(super) fn bind(&mut self, member: ThreadId) {
        let mut state = self.quota.lock();
        match self.owner {
            None => {
                state.pending_reviews -= 1;
                state.reviews.insert(member, ReviewBudget::default());
            }
            Some(owner) => {
                state.helpers.insert(member, owner);
            }
        }
        self.bound = Some(member);
    }

    pub(super) fn commit(&mut self) -> Result<()> {
        let mut state = self.quota.lock();
        match self.owner {
            None => {}
            Some(owner) => {
                let review = state
                    .reviews
                    .get_mut(&owner)
                    .expect("reserved reviewer retained");
                if review.closed || review.closing {
                    return Err(CodexErr::InvalidRequest(
                        "reviewer closed during explorer startup".to_string(),
                    ));
                }
                review.pending_helpers -= 1;
                review.helpers_created += 1;
            }
        }
        self.committed = true;
        Ok(())
    }
}

impl Drop for ReviewReservation {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let mut state = self.quota.lock();
        match self.owner {
            None => {
                if let Some(member) = self.bound {
                    state.reviews.remove(&member);
                } else {
                    state.pending_reviews -= 1;
                }
            }
            Some(owner) => {
                if let Some(review) = state.reviews.get_mut(&owner) {
                    review.pending_helpers -= 1;
                }
                if let Some(member) = self.bound {
                    state.helpers.remove(&member);
                }
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct ReviewStartupQuota(Arc<Mutex<ReviewReservation>>);

impl ReviewStartupQuota {
    pub(super) fn new(reservation: ReviewReservation) -> Self {
        Self(Arc::new(Mutex::new(reservation)))
    }
    pub(super) fn commit(&self) -> Result<()> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .commit()
    }
    pub(crate) fn fail_closed(&self) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .committed = true;
    }
}

use super::LocalAgentControl;
use codex_protocol::AgentPath;
use codex_protocol::error::CodexErrorDetails;

impl LocalAgentControl {
    #[tracing::instrument(skip_all)]
    pub(super) async fn admit_review_message(
        &self,
        caller: ThreadId,
        target: ThreadId,
        input: &crate::agent::api::AgentInput,
    ) -> Result<bool> {
        let protected = self.get_agent_metadata(target).is_some_and(|metadata| {
            matches!(
                metadata.agent_role.as_deref(),
                Some(crate::protected_reviewer::ROLE | crate::protected_reviewer::HELPER_ROLE)
            )
        });
        let helper_owner = self
            .runtime
            .review_quota
            .owner(caller)
            .filter(|owner| *owner != caller);
        if !protected && helper_owner.is_none() {
            return Ok(false);
        }
        if helper_owner != Some(target)
            || self.runtime.review_quota.owner(target) != Some(target)
            || !matches!(
                input,
                crate::agent::api::AgentInput::Message {
                    mode: crate::agent::types::MessageDeliveryMode::QueueOnly,
                    ..
                }
            )
            || self
                .runtime
                .upgrade()?
                .get_thread(target)
                .await?
                .session
                .active_turn
                .lock()
                .await
                .is_none()
        {
            return Err(CodexErr::InvalidRequest(
                "protected reviewers accept only evidence from their own explorers during an active review".to_string(),
            ));
        }
        Ok(protected)
    }

    #[tracing::instrument(skip_all)]
    pub(crate) async fn close_review_agent(
        &self,
        caller: ThreadId,
        target: ThreadId,
    ) -> Result<()> {
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

    #[tracing::instrument(skip_all)]
    pub(super) async fn shutdown_review_tree(&self, agent_id: ThreadId) -> Result<String> {
        let result = self.stop_review_member(agent_id).await;
        let quota = &self.runtime.review_quota;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        while quota
            .lock()
            .reviews
            .get(&agent_id)
            .is_some_and(|review| review.pending_helpers > 0)
        {
            if tokio::time::Instant::now() >= deadline {
                return Err(CodexErr::InvalidRequest(
                    "reviewer startup cleanup did not drain; review quota retained".to_string(),
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let helpers: Vec<_> = quota
            .lock()
            .helpers
            .iter()
            .filter_map(|(helper, owner)| (*owner == agent_id).then_some(*helper))
            .collect();
        for helper in helpers {
            self.stop_review_member(helper).await?;
        }
        if result.is_ok()
            && let Some(review) = quota.lock().reviews.get_mut(&agent_id)
        {
            review.closed = true;
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
