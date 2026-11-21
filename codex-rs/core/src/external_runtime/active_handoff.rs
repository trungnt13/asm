use crate::context_manager::ContextManager;
use crate::session::step_context::StepContext;
use codex_history::InitialHistory;
use codex_history::RolloutItem;
use codex_protocol::error::CodexErr;
use std::collections::HashMap;
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

#[tracing::instrument(skip_all, err)]
pub(crate) async fn active_handoff_history(step: &StepContext) -> Result<InitialHistory, CodexErr> {
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
    Ok(InitialHistory::Forked(
        history
            .annotated_items()
            .iter()
            .cloned()
            .map(RolloutItem::ResponseItem)
            .collect(),
    ))
}
