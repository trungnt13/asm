use std::collections::HashMap;
use std::process::ExitStatus;
use std::sync::Arc;
use std::time::Duration;

use codex_extension_api::ExternalAgentEvent;
use codex_extension_api::ExternalAgentLaunchError;
use codex_extension_api::ExternalObservation;
use serde_json::Value;
use serde_json::json;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::process::Child;
use tokio::process::ChildStdin;
use tokio::process::ChildStdout;
use tokio::sync::Mutex;
use tokio::sync::Notify;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::watch;
use uuid::Uuid;

use crate::events::Observations;

type Replies = Arc<Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>>;
pub(super) type Exit = watch::Receiver<Option<Result<ExitStatus, String>>>;

struct Write {
    bytes: Vec<u8>,
    receipt: oneshot::Sender<Result<(), String>>,
}

pub(super) struct Commands {
    writes: mpsc::Sender<Write>,
    close: Arc<Notify>,
    replies: Replies,
    stderr: Arc<Mutex<Vec<u8>>>,
}

impl Drop for Commands {
    fn drop(&mut self) {
        self.close.notify_one();
    }
}

impl Commands {
    pub(super) fn start(
        mut child: Child,
        observations: Arc<Observations>,
    ) -> Result<(Arc<Self>, Exit), String> {
        let stdin = child.stdin.take().ok_or("Missing bridge stdin")?;
        let stdout = child.stdout.take().ok_or("Missing bridge stdout")?;
        let mut stderr = child.stderr.take().ok_or("Missing bridge stderr")?;
        let diagnostics = Arc::new(Mutex::new(Vec::new()));
        let stderr_tail = Arc::clone(&diagnostics);
        tokio::spawn(async move {
            let mut buffer = [0; 4096];
            loop {
                let count = match stderr.read(&mut buffer).await {
                    Ok(0) | Err(_) => break,
                    Ok(count) => count,
                };
                let mut tail = stderr_tail.lock().await;
                tail.extend_from_slice(&buffer[..count]);
                let excess = tail.len().saturating_sub(/*rhs*/ 8192);
                tail.drain(..excess);
            }
        });
        let (writes, receiver) = mpsc::channel(/*buffer*/ 16);
        let close = Arc::new(Notify::new());
        let commands = Arc::new(Self {
            writes,
            close: Arc::clone(&close),
            replies: Arc::new(Mutex::new(HashMap::new())),
            stderr: diagnostics,
        });
        tokio::spawn(write_commands(stdin, receiver, close));
        tokio::spawn(read_observations(
            stdout,
            Arc::downgrade(&commands),
            Arc::clone(&commands.replies),
            observations,
        ));
        let (exit_sender, exit) = watch::channel(/*init*/ None);
        tokio::spawn(async move {
            exit_sender.send_replace(Some(child.wait().await.map_err(|error| error.to_string())));
        });
        Ok((commands, exit))
    }

