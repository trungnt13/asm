use crate::agent::types::NativeForkCut;
use crate::context_manager::ContextManager;
use crate::session::session::Session;
use crate::session::step_context::StepContext;
use codex_history::InitialHistory;
use codex_history::RolloutItem;
use codex_protocol::error::CodexErr;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::time::Duration;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

pub(super) type HandoffCapture = watch::Sender<Option<HandoffCut>>;

pub(super) struct HandoffCut {
    pub(super) tools: HashMap<String, String>,
    pub(super) history: ContextManager,
}

pub(super) struct HandoffInvocation {
    pub(super) tool_use_id: String,
    pub(super) tool_name: String,
    pub(super) active: CancellationToken,
}

// Only Cursor's stdout reader advances this watermark, after recording the assistant text.
// An HTTP callback can arrive before stdout, and yielded code can outlive the callback.
pub(super) type CursorHandoffCapture = watch::Sender<VecDeque<(String, usize)>>;

pub(super) struct CursorHandoffInvocation {
    pub(super) tool_use_id: String,
    pub(super) active: CancellationToken,
}

#[tracing::instrument(skip_all, err)]
pub(crate) async fn active_handoff_history(
    session: &Session,
    step: &StepContext,
) -> Result<NativeForkCut, CodexErr> {
    if let Some(capture) = step.turn.extension_data.get::<CursorHandoffCapture>() {
        let unverified =
            || CodexErr::InvalidRequest("No matching foreground Cursor fork cut".into());
        let invocation = step
            .extension_data
            .get::<CursorHandoffInvocation>()
            .ok_or_else(unverified)?;
        let count = {
            let mut latest = capture.subscribe();
            let cuts = tokio::select! {
                biased;
                _ = invocation.active.cancelled() => return Err(unverified()),
                result = tokio::time::timeout(
                    Duration::from_secs(/*secs*/ 2),
                    latest.wait_for(|cuts| cuts.iter().any(|(id, _)| id == &invocation.tool_use_id)),
                ) => result.ok().and_then(Result::ok).ok_or_else(unverified)?,
            };
            cuts.iter()
                .find_map(|(id, count)| (id == &invocation.tool_use_id).then_some(*count))
                .ok_or_else(unverified)?
        };
        let items = session.shared_history_items().await;
        if invocation.active.is_cancelled() || count > items.len() {
            return Err(unverified());
        }
        return Ok(NativeForkCut::LivePrefix { items, len: count });
    }
    let unverified = || CodexErr::InvalidRequest("No matching unsteered Claude fork cut".into());
    let invocation = step
        .extension_data
        .get::<HandoffInvocation>()
        .ok_or_else(unverified)?;
    let mut latest = step
        .turn
        .extension_data
        .get::<HandoffCapture>()
        .ok_or_else(unverified)?
        .subscribe();
    let cut = tokio::time::timeout(
        Duration::from_secs(/*secs*/ 2),
        latest.wait_for(|cut| {
            cut.as_ref().is_some_and(|cut| {
                cut.tools.get(&invocation.tool_use_id) == Some(&invocation.tool_name)
            })
        }),
    )
    .await
    .ok()
    .and_then(Result::ok)
    .ok_or_else(unverified)?;
    let history = &cut
        .as_ref()
        .filter(|_| !invocation.active.is_cancelled())
        .ok_or_else(unverified)?
        .history;
    Ok(NativeForkCut::Rollout(InitialHistory::Forked(
        history
            .annotated_items()
            .iter()
            .cloned()
            .map(RolloutItem::ResponseItem)
            .collect(),
    )))
}
