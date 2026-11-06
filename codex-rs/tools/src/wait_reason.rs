use crate::JsonSchema;

pub const WAIT_REASON_DESCRIPTION: &str = "What you are waiting for and what work you should resume after an interruption. At most 500 characters; use an empty string to omit the reason context. Continuation guidance is still returned.";

pub struct WaitReason(String);

pub enum WaitReminder {
    Interrupted,
    Returned,
}

impl WaitReason {
    pub fn new(reason: &str) -> Result<Self, String> {
        let reason = reason.trim();
        if reason.chars().count() > 500 {
            return Err(
                "reason must be at most 500 characters after trimming whitespace".to_string(),
            );
        }
        Ok(Self(reason.to_string()))
    }

    pub fn reminder(&self, wait_tool: &str, outcome: WaitReminder) -> String {
        let mut guidance = match outcome {
            WaitReminder::Interrupted => format!(
                "Your {wait_tool} call was interrupted by new input.\n\nResponse to user input and understand the intention to determine whether the new input cancels, pauses, replaces, or otherwise changes the pending task. A status question alone does not cancel or pause it."
            ),
            WaitReminder::Returned => format!("Your {wait_tool} call returned."),
        };
        guidance.push_str(&format!(
            "\n\nIf the task should continue, check whether the awaited work is complete or blocked. If it remains pending, give any status update as commentary, then call {wait_tool} again with valid current arguments. Do not end the turn merely because you answered a status question or a delegated agent is still working."
        ));
        if !self.0.is_empty() {
            let reason = serde_json::Value::String(self.0.clone());
            guidance.push_str(&format!(
                "\n\nReason for waiting (quoted context): {reason}"
            ));
        }
        guidance
    }
}

pub fn require_wait_reason(parameters: &mut JsonSchema) {
    parameters.properties.get_or_insert_default().insert(
        "reason".to_string(),
        JsonSchema::string(Some(WAIT_REASON_DESCRIPTION.to_string())),
    );
    let mut required = parameters.required.take().unwrap_or_default().into_vec();
    if !required.iter().any(|name| name == "reason") {
        required.push("reason".to_string());
    }
    parameters.required = Some(required.into_boxed_slice());
}
