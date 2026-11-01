use super::claude;
use super::cursor;
use std::path::Path;
use tokio::process::Command;
use url::Url;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum LoginProvider {
    Claude,
    Cursor,
}
impl LoginProvider {
    pub(super) fn from_model(model: &str) -> Option<Self> {
        if model.starts_with("claude-code/") {
            Some(Self::Claude)
        } else if model.starts_with("cursor-agent/") {
            Some(Self::Cursor)
        } else {
            None
        }
    }
    pub(super) fn from_command(text: &str) -> Option<Self> {
        if text.eq_ignore_ascii_case("claude login") {
            Some(Self::Claude)
        } else if text.eq_ignore_ascii_case("cursor login") {
            Some(Self::Cursor)
        } else {
            None
        }
    }
    pub(super) fn command(self, cwd: &Path) -> Command {
        match self {
            Self::Claude => claude::command(cwd),
            Self::Cursor => cursor::command(cwd),
        }
    }
    pub(super) fn login_args(self) -> &'static [&'static str] {
        match self {
            Self::Claude => claude::LOGIN_ARGS,
            Self::Cursor => cursor::LOGIN_ARGS,
        }
    }
    pub(super) fn status_args(self) -> &'static [&'static str] {
        match self {
            Self::Claude => claude::STATUS_ARGS,
            Self::Cursor => cursor::STATUS_ARGS,
        }
    }
    pub(super) fn environment_error(self) -> Option<&'static str> {
        match self {
            Self::Claude => claude::environment_error(),
            Self::Cursor => cursor::environment_error(),
        }
    }
    pub(super) fn valid_url(self, url: &Url) -> bool {
        match self {
            Self::Claude => claude::valid_url(url),
            Self::Cursor => cursor::valid_url(url),
        }
    }
    pub(super) fn authenticated(self, status: &serde_json::Value) -> bool {
        match self {
            Self::Claude => claude::authenticated(status),
            Self::Cursor => cursor::authenticated(status),
        }
    }
    pub(super) fn instructions(self, url: &str) -> String {
        match self {
            Self::Claude => claude::instructions(url),
            Self::Cursor => cursor::instructions(url),
        }
    }
    pub(super) fn accepts_reply(self, text: &str) -> bool {
        match self {
            Self::Claude => claude::is_authorization_code(text),
            Self::Cursor => text.eq_ignore_ascii_case("done"),
        }
    }
    pub(super) fn stdin_reply(self, text: &str) -> Option<&str> {
        match self {
            Self::Claude => Some(text),
            Self::Cursor => None,
        }
    }
}
