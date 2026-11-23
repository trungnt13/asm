use std::sync::Arc;

use crate::hook_runtime::PostCompactHookOutcome;
use crate::hook_runtime::PreCompactHookOutcome;
use crate::hook_runtime::run_post_compact_hooks;
use crate::hook_runtime::run_pre_compact_hooks;
use crate::session::session::Session;
use crate::session::step_context::StepContext;
use codex_analytics::CompactionTrigger;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::items::ContextCompactionItem;
use codex_protocol::items::TurnItem;

/// Honor an explicit `new_context` request without summarizing the old window.
pub(crate) async fn run_context_reset(
    sess: &Arc<Session>,
    step_context: &StepContext,
) -> CodexResult<bool> {
    let turn_context = &step_context.turn;
    if !sess.inline_compaction_allowed(turn_context)? {
        return Ok(false);
    }
    let world_state = Arc::new(
        sess.build_world_state_for_step(step_context, /*new_window*/ true)
            .await?,
    );
    if !sess.inline_compaction_allowed(turn_context)? {
        return Ok(false);
    }
    let _profile_guard = turn_context.turn_timing_state.begin_compaction();
    let trigger = CompactionTrigger::Auto;
    let pre_compact_outcome = run_pre_compact_hooks(sess, turn_context, trigger).await;
    match pre_compact_outcome {
        PreCompactHookOutcome::Continue => {}
        PreCompactHookOutcome::Stopped => return Err(CodexErr::TurnAborted),
    }

    let compaction_item = TurnItem::ContextCompaction(ContextCompactionItem::new());
    sess.emit_turn_item_started(turn_context, &compaction_item)
        .await;
    // This reset installs history without making a remote compaction request.
    let trace = sess.services.rollout_thread_trace.compaction_trace_context(
        &turn_context.sub_id,
        compaction_item.id(),
        &turn_context.model_info().slug,
        &turn_context.provider.info().name,
    );
    sess.start_new_context_window(step_context, world_state, &trace)
        .await;
    sess.emit_turn_item_completed(turn_context, compaction_item)
        .await;

    let post_compact_outcome = run_post_compact_hooks(sess, turn_context, trigger).await;
    if let PostCompactHookOutcome::Stopped = post_compact_outcome {
        return Err(CodexErr::TurnAborted);
    }

    Ok(true)
}
