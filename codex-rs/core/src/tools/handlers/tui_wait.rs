use std::future::Future;

use crate::function_tool::FunctionCallError;
use codex_tools::WaitReason;
use codex_tools::WaitReminder;
use serde_json::Value;
use serde_json::json;
use tokio_util::sync::CancellationToken;

pub(super) fn reason(arguments: &Value) -> Result<WaitReason, FunctionCallError> {
    let reason = arguments
        .get("reason")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            FunctionCallError::RespondToModel("wait_threads requires a reason string".to_string())
        })?;
    WaitReason::new(reason).map_err(FunctionCallError::RespondToModel)
}

pub(super) fn interrupted(reason: &WaitReason) -> Value {
    let mut result = json!({"timedOut": false, "wake": null, "polls": [], "interrupted": true});
    if let Some(message) = reason.reminder(WaitReminder::Interrupted) {
        result["message"] = json!(message);
    }
    result
}

#[tracing::instrument(skip_all)]
pub(super) async fn observe<T>(
    observation: impl Future<Output = T>,
    preempt: Option<&CancellationToken>,
    cancellation: &CancellationToken,
) -> Option<T> {
    tokio::pin!(observation);
    let Some(preempt) = preempt else {
        return Some(observation.await);
    };
    tokio::select! {
        biased;
        result = &mut observation => Some(result),
        _ = preempt.cancelled() => {
            cancellation.cancel();
            observation.await;
            None
        }
    }
}
