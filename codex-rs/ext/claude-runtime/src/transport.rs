use std::collections::HashMap;
use std::process::ExitStatus;
use std::sync::Arc;
use std::time::Duration;

use codex_extension_api::ExternalAgentEvent;
use codex_extension_api::ExternalObservation;
use serde_json::Value;
use serde_json::json;
use tokio::io::AsyncBufReadExt;
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

type Replies = Arc<Mutex<HashMap<String, oneshot::Sender<Result<(), String>>>>>;
pub(super) type Exit = watch::Receiver<Option<Result<ExitStatus, String>>>;

struct Write {
    bytes: Vec<u8>,
    receipt: oneshot::Sender<Result<(), String>>,
}

pub(super) struct Commands {
    writes: mpsc::Sender<Write>,
    close: Arc<Notify>,
    replies: Replies,
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
        let (writes, receiver) = mpsc::channel(/*buffer*/ 16);
        let close = Arc::new(Notify::new());
        let commands = Arc::new(Self {
            writes,
            close: Arc::clone(&close),
            replies: Arc::new(Mutex::new(HashMap::new())),
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
    pub(super) async fn request(&self, op: &str, payload: Value) -> Result<(), String> {
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
                let result = if value.get("ok").and_then(Value::as_bool) == Some(true) {
                    Ok(())
                } else {
                    Err(value
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("Bridge command failed")
                        .to_owned())
                };
                let _ = reply.send(result);
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
