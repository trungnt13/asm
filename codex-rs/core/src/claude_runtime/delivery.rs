//! Confirms mailbox results at Cursor's native shell-output boundary, not at the HTTP bridge.

use anyhow::ensure;
use serde_json::Value;

pub(super) struct CursorMailReceipts {
    session_id: String,
    pending: Vec<String>,
}

impl CursorMailReceipts {
    pub(super) fn new(session_id: String) -> Self {
        Self {
            session_id,
            pending: Vec::new(),
        }
    }

    pub(super) fn stamp(&mut self, messages: &mut Vec<String>) {
        if !messages.is_empty() {
            let receipt = format!("ASM mail delivery receipt: {}", uuid::Uuid::new_v4());
            messages.push(receipt.clone());
            self.pending.push(receipt);
        }
    }

    pub(super) fn observe(&mut self, frame: &Value) {
        if self.pending.is_empty()
            || frame["method"] != "session/update"
            || frame["params"]["sessionId"] != self.session_id
        {
            return;
        }
        let update = &frame["params"]["update"];
        if update["sessionUpdate"] != "tool_call_update"
            || update["status"] != "completed"
            || update["rawOutput"]["exitCode"] != 0
        {
            return;
        }
        let Some(stdout) = update["rawOutput"]["stdout"].as_str() else {
            return;
        };
        let Ok(result) = serde_json::from_str::<Value>(stdout) else {
            return;
        };
        if result["isError"] != false {
            return;
        }
        let Some(content) = result["content"].as_array() else {
            return;
        };
        self.pending.retain(|receipt| {
            !content.iter().any(|block| {
                block["type"] == "text" && block["text"].as_str() == Some(receipt.as_str())
            })
        });
    }

    pub(super) fn ensure_delivered(&self) -> anyhow::Result<()> {
        ensure!(
            self.pending.is_empty(),
            "Cursor did not confirm mailbox output delivery; explicit recovery is required"
        );
        Ok(())
    }
}
