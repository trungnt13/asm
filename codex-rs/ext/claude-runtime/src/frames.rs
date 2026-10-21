use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::BufReader;
use tokio::sync::mpsc;

pub(crate) const FRAME_LIMIT: usize = 1024 * 1024;
const STDERR_LIMIT: usize = 16 * 1024;

pub(crate) async fn read_frames(
    stdout: impl AsyncRead + Unpin,
    sender: mpsc::Sender<Result<Value>>,
) {
    let mut reader = BufReader::new(stdout);
    loop {
        let frame = read_frame(&mut reader).await;
        let terminal = !matches!(&frame, Ok(Some(_)));
        let frame = match frame {
            Ok(Some(frame)) => Ok(frame),
            Ok(None) => Err(anyhow::anyhow!(
                "Claude stdout closed before the next result"
            )),
            Err(error) => Err(error),
        };
        if sender.send(frame).await.is_err() || terminal {
            return;
        }
    }
}

async fn read_frame(reader: &mut (impl AsyncBufReadExt + Unpin)) -> Result<Option<Value>> {
    let mut bytes = Vec::new();
    loop {
        let available = reader.fill_buf().await.context("reading Claude stdout")?;
        if available.is_empty() {
            if bytes.is_empty() {
                return Ok(None);
            }
            bail!("Claude stdout ended inside a JSON frame");
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let length = newline.map_or(available.len(), |index| index + 1);
        if bytes.len() + length > FRAME_LIMIT {
            bail!("Claude stdout frame exceeded {FRAME_LIMIT} bytes");
        }
        bytes.extend_from_slice(&available[..length]);
        reader.consume(length);
        if newline.is_some() {
            return serde_json::from_slice(&bytes)
                .context("Claude emitted an invalid stream-json frame")
                .map(Some);
        }
    }
}

pub(crate) async fn drain_stderr(
    mut stderr: impl AsyncRead + Unpin,
    tail: Arc<Mutex<VecDeque<u8>>>,
) {
    let mut buffer = [0; 4096];
    loop {
        match stderr.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(length) => {
                let mut tail = tail
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                tail.extend(&buffer[..length]);
                let excess = tail.len().saturating_sub(STDERR_LIMIT);
                tail.drain(..excess);
            }
        }
    }
}
