use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use super::provider::LoginProvider;
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

pub(super) struct LoginProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    provider: LoginProvider,
    stdout: ChildStdout,
    cwd: PathBuf,
    output_bytes: usize,
    deadline: Instant,
}

impl LoginProcess {
    #[tracing::instrument(skip_all)]
    pub(super) async fn start(cwd: &Path, provider: LoginProvider) -> Result<(Self, String), ()> {
        let mut child = command(cwd, provider)
            .args(provider.login_args())
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
                if let Some(url) = subscription_url(&output, provider) {
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
                provider,
                stdout,
                cwd: cwd.to_path_buf(),
                output_bytes: output.len(),
                deadline: Instant::now() + LOGIN_LIFETIME,
            },
            auth_url,
        ))
    }

    pub(super) fn expired(&self) -> bool {
        Instant::now() >= self.deadline
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn submit(mut self, code: &str) -> Result<(), ()> {
        if self.expired() || !self.provider.accepts_reply(code) {
            return Err(());
        }
        let deadline = self.deadline.min(Instant::now() + Duration::from_secs(30));
        timeout_at(deadline, async {
            if let Some(code) = self.provider.stdin_reply(code) {
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
                let mut child = command(&self.cwd, self.provider)
                    .args(self.provider.status_args())
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
                if self.provider.authenticated(&status) {
                    Ok(())
                } else {
                    Err(())
                }
            },
        )
        .await
        .map_err(|_| ())?
    }
}

fn subscription_url(output: &[u8], provider: LoginProvider) -> Option<String> {
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
            || !provider.valid_url(&url)
        {
            continue;
        }
        return Some(url.into());
    }
    None
}

fn command(cwd: &Path, provider: LoginProvider) -> Command {
    let mut command = provider.command(cwd);
    command.env_remove("ENABLE_BETA_TRACING_DETAILED");
    command.env_remove("ENABLE_ENHANCED_TELEMETRY_BETA");
    command.env_remove("DEBUG");
    command.env_remove("DEBUG_AUTH");
    command.env_remove("DEBUG_SDK");
    command.env_remove("NODE_DEBUG");
    command.env_remove("NODE_OPTIONS");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("OTEL_") {
            command.env_remove(name);
        }
    }
    command
        .env("BROWSER", "true")
        .env("DISABLE_TELEMETRY", "1")
        .env("DISABLE_ERROR_REPORTING", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(/*kill_on_drop*/ true);
    command
}
