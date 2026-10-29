use crate::cleanup;
use crate::frames;
use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use codex_utils_pty::process_group;
use serde_json::Value;
use serde_json::json;
use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Child;
use tokio::process::ChildStdin;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use uuid::Uuid;

pub(super) const STOP_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 5);

/// Native CLI process leading its own process group, with framed stdout and a bounded stderr tail.
pub(super) struct ExternalProcess {
    pub(super) child: Child,
    pub(super) marker: String,
    pub(super) birth: Option<cleanup::ProcessBirth>,
    cleanup_complete: bool,
    stdin: Option<ChildStdin>,
    pub(super) frames: mpsc::Receiver<Result<Value>>,
    stdout_reader: JoinHandle<()>,
    stderr_reader: Option<JoinHandle<()>>,
    stderr: Arc<Mutex<VecDeque<u8>>>,
    exit_status: Option<String>,
    pre_cleanup_process: Option<Value>,
    frame_policy: frames::FramePolicy,
}

// A dropped turn future must not leave a live agent executing outside ASM's task lifetime.
pub(super) struct TurnGuard {
    pub(super) pid: Option<u32>,
    pub(super) armed: bool,
    pub(super) marker: String,
    pub(super) birth: Option<cleanup::ProcessBirth>,
}

impl Drop for TurnGuard {
    fn drop(&mut self) {
        if self.armed {
            cleanup::kill_owned(self.pid, &self.marker, self.birth).unwrap_or_else(|error| {
                tracing::warn!(%error, "native descendant cleanup is incomplete");
            });
        }
    }
}

impl ExternalProcess {
    /// Starts `command` as the leader of a new process group that dies with ASM.
    pub(super) fn spawn(mut command: Command, frame_policy: frames::FramePolicy) -> Result<Self> {
        let marker = Uuid::new_v4().to_string();
        command.env(cleanup::OWNER_ENV, &marker);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(/*kill_on_drop*/ true);
        #[cfg(unix)]
        {
            let parent_pid = std::process::id() as i32;
            // SAFETY: only async-signal-safe system calls run in the child.
            unsafe {
                command.pre_exec(move || {
                    process_group::set_process_group()?;
                    process_group::set_parent_death_signal(parent_pid)
                });
            }
        }
        let mut child = command.spawn()?;
        let birth = child.id().and_then(|pid| {
            cleanup::process_birth(pid).inspect_err(|error| {
                tracing::warn!(%error, "native process identity is unavailable; cleanup remains conservative");
            }).ok()
        });
        let stdin = child.stdin.take().context("native CLI stdin unavailable")?;
        let stdout = child
            .stdout
            .take()
            .context("native CLI stdout unavailable")?;
        let stderr_pipe = child
            .stderr
            .take()
            .context("native CLI stderr unavailable")?;
        let (sender, frames) = mpsc::channel(/*buffer*/ 32);
        let stderr = Arc::new(Mutex::new(VecDeque::new()));
        let stdout_reader = tokio::spawn(frames::read_frames(stdout, sender, frame_policy));
        let stderr_reader = Some(tokio::spawn(frames::drain_stderr(
            stderr_pipe,
            Arc::clone(&stderr),
        )));
        Ok(Self {
            child,
            marker,
            birth,
            cleanup_complete: false,
            stdin: Some(stdin),
            frames,
            stdout_reader,
            stderr_reader,
            stderr,
            exit_status: None,
            pre_cleanup_process: None,
            frame_policy,
        })
    }

    pub(super) fn diagnostic_snapshot(&self, failure_result: &Option<Value>) -> Value {
        let stderr = self
            .stderr
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .copied()
            .collect::<Vec<_>>();
        json!({
            "stderr_tail": String::from_utf8_lossy(&stderr),
            "failure_result": failure_result,
            "exit_status": self.exit_status,
            "pre_cleanup_process": self.pre_cleanup_process,
        })
    }

    pub(super) async fn write_frame(&mut self, frame: Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(&frame)?;
        if bytes.len() + 1 > self.frame_policy.input_limit(&frame) {
            bail!("native CLI input frame exceeded size limit");
        }
        bytes.push(b'\n');
        let stdin = self.stdin.as_mut().context("native CLI stdin is closed")?;
        stdin
            .write_all(&bytes)
            .await
            .context("writing native CLI input")?;
        stdin.flush().await.context("flushing native CLI input")
    }

    pub(super) async fn kill(&mut self) {
        if let Err(error) = self.terminate().await {
            tracing::warn!(%error, "native descendant cleanup is incomplete");
        }
    }

    pub(super) async fn terminate(&mut self) -> Result<()> {
        let pid = self.child.id();
        if self.pre_cleanup_process.is_none() {
            self.pre_cleanup_process = Some(match self.child.try_wait() {
                Ok(Some(status)) => json!({"state": "exited", "exit_status": status.to_string()}),
                Ok(None) => json!({"state": "running"}),
                Err(error) => json!({"state": "unknown", "error": error.to_string()}),
            });
        }
        let marker = self.marker.clone();
        let birth = self.birth;
        let cleanup = tokio::task::spawn_blocking(move || cleanup::kill_owned(pid, &marker, birth))
            .await
            .context("native cleanup task failed")?;
        self.cleanup_complete = cleanup.is_ok();
        if let Ok(Ok(status)) = timeout(STOP_TIMEOUT, self.child.wait()).await {
            self.exit_status = Some(status.to_string());
        }
        cleanup
    }

    pub(super) async fn shutdown(&mut self) -> Result<()> {
        self.stdin.take();
        if let Ok(Ok(status)) = timeout(STOP_TIMEOUT, self.child.wait()).await {
            self.exit_status = Some(status.to_string());
        }
        let cleanup = self.terminate().await;
        self.wait_for_stderr().await;
        cleanup
    }

    /// Lets stderr finish before callers capture diagnostics. Keeps the task owned
    /// while awaiting, so cancellation still lets Drop stop it.
    pub(super) async fn wait_for_stderr(&mut self) {
        if let Some(reader) = self.stderr_reader.as_mut() {
            if timeout(STOP_TIMEOUT, &mut *reader).await.is_err() {
                reader.abort();
            }
            self.stderr_reader.take();
        }
    }
}

impl Drop for ExternalProcess {
    fn drop(&mut self) {
        if !self.cleanup_complete
            && let Err(error) = cleanup::kill_owned(self.child.id(), &self.marker, self.birth)
        {
            tracing::warn!(%error, "native descendant cleanup is incomplete");
        }
        self.stdout_reader.abort();
        if let Some(reader) = &self.stderr_reader {
            reader.abort();
        }
    }
}
