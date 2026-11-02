use crate::config::Config;
use codex_protocol::ThreadId;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

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

pub(super) enum ReviewReservationKind {
    Reviewer,
    Explorer { owner: ThreadId },
}

pub(super) struct ReviewReservation {
    quota: Arc<ReviewQuota>,
    kind: ReviewReservationKind,
    bound: Option<ThreadId>,
    committed: bool,
}

impl ReviewQuota {
    pub(super) fn reserve_for_source(
        self: &Arc<Self>,
        config: &Config,
        session_source: Option<&codex_protocol::protocol::SessionSource>,
        root_thread_id: Option<ThreadId>,
    ) -> Result<Option<ReviewReservation>> {
        if let Some(source) = session_source {
            let parent_id = source.parent_thread_id();
            if parent_id.is_some_and(|parent| self.is_helper(parent)) {
                return Err(CodexErr::InvalidRequest(
                    "protected explorer helpers cannot create descendants".to_string(),
                ));
            }
            let parent_is_reviewer = parent_id.is_some_and(|parent| {
                self.owner(parent) == Some(parent) && !self.is_helper(parent)
            });
            let kind = if crate::protected_reviewer::is_helper(source) {
                if !parent_is_reviewer {
                    return Err(CodexErr::InvalidRequest(
                        "protected explorer requires trusted reviewer owner".to_string(),
                    ));
                }
                Some(ReviewReservationKind::Explorer {
                    owner: parent_id.expect("validated reviewer parent"),
                })
            } else if parent_is_reviewer {
                return Err(CodexErr::InvalidRequest(
                    "protected reviewers may create only trusted explorers".to_string(),
                ));
            } else if crate::protected_reviewer::is_reviewer(source) {
                if parent_id.is_none() || parent_id != root_thread_id {
                    return Err(CodexErr::InvalidRequest(
                        "only the tree root can create protected reviewers".to_string(),
                    ));
                }
                Some(ReviewReservationKind::Reviewer)
            } else {
                None
            };
            kind.map(|kind| self.reserve(config, kind)).transpose()
        } else {
            Ok(None)
        }
    }

    pub(super) fn reserve(
        self: &Arc<Self>,
        config: &Config,
        kind: ReviewReservationKind,
    ) -> Result<ReviewReservation> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let requested = (
            config.reviewer_subagent.max_concurrent_reviews.get(),
            config.reviewer_subagent.max_explorers,
        );
        requested
            .0
            .checked_mul(requested.1.checked_add(1).ok_or_else(|| {
                CodexErr::InvalidRequest("reviewer capacity overflow".to_string())
            })?)
            .ok_or_else(|| CodexErr::InvalidRequest("reviewer capacity overflow".to_string()))?;
        if let Some(limits) = state.limits {
            if limits != requested {
                return Err(CodexErr::InvalidRequest(
                    "reviewer quotas are frozen for this tree".to_string(),
                ));
            }
        } else {
            state.limits = Some(requested);
        }
        match kind {
            ReviewReservationKind::Reviewer => {
                let active = state
                    .reviews
                    .values()
                    .filter(|review| !review.closed || review.pending_helpers > 0)
                    .count();
                if active.saturating_add(state.pending_reviews) >= requested.0 {
                    return Err(CodexErr::InvalidRequest(
                        "reviewer quota reached; close the previous reviewer subtree first"
                            .to_string(),
                    ));
                }
                state.pending_reviews += 1;
            }
            ReviewReservationKind::Explorer { owner } => {
                let review = state.reviews.get_mut(&owner).ok_or_else(|| {
                    CodexErr::InvalidRequest("trusted reviewer owner is unavailable".to_string())
                })?;
                if review.closed
                    || review.closing
                    || review
                        .helpers_created
                        .saturating_add(review.pending_helpers)
                        >= requested.1
                {
                    return Err(CodexErr::InvalidRequest(
                        "reviewer lifetime explorer quota reached".to_string(),
                    ));
                }
                review.pending_helpers += 1;
            }
        }
        Ok(ReviewReservation {
            quota: Arc::clone(self),
            kind,
            bound: None,
            committed: false,
        })
    }

    pub(super) fn owner(&self, member: ThreadId) -> Option<ThreadId> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.reviews.contains_key(&member) {
            Some(member)
        } else {
            state.helpers.get(&member).copied()
        }
    }

    pub(super) fn is_helper(&self, member: ThreadId) -> bool {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .helpers
            .contains_key(&member)
    }

    pub(super) fn capacity(&self) -> usize {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .limits
            .map_or(0, |(reviews, helpers)| reviews * (1 + helpers))
    }

    pub(super) fn begin_close(&self, owner: ThreadId) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(review) = state.reviews.get_mut(&owner) else {
            return false;
        };
        review.closing = true;
        true
    }

    pub(super) fn pending_helpers(&self, owner: ThreadId) -> bool {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .reviews
            .get(&owner)
            .is_some_and(|review| review.pending_helpers > 0)
    }

    pub(super) fn helper_ids(&self, owner: ThreadId) -> Vec<ThreadId> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .helpers
            .iter()
            .filter_map(|(helper, reviewer)| (*reviewer == owner).then_some(*helper))
            .collect()
    }

    pub(super) fn subtree_closed(&self, owner: ThreadId) {
        if let Some(review) = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .reviews
            .get_mut(&owner)
        {
            review.closed = true;
        }
    }
}

impl ReviewReservation {
    pub(super) fn fail_closed(&mut self) {
        self.committed = true;
    }

    pub(super) fn bind(&mut self, member: ThreadId) {
        let mut state = self
            .quota
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match self.kind {
            ReviewReservationKind::Reviewer => {
                state.pending_reviews -= 1;
                state.reviews.insert(member, ReviewBudget::default());
            }
            ReviewReservationKind::Explorer { owner } => {
                state.helpers.insert(member, owner);
            }
        }
        self.bound = Some(member);
    }

    pub(super) fn commit(&mut self) -> Result<()> {
        let mut state = self
            .quota
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match self.kind {
            ReviewReservationKind::Reviewer => {}
            ReviewReservationKind::Explorer { owner } => {
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
        let mut state = self
            .quota
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match self.kind {
            ReviewReservationKind::Reviewer => {
                if let Some(member) = self.bound {
                    state.reviews.remove(&member);
                } else {
                    state.pending_reviews -= 1;
                }
            }
            ReviewReservationKind::Explorer { owner } => {
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
            .fail_closed();
    }
}

use super::LocalAgentControl;
use codex_protocol::AgentPath;
use codex_protocol::error::CodexErrorDetails;

impl LocalAgentControl {
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
        let result = match self.shutdown_live_agent(agent_id).await {
            Err(err)
                if matches!(
                    err.details(),
                    CodexErrorDetails::ThreadNotFound(_) | CodexErrorDetails::InternalAgentDied
                ) =>
            {
                Ok(String::new())
            }
            result => result,
        };
        let descendant_ids = {
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
