use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::process::Child;
use tokio::process::ChildStdin;
use tokio::process::ChildStdout;
use tokio::process::Command;
use tokio::time::Instant;
use tokio::time::timeout;
use tokio::time::timeout_at;
use url::Url;

const OUTPUT_LIMIT: usize = 64 * 1024;
const LOGIN_LIFETIME: Duration = Duration::from_secs(300);

pub(crate) enum LoginProvider {
    Claude,
    Cursor,
}

pub(crate) struct LoginProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    cursor: bool,
    stdout: ChildStdout,
    cwd: PathBuf,
    output_bytes: usize,
    deadline: Instant,
}

impl LoginProcess {
    #[tracing::instrument(skip_all)]
    pub(crate) async fn start(cwd: &Path, provider: LoginProvider) -> Result<(Self, String), ()> {
        let cursor = matches!(provider, LoginProvider::Cursor);
        let mut child = command(cwd, cursor)
            .args(if cursor {
                vec!["login"]
            } else {
                vec!["auth", "login", "--claudeai"]
            })
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|_| ())?;
        let stdin = child.stdin.take();
        let mut stdout = child.stdout.take().ok_or(())?;
        let mut output = Vec::new();
        let auth_url = timeout(Duration::from_secs(20), async {
            let mut chunk = [0_u8; 1024];
            loop {
                let count = stdout.read(&mut chunk).await.map_err(|_| ())?;
                if count == 0 || output.len() + count > OUTPUT_LIMIT {
                    return Err(());
                }
                output.extend_from_slice(&chunk[..count]);
                if let Some(url) = subscription_url(&output, cursor) {
                    return Ok(url);
                }
            }
        })
        .await
        .map_err(|_| ())??;
        Ok((
            Self {
                child,
                stdin,
                cursor,
                stdout,
                cwd: cwd.to_path_buf(),
                output_bytes: output.len(),
                deadline: Instant::now() + LOGIN_LIFETIME,
            },
            auth_url,
        ))
    }

    pub(crate) fn expired(&self) -> bool {
        Instant::now() >= self.deadline
    }

    #[tracing::instrument(skip_all)]
    pub(crate) async fn submit(mut self, code: &str) -> Result<(), ()> {
        if self.expired()
            || (!self.cursor
                && (code.len() > 4096
                    || !code.split_once('#').is_some_and(|(code, state)| {
                        !code.is_empty()
                            && !state.is_empty()
                            && code.bytes().chain(state.bytes()).all(|byte| {
                                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
                            })
                    })))
        {
            return Err(());
        }
        let deadline = self.deadline.min(Instant::now() + Duration::from_secs(30));
        timeout_at(deadline, async {
            if !self.cursor {
                let stdin = self.stdin.as_mut().ok_or(())?;
                stdin.write_all(code.as_bytes()).await.map_err(|_| ())?;
                stdin.write_all(b"\n").await.map_err(|_| ())?;
                stdin.flush().await.map_err(|_| ())?;
            }
            let mut chunk = [0_u8; 1024];
            loop {
                let count = self.stdout.read(&mut chunk).await.map_err(|_| ())?;
                self.output_bytes += count;
                if self.output_bytes > OUTPUT_LIMIT {
                    return Err(());
                }
                if count == 0 {
                    break;
                }
            }
            if !self.child.wait().await.map_err(|_| ())?.success() {
                return Err(());
            }
            Ok(())
        })
        .await
        .map_err(|_| ())??;
        timeout_at(
            deadline.min(Instant::now() + Duration::from_secs(10)),
            async {
                let mut child = command(&self.cwd, self.cursor)
                    .args(if self.cursor {
                        vec!["status", "--format", "json"]
                    } else {
                        vec!["auth", "status"]
                    })
                    .spawn()
                    .map_err(|_| ())?;
                let stdout = child.stdout.take().ok_or(())?;
                let mut output = Vec::new();
                stdout
                    .take((OUTPUT_LIMIT + 1) as u64)
                    .read_to_end(&mut output)
                    .await
                    .map_err(|_| ())?;
                if output.len() > OUTPUT_LIMIT || !child.wait().await.map_err(|_| ())?.success() {
                    return Err(());
                }
                let status: serde_json::Value = serde_json::from_slice(&output).map_err(|_| ())?;
                if self.cursor {
                    return if status["isAuthenticated"] == true
                        && status["hasAccessToken"] == true
                        && status["hasRefreshToken"] == true
                    {
                        Ok(())
                    } else {
                        Err(())
                    };
                }
                if status["loggedIn"] != true
                    || status["authMethod"] != "claude.ai"
                    || status["apiProvider"] != "firstParty"
                {
                    return Err(());
                }
                Ok(())
            },
        )
        .await
        .map_err(|_| ())?
    }
}

