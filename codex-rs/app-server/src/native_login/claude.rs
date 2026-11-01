use std::path::Path;
use tokio::process::Command;
use url::Url;

pub(super) fn command(cwd: &Path) -> Command {
    let mut command = Command::new("claude");
    command.env_remove("DEBUG_CLAUDE_AGENT_SDK");
    command.env_remove("ANTHROPIC_API_KEY");
    command.env_remove("ANTHROPIC_AUTH_TOKEN");
    command.env_remove("ANTHROPIC_BASE_URL");
    command.env_remove("ANTHROPIC_PROFILE");
    command.env_remove("CLAUDE_CODE_USE_BEDROCK");
    command.env_remove("CLAUDE_CODE_USE_VERTEX");
    command.env_remove("CLAUDE_CODE_USE_FOUNDRY");
    command.env_remove("CLAUDE_CODE_USE_ANTHROPIC_AWS");
    command.env_remove("CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD");
    command.env_remove("CLAUDE_CODE_OAUTH_TOKEN");
    command.env_remove("CLAUDE_CODE_OAUTH_REFRESH_TOKEN");
    command.env_remove("CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR");
    command.env_remove("CLAUDE_CODE_CUSTOM_OAUTH_URL");
    command.env_remove("CLAUDE_CODE_OAUTH_CLIENT_ID");
    command.env_remove("CLAUDE_CODE_OAUTH_SCOPES");
    command.env_remove("CLAUDE_CODE_OAUTH_SCOPES_APPEND");
    command.env_remove("CLAUDE_LOCAL_OAUTH_API_BASE");
    command.env_remove("CLAUDE_LOCAL_OAUTH_APPS_BASE");
    command.env_remove("CLAUDE_LOCAL_OAUTH_CONSOLE_BASE");
    command.env_remove("CLAUDE_CODE_ENABLE_TELEMETRY");
    command.env_remove("CLAUDE_CODE_DEBUG_LOG_LEVEL");
    command.env_remove("CLAUDE_CODE_DEBUG_LOGS_DIR");
    command.env_remove("CLAUDE_CODE_SESSION_LOG");
    command.env_remove("CLAUDE_CODE_TEE_SDK_STDOUT");
    command.env_remove("CLAUDE_CODE_TERMINAL_RECORDING");
    command.env_remove("CLAUDE_DEBUG");
    command.env_remove("CLAUDE_PTY_RECORD");
    command.env_remove("ANTHROPIC_LOG");
    command
        .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
        .current_dir(cwd);
    command
}
pub(super) const LOGIN_ARGS: &[&str] = &["auth", "login", "--claudeai"];
pub(super) const STATUS_ARGS: &[&str] = &["auth", "status"];
pub(super) fn environment_error() -> Option<&'static str> {
    ["CLAUDE_CODE_OAUTH_TOKEN", "CLAUDE_CODE_OAUTH_REFRESH_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR"].into_iter().any(|name| std::env::var_os(name).is_some()).then_some("Native login is unavailable while native OAuth environment overrides are set. Remove those overrides from the backend environment and retry.")
}
pub(super) fn valid_url(url: &Url) -> bool {
    let params = url
        .query_pairs()
        .collect::<std::collections::HashMap<_, _>>();
    matches!(
        (url.host_str(), url.path()),
        (Some("claude.com"), "/cai/oauth/authorize") | (Some("claude.ai"), "/oauth/authorize")
    ) && params.get("response_type").map(|value| value.as_ref()) == Some("code")
        && params.get("redirect_uri").map(|value| value.as_ref())
            == Some("https://platform.claude.com/oauth/code/callback")
        && ["state", "client_id", "code_challenge"]
            .iter()
            .all(|key| params.get(*key).is_some_and(|value| !value.is_empty()))
}
pub(super) fn authenticated(status: &serde_json::Value) -> bool {
    status["loggedIn"] == true
        && status["authMethod"] == "claude.ai"
        && status["apiProvider"] == "firstParty"
}
pub(super) fn instructions(url: &str) -> String {
    format!(
        "Sign in to your Claude subscription: [Open Claude sign-in]({url}).\nCopy the one-time authorization code from the browser and send it as your next message. Do not send an API key or access token. Send `cancel` to cancel. The failed prompt will not be replayed."
    )
}
pub(super) fn is_authorization_code(text: &str) -> bool {
    text.len() <= 4096
        && text.split_once('#').is_some_and(|(code, state)| {
            [code, state].into_iter().all(|part| {
                part.len() >= 16
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            })
        })
}
