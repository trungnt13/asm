//! Run persistent official Claude and Cursor CLI sessions.
//! The native CLI owns tools and configuration. Callers serialize turns.

mod claude;
mod cleanup;
mod cursor;
mod frames;
mod process;

pub use claude::ClaudeOptions;
pub use claude::ClaudeRuntime;
pub use claude::ClaudeSession;
pub use claude::ClaudeSessionKind;
pub use claude::validate_prompt as validate_claude_prompt;
pub use cursor::CursorOptions;
pub use cursor::CursorRuntime;
pub use cursor::CursorSession;

use serde_json::Value;

pub enum Prompt {
    Text(String),
    Blocks(Vec<InputBlock>),
}

pub enum InputBlock {
    Text { text: String },
    Image { mime_type: String, data: String },
}

#[derive(Debug)]
pub enum ExternalTurnFailure {
    Authentication,
}

impl std::fmt::Display for ExternalTurnFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Authentication => formatter.write_str("Native authentication is required"),
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
