use crate::JsonSchema;

pub const WAIT_REASON_DESCRIPTION: &str = "Pending work to resume after waiting. At most 500 characters; use an empty string for no reminder.";

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

    pub fn reminder(&self, outcome: WaitReminder) -> Option<String> {
        if self.0.is_empty() {
            return None;
        }
        let guidance = match outcome {
            WaitReminder::Interrupted => {
                "Handle the new input, then resume the pending work unless canceled, paused, replaced, or blocked."
            }
            WaitReminder::Returned => {
                "Check the pending work after waiting; continue unless canceled, paused, replaced, or blocked."
            }
        };
        let reason = serde_json::Value::String(self.0.clone());
        Some(format!(
            "{guidance}\nReason for waiting (quoted context): {reason}"
        ))
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
