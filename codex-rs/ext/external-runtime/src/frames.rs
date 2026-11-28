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

const FRAME_LIMIT: usize = 1024 * 1024;
// This is ASM's bounded native JSON transport budget, not a provider image limit.
const IMAGE_FRAME_LIMIT: usize = 32 * FRAME_LIMIT;

#[derive(Clone, Copy)]
pub(crate) enum FramePolicy {
    CursorSdk,
    ClaudeImages,
}

impl FramePolicy {
    fn max_size(self) -> usize {
        match self {
            Self::ClaudeImages => IMAGE_FRAME_LIMIT,
            Self::CursorSdk => FRAME_LIMIT,
        }
    }

    fn output_limit(self, frame: &Value) -> usize {
        match self {
            Self::ClaudeImages
                if frame["type"] == "user"
                    && frame["message"]["content"]
                        .as_array()
                        .is_some_and(|content| {
                            content.iter().any(|block| {
                                block["type"] == "image"
                                    || (block["type"] == "tool_result"
                                        && block["content"].as_array().is_some_and(|parts| {
                                            parts.iter().any(|part| part["type"] == "image")
                                        }))
                            })
                        }) =>
            {
                IMAGE_FRAME_LIMIT
            }
            Self::ClaudeImages | Self::CursorSdk => FRAME_LIMIT,
        }
    }

    pub(crate) fn input_limit(self, frame: &Value) -> usize {
        let limit = match self {
            Self::ClaudeImages => self.output_limit(frame),
            Self::CursorSdk
                if frame["type"] == "prompt"
                    && frame["blocks"].as_array().is_some_and(|blocks| {
                        blocks.iter().any(|block| block["type"] == "image")
                    }) =>
            {
                IMAGE_FRAME_LIMIT
            }
            Self::CursorSdk => FRAME_LIMIT,
        };
        if limit == IMAGE_FRAME_LIMIT {
            // Keep the shared native image input budget. Claude may echo frames with metadata.
            limit - FRAME_LIMIT
        } else {
            limit
        }
    }
}
const STDERR_LIMIT: usize = 16 * 1024;

#[tracing::instrument(skip_all)]
pub(crate) async fn read_frames(
    stdout: impl AsyncRead + Unpin,
    sender: mpsc::Sender<Result<Value>>,
    policy: FramePolicy,
) {
    let mut reader = BufReader::new(stdout);
    loop {
        let frame = read_frame(&mut reader, policy).await;
        let terminal = !matches!(&frame, Ok(Some(_)));
        let frame = match frame {
            Ok(Some(frame)) => Ok(frame),
            Ok(None) => Err(anyhow::anyhow!(
                "native CLI stdout closed before the next result"
            )),
            Err(error) => Err(error),
        };
        if sender.send(frame).await.is_err() || terminal {
            return;
        }
    }
}

#[tracing::instrument(skip_all)]
async fn read_frame(
    reader: &mut (impl AsyncBufReadExt + Unpin),
    policy: FramePolicy,
) -> Result<Option<Value>> {
    let mut bytes = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .await
            .context("reading native CLI stdout")?;
        if available.is_empty() {
            if bytes.is_empty() {
                return Ok(None);
            }
            bail!("native CLI stdout ended inside a JSON frame");
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let length = newline.map_or(available.len(), |index| index + 1);
        if bytes.len() + length > policy.max_size() {
            bail!(
                "native CLI stdout frame exceeded {} bytes",
                policy.max_size()
            );
        }
        bytes.extend_from_slice(&available[..length]);
        reader.consume(length);
        if newline.is_some() {
            let frame = serde_json::from_slice(&bytes)
                .context("native CLI emitted an invalid stream-json frame")?;
            anyhow::ensure!(
                bytes.len() <= policy.output_limit(&frame),
                "native CLI stdout frame exceeded its content size limit"
            );
            return Ok(Some(frame));
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