    pub(super) fn close(&self) {
        self.close.notify_one();
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn diagnostics(&self) -> String {
        let tail = self.stderr.lock().await;
        let text: String = String::from_utf8_lossy(&tail)
            .chars()
            .filter(|character| !character.is_control() || *character == '\n')
            .collect();
        if text.is_empty() {
            String::new()
        } else {
            format!("; bridge stderr: {text}")
        }
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn launch(&self, payload: Value) -> Result<(), ExternalAgentLaunchError> {
        let reply = self
            .request_reply("launch", payload)
            .await
            .map_err(ExternalAgentLaunchError::Unsettled)?;
        if reply.get("ok").and_then(Value::as_bool) == Some(true) {
            return Ok(());
        }
        let error = reply
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("Bridge launch failed")
            .to_owned();
        Err(
            if reply.get("settled").and_then(Value::as_bool) == Some(true) {
                ExternalAgentLaunchError::Rejected(error)
            } else {
                ExternalAgentLaunchError::Unsettled(error)
            },
        )
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn request(&self, op: &str, payload: Value) -> Result<(), String> {
        let result = self.request_reply(op, payload).await.and_then(|reply| {
            if reply.get("ok").and_then(Value::as_bool) == Some(true) {
                Ok(())
            } else {
                Err(reply
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("Bridge command failed")
                    .to_owned())
            }
        });
        match result {
            Ok(()) => Ok(()),
            Err(error) => Err(format!("{error}{}", self.diagnostics().await)),
        }
    }

    #[tracing::instrument(skip_all)]
    async fn request_reply(&self, op: &str, payload: Value) -> Result<Value, String> {
        let id = Uuid::new_v4().to_string();
        let mut bytes = serde_json::to_vec(&json!({"id": id, "op": op, "payload": payload}))
            .map_err(|error| error.to_string())?;
        if bytes.len() > 256 * 1024 {
            return Err("Claude bridge command exceeds 256 KiB".into());
        }
        bytes.push(b'\n');
        let (reply, response) = oneshot::channel();
        self.replies.lock().await.insert(id.clone(), reply);
        let (receipt, written) = oneshot::channel();
        let result = tokio::time::timeout(Duration::from_secs(/*secs*/ 35), async {
            self.writes
                .send(Write { bytes, receipt })
                .await
                .map_err(|_| "Claude bridge writer closed".to_owned())?;
            written
                .await
                .map_err(|_| "Claude bridge write interrupted; outcome unknown".to_owned())??;
            response
                .await
                .map_err(|_| "Claude bridge disconnected".to_owned())?
        })
        .await;
        self.replies.lock().await.remove(&id);
        result.map_err(|_| "Claude bridge command timed out; outcome unknown".to_owned())?
    }
}

#[tracing::instrument(skip_all)]
async fn write_commands(
    mut stdin: ChildStdin,
    mut writes: mpsc::Receiver<Write>,
    close: Arc<Notify>,
) {
    loop {
        let command = tokio::select! {
            biased;
            _ = close.notified() => break,
            command = writes.recv() => command,
        };
        let Some(command) = command else { break };
        let result = tokio::select! {
            biased;
            _ = close.notified() => break,
            result = tokio::time::timeout(Duration::from_secs(/*secs*/ 8), stdin.write_all(&command.bytes)) => result,
        };
        let result = result
            .map_err(|_| "Claude bridge write timed out; outcome unknown".to_owned())
            .and_then(|result| result.map_err(|error| error.to_string()));
        let failed = result.is_err();
        let _ = command.receipt.send(result);
        if failed {
            break;
        }
    }
    // Only this task owns stdin. Closing it requests Python's guarded cleanup.
}

#[tracing::instrument(skip_all)]
async fn read_observations(
    stdout: ChildStdout,
    commands: std::sync::Weak<Commands>,
    replies: Replies,
    observations: Arc<Observations>,
) {
    let mut closed = false;
    let mut failure = None;
    let mut reader = BufReader::new(stdout);
    'read: loop {
        let mut line = Vec::new();
        loop {
            let Ok(available) = reader.fill_buf().await else {
                break 'read;
            };
            if available.is_empty() {
                break 'read;
            }
            let count = available
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(available.len(), |index| index + 1);
            if line.len() + count > 256 * 1024 {
                break 'read;
            }
            line.extend_from_slice(&available[..count]);
            reader.consume(count);
            if line.last() == Some(&b'\n') {
                break;
            }
        }
        let Ok(value) = serde_json::from_slice::<Value>(&line) else {
            break;
        };
        if let Some(id) = value.get("reply_to").and_then(Value::as_str) {
            if let Some(reply) = replies.lock().await.remove(id) {
                let _ = reply.send(Ok(value));
            }
        } else if let Ok(event) = serde_json::from_value::<ExternalAgentEvent>(value) {
            closed |= matches!(event.kind, ExternalObservation::Closed);
            if failure.is_none()
                && let Err(error) = observations.push(event)
            {
                observations.fail(error.clone());
                failure = Some(error);
                if let Some(commands) = commands.upgrade() {
                    let shutdown = json!({"id": Uuid::new_v4().to_string(), "op": "shutdown", "payload": null});
                    let mut bytes = shutdown.to_string().into_bytes();
                    bytes.push(b'\n');
                    let (receipt, _) = oneshot::channel();
                    if commands.writes.try_send(Write { bytes, receipt }).is_err() {
                        commands.close();
                    }
                }
            }
        } else {
            break;
        }
    }
    if !closed && let Some(commands) = commands.upgrade() {
        commands.close();
    }
    for (_, reply) in replies.lock().await.drain() {
        let _ = reply.send(Err("Claude bridge disconnected".into()));
    }
    if !closed && failure.is_none() {
        failure = Some("Claude bridge disconnected; settlement unknown".into());
    }
    observations.finish_stream(failure);
}

#[tracing::instrument(skip_all)]
pub(super) async fn wait_for_exit(mut exit: Exit) -> Result<ExitStatus, String> {
    tokio::time::timeout(Duration::from_secs(/*secs*/ 8), async {
        loop {
            let status = exit.borrow().clone();
            if let Some(status) = status {
                return status;
            }
            exit.changed()
                .await
                .map_err(|_| "Claude bridge exit observer disconnected".to_owned())?;
        }
    })
    .await
    .map_err(|_| "Claude bridge exit not confirmed; settlement unknown".to_owned())?
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
