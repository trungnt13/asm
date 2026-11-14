//! Run persistent official Claude and Cursor CLI sessions.
//! The native CLI owns tools and configuration. Callers serialize turns.

mod claude;
mod claude_owner;
mod claude_steering;
mod cleanup;
mod cursor;
mod frames;
mod process;

pub use claude::ClaudeOptions;
pub use claude::ClaudeSession;
pub use claude::ClaudeSessionKind;
pub use claude::validate_prompt as validate_claude_prompt;
pub use claude_owner::ClaudeContinuation;
pub use claude_owner::ClaudeRuntime;
pub use claude_steering::ClaudeSteering;
pub use cursor::CursorOptions;
pub use cursor::CursorRuntime;
pub use cursor::CursorSession;

use serde_json::Value;

#[derive(Clone)]
pub enum Prompt {
    Text(String),
    Blocks(Vec<InputBlock>),
}

#[derive(Clone)]
pub enum InputBlock {
    Text { text: String },
    Image { mime_type: String, data: String },
}

/// A failed turn that the native CLI closed at a confirmed idle boundary, so the session
/// stays resumable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalTurnFailure {
    Authentication,
    UsageLimit,
}

impl std::fmt::Display for ExternalTurnFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Authentication => formatter.write_str("Native authentication is required"),
            Self::UsageLimit => formatter.write_str("Native usage limit reached"),
        }
    }
}

impl std::error::Error for ExternalTurnFailure {}

pub struct ExternalTurnOutcome {
    pub result: Option<Value>,
    pub interrupted: bool,
    pub submitted_steering_count: usize,
    pub failure: Option<ExternalTurnFailure>,
}
