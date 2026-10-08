//! Interactive Claude Code workers, without subscription credential mediation.

mod events;
mod runtime;
mod transport;

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use codex_extension_api::ExternalAgentBackend;
use codex_extension_api::ExternalAgentLaunch;
use codex_extension_api::ExternalAgentRuntime;
use futures::future::BoxFuture;
use serde_json::json;
use tokio::process::Command;
use tokio::sync::Semaphore;
use uuid::Uuid;

use events::Observations;
use runtime::Admission;
use runtime::ClaudeRuntime;
use transport::Commands;
use transport::wait_for_exit;

const FORBIDDEN_PROVIDER_ENV: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_PROFILE",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
];

/// Opens an official interactive CLI inside an isolated, owned tmux server.
/// Human development-channel consent and Claude's own authentication are retained.
pub struct ClaudeCodeBackend {
    pub python: PathBuf,
    pub claude: PathBuf,
    pub tmux: PathBuf,
}

impl Default for ClaudeCodeBackend {
    fn default() -> Self {
        Self {
            python: "python3".into(),
            claude: "claude".into(),
            tmux: "tmux".into(),
        }
    }
}

impl ExternalAgentBackend for ClaudeCodeBackend {
    fn open(
        &self,
        launch: ExternalAgentLaunch,
    ) -> BoxFuture<'_, Result<Arc<dyn ExternalAgentRuntime>, String>> {
        Box::pin(self.open_worker(launch))
    }
}

impl ClaudeCodeBackend {
    #[tracing::instrument(skip_all)]
    async fn open_worker(
        &self,
        launch: ExternalAgentLaunch,
    ) -> Result<Arc<dyn ExternalAgentRuntime>, String> {
        if !cfg!(target_os = "linux") {
            return Err("Claude workers require Linux child-subreaper cancellation support".into());
        }
        if FORBIDDEN_PROVIDER_ENV
            .iter()
            .any(|key| std::env::var_os(key).is_some() || launch.env.contains_key(*key))
        {
            return Err("Claude subscription worker refuses ambient or selected provider/API credential overrides".into());
        }
        let selected_path = launch
            .env
            .get("PATH")
            .ok_or("Claude worker requires an explicit selected PATH")?;
        let resolve = |program: &PathBuf| -> Result<PathBuf, String> {
            let candidates = if program.is_absolute() {
                vec![program.clone()]
            } else if program.components().count() > 1 {
                vec![launch.cwd.as_path().join(program)]
            } else {
                std::env::split_paths(selected_path)
                    .map(|directory| {
                        let directory = if directory.is_absolute() {
                            directory
                        } else {
                            launch.cwd.as_path().join(directory)
                        };
                        directory.join(program)
                    })
                    .collect()
            };
            candidates
                .into_iter()
                .find(|candidate| {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        candidate.metadata().is_ok_and(|metadata| {
                            metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
                        })
                    }
                    #[cfg(not(unix))]
                    {
                        candidate.is_file()
                    }
                })
                .ok_or_else(|| {
                    format!(
                        "Executable {} unavailable in selected environment",
                        program.display()
                    )
                })
        };
        let python = resolve(&self.python)?;
        let claude = resolve(&self.claude)?;
        let tmux = resolve(&self.tmux)?;
        let assets = launch.state_dir.join(format!("bridge-{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&assets)
            .await
            .map_err(|error| error.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&assets, std::fs::Permissions::from_mode(/*mode*/ 0o700))
                .await
                .map_err(|error| error.to_string())?;
        }
        for (name, source) in [
            ("bridge.py", include_str!("../bridge.py")),
            ("activity.py", include_str!("../activity.py")),
            ("channel.py", include_str!("../channel.py")),
            ("instructions.py", include_str!("../instructions.py")),
            ("lifecycle.py", include_str!("../lifecycle.py")),
            ("observations.py", include_str!("../observations.py")),
        ] {
            tokio::fs::write(assets.join(name), source)
                .await
                .map_err(|error| error.to_string())?;
        }
        let child = Command::new(&python)
            .env_clear()
            .envs(&launch.env)
            .current_dir(&launch.cwd)
            .arg("-u")
            .arg(assets.join("bridge.py"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| error.to_string())?;
        let observations = Arc::new(Observations::default());
        let (commands, exit) = Commands::start(child, Arc::clone(&observations))?;
        let runtime = Arc::new(ClaudeRuntime {
            commands,
            exit,
            observations,
            submissions: Arc::new(Semaphore::new(/*permits*/ 1)),
            admission: Admission::default(),
        });
        if let Err(error) = runtime
            .commands
            .request(
                "launch",
                json!({"launch": launch, "claude": claude, "tmux": tmux}),
            )
            .await
        {
            runtime.commands.close();
            match wait_for_exit(runtime.exit.clone()).await {
                Ok(status) if status.success() => return Err(error),
                result => return Err(format!("{error}; bridge cleanup not confirmed: {result:?}")),
            }
        }
        Ok(runtime)
    }
}
