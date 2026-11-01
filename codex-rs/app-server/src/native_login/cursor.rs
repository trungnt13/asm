use std::path::Path;
use tokio::process::Command;
use url::Url;

pub(super) fn command(cwd: &Path) -> Command {
    let mut command = Command::new("agent");
    command.env_remove("CURSOR_API_KEY");
    command.env_remove("CURSOR_AUTH_TOKEN");
    command.env_remove("CURSOR_API_ENDPOINT");
    command.env_remove("CURSOR_WEBSITE_URL");
    command.env("NO_OPEN_BROWSER", "1").current_dir(cwd);
    command
}
pub(super) const LOGIN_ARGS: &[&str] = &["login"];
pub(super) const STATUS_ARGS: &[&str] = &["status", "--format", "json"];
pub(super) fn environment_error() -> Option<&'static str> {
    (["CURSOR_AUTH_TOKEN", "CURSOR_API_ENDPOINT", "CURSOR_WEBSITE_URL"].into_iter().any(|name| std::env::var_os(name).is_some()) || std::env::var("AGENT_CLI_CREDENTIAL_STORE").is_ok_and(|value| value == "memory"))
        .then_some("Cursor login is unavailable with authentication or endpoint overrides, or a memory-only credential store. Remove those overrides and retry.")
}
pub(super) fn valid_url(url: &Url) -> bool {
    let params = url
        .query_pairs()
        .collect::<std::collections::HashMap<_, _>>();
    url.host_str() == Some("cursor.com")
        && url.path() == "/loginDeepControl"
        && params.keys().all(|key| {
            matches!(
                key.as_ref(),
                "challenge" | "uuid" | "mode" | "redirectTarget" | "supportsSelectedTeamLogin"
            )
        })
        && params.get("mode").map(|value| value.as_ref()) == Some("login")
        && params.get("redirectTarget").map(|value| value.as_ref()) == Some("cli")
        && ["challenge", "uuid"]
            .iter()
            .all(|key| params.get(*key).is_some_and(|value| !value.is_empty()))
}
pub(super) fn authenticated(status: &serde_json::Value) -> bool {
    status["isAuthenticated"] == true
        && status["hasAccessToken"] == true
        && status["hasRefreshToken"] == true
}
pub(super) fn instructions(url: &str) -> String {
    format!(
        "Sign in to your Cursor account: [Open Cursor sign-in]({url}). Complete authentication in your browser, then send `done`. Do not send any key, code, or token. Send `cancel` to cancel. The failed prompt will not be replayed."
    )
}
