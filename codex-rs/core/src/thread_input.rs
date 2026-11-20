//! Shared, non-consuming input-activity waits for native tools and hosts.

use crate::session::InputQueueActivity;
use crate::session::session::Session;
use std::time::Duration;
use tokio::sync::watch;

/// Why a native or host input-activity wait finished.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputWaitOutcome {
    /// Agent mail was observed, including a typed agent message from a notifying injection.
    MailboxActivity,
    /// User steering or another kind of notifying input was observed.
    Steered,
    /// The deadline elapsed or the input-activity source closed.
    TimedOut,
}

pub(crate) async fn wait_for_input_activity(
    session: &Session,
    turn_id: &str,
    timeout: Duration,
) -> InputWaitOutcome {
    tokio::time::timeout(timeout, async {
        let (activity, pending) = subscribe_input_activity(session, turn_id).await;
        wait_for_activity(activity, pending).await
    })
    .await
    .unwrap_or(InputWaitOutcome::TimedOut)
}

pub(crate) async fn subscribe_input_activity(
    session: &Session,
    turn_id: &str,
) -> (
    watch::Receiver<InputQueueActivity>,
    Option<InputQueueActivity>,
) {
    let turn_state = session
        .input_queue
        .turn_state_for_sub_id(&session.active_turn, turn_id)
        .await;
    let (activity, pending) = session
        .input_queue
        .subscribe_activity(turn_state.as_deref())
        .await;
    let pending = pending.or_else(|| {
        (session
            .external_runtime
            .queued_input_count
            .load(std::sync::atomic::Ordering::SeqCst)
            > 0)
        .then_some(crate::session::InputQueueActivity::Steer)
    });
    (activity, pending)
}

pub(crate) async fn wait_for_activity(
    mut activity: watch::Receiver<InputQueueActivity>,
    pending: Option<InputQueueActivity>,
) -> InputWaitOutcome {
    let activity = match pending {
        Some(activity) => activity,
        None => match activity.changed().await {
            Ok(()) => *activity.borrow_and_update(),
            Err(_) => return InputWaitOutcome::TimedOut,
        },
    };
    match activity {
        InputQueueActivity::Mailbox => InputWaitOutcome::MailboxActivity,
        InputQueueActivity::Steer => InputWaitOutcome::Steered,
    }
}
