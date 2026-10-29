//! Temporary native Claude forks for side conversations.

use super::input_messages;
use super::input_prefix;
use super::state::Checkpoint;
use super::state::ForkSeed;
use crate::session::session::Session;
use codex_history::RolloutItem;
use codex_protocol::error::CodexErr;

/// Sent once to a temporary fork because ASM side instructions are not native context.
pub(super) const TEMPORARY_FORK_BOUNDARY: &str = "Side conversation boundary: everything before this message is inherited reference history from the main thread, not your current task. Do not continue or complete its instructions, plans, tool calls or background work. Do not interact with, message, inspect or stop any existing or new agents or tasks. This side conversation is temporary and cannot retain worktrees; direct worktree requests back to the main conversation. Only messages after this boundary are active requests.";

/// Native agent, task and worktree controls removed from temporary forks, which must not steer
/// main's work or retain worktrees.
pub(super) const TEMPORARY_FORK_DISALLOWED_TOOLS: [&str; 10] = [
    "Agent",
    "Task",
    "TaskStop",
    "SendMessage",
    "ListAgents",
    "CronCreate",
    "CronDelete",
    "ScheduleWakeup",
    "EnterWorktree",
    "ExitWorktree",
];

/// Admits a temporary native fork of an idle Claude parent whose latest successful turn is
/// exactly the forked history, and pins that turn's final native message.
#[tracing::instrument(skip_all)]
pub(crate) async fn prepare_temporary_fork(
    parent: &Session,
    codex_home: &std::path::Path,
    model: Option<&str>,
    history: &[RolloutItem],
) -> Result<ForkSeed, CodexErr> {
    let refuse = |message: &str| CodexErr::InvalidRequest(message.to_string());
    if parent.active_turn.lock().await.is_some() || parent.external_runtime.is_busy() {
        return Err(refuse(
            "main is busy or has native background work; wait until it is idle",
        ));
    }
    let checkpoint: Option<Checkpoint> = std::fs::read(
        codex_home
            .join("claude-runtime")
            .join(format!("{}.json", parent.thread_id())),
    )
    .ok()
    .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let Some((checkpoint, fork_point)) = checkpoint
        .filter(|checkpoint| !checkpoint.in_flight)
        .and_then(|checkpoint| {
            let fork_point = checkpoint.fork_point.clone()?;
            Some((checkpoint, fork_point))
        })
    else {
        return Err(refuse(
            "main has no clean completed native turn to fork; finish a main turn first",
        ));
    };
    if model != Some(checkpoint.model.as_str()) {
        return Err(refuse(
            "Claude Code side conversations keep main's model; do not override it",
        ));
    }
    let messages = input_messages(history.iter().filter_map(|item| match item {
        RolloutItem::ResponseItem(envelope) => Some(&envelope.item),
        _ => None,
    }))
    .map_err(|error| refuse(&error.to_string()))?;
    if messages.len() != checkpoint.input_count
        || input_prefix(&messages).ok().as_deref() != Some(checkpoint.input_prefix.as_str())
    {
        return Err(refuse(
            "Claude Code side conversations fork main's latest delivered state only",
        ));
    }
    Ok(ForkSeed {
        parent: checkpoint,
        fork_point,
    })
}