fn subscription_url(output: &[u8], cursor: bool) -> Option<String> {
    let output = std::str::from_utf8(output).ok()?;
    for (start, _) in output.match_indices("https://") {
        let candidate = &output[start..];
        let Some(end) =
            candidate.find(|ch: char| ch.is_ascii_whitespace() || ch.is_ascii_control())
        else {
            continue;
        };
        let Ok(url) = Url::parse(&candidate[..end]) else {
            continue;
        };
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.fragment().is_some()
            || (!cursor
                && !matches!(
                    (url.host_str(), url.path()),
                    (Some("claude.com"), "/cai/oauth/authorize")
                        | (Some("claude.ai"), "/oauth/authorize")
                ))
            || (cursor
                && (url.host_str() != Some("cursor.com") || url.path() != "/loginDeepControl"))
        {
            continue;
        }
        let params = url
            .query_pairs()
            .collect::<std::collections::HashMap<_, _>>();
        if cursor {
            if !params.keys().all(|key| {
                matches!(
                    key.as_ref(),
                    "challenge" | "uuid" | "mode" | "redirectTarget" | "supportsSelectedTeamLogin"
                )
            }) {
                continue;
            }
            if params.get("mode").map(|v| v.as_ref()) == Some("login")
                && params.get("redirectTarget").map(|v| v.as_ref()) == Some("cli")
                && ["challenge", "uuid"]
                    .iter()
                    .all(|key| params.get(*key).is_some_and(|v| !v.is_empty()))
            {
                return Some(url.into());
            }
            continue;
        }
        if params.get("response_type").map(|value| value.as_ref()) == Some("code")
            && params.get("redirect_uri").map(|value| value.as_ref())
                == Some("https://platform.claude.com/oauth/code/callback")
            && ["state", "client_id", "code_challenge"]
                .iter()
                .all(|key| params.get(*key).is_some_and(|value| !value.is_empty()))
        {
            return Some(url.into());
        }
    }
    None
}

fn command(cwd: &Path, cursor: bool) -> Command {
    let mut command = Command::new(if cursor { "agent" } else { "claude" });
    for name in [
        "CURSOR_API_KEY",
        "CURSOR_AUTH_TOKEN",
        "CURSOR_API_ENDPOINT",
        "CURSOR_WEBSITE_URL",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_PROFILE",
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
        "CLAUDE_CODE_USE_ANTHROPIC_AWS",
        "CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDE_CODE_OAUTH_REFRESH_TOKEN",
        "CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR",
        "CLAUDE_CODE_CUSTOM_OAUTH_URL",
        "CLAUDE_CODE_OAUTH_CLIENT_ID",
        "CLAUDE_CODE_OAUTH_SCOPES",
        "CLAUDE_CODE_OAUTH_SCOPES_APPEND",
        "CLAUDE_LOCAL_OAUTH_API_BASE",
        "CLAUDE_LOCAL_OAUTH_APPS_BASE",
        "CLAUDE_LOCAL_OAUTH_CONSOLE_BASE",
        "CLAUDE_CODE_ENABLE_TELEMETRY",
        "CLAUDE_CODE_DEBUG_LOG_LEVEL",
        "CLAUDE_CODE_DEBUG_LOGS_DIR",
        "CLAUDE_CODE_SESSION_LOG",
        "CLAUDE_CODE_TEE_SDK_STDOUT",
        "CLAUDE_CODE_TERMINAL_RECORDING",
        "CLAUDE_DEBUG",
        "CLAUDE_PTY_RECORD",
        "ENABLE_BETA_TRACING_DETAILED",
        "ENABLE_ENHANCED_TELEMETRY_BETA",
        "ANTHROPIC_LOG",
        "DEBUG",
        "DEBUG_AUTH",
        "DEBUG_SDK",
        "DEBUG_CLAUDE_AGENT_SDK",
        "NODE_DEBUG",
        "NODE_OPTIONS",
    ] {
        command.env_remove(name);
    }
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("OTEL_") {
            command.env_remove(name);
        }
    }
    command
        .env("BROWSER", "true")
        .env("NO_OPEN_BROWSER", "1")
        .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
        .env("DISABLE_TELEMETRY", "1")
        .env("DISABLE_ERROR_REPORTING", "1")
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(/*kill_on_drop*/ true);
    command
}
