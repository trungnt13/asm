use super::CursorRuntime;
use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use serde_json::Value;
use serde_json::json;

impl CursorRuntime {
    /// Rejects unsupported images and oversized frames before ASM records pending input.
    pub fn validate_prompt(&self, prompt: &Value) -> Result<()> {
        self.prompt_frame(prompt).map(|_| ())
    }

    pub(super) fn prompt_frame(&self, prompt: &Value) -> Result<Value> {
        let mut blocks = Vec::new();
        match prompt {
            Value::String(text) => {
                let text = match &self.bridge_instructions {
                    Some(instructions) => format!("{instructions}\n\n{text}"),
                    None => text.to_owned(),
                };
                blocks.push(json!({"type": "text", "text": text}));
            }
            Value::Array(content) => {
                if let Some(instructions) = &self.bridge_instructions {
                    blocks.push(json!({"type": "text", "text": format!("{instructions}\n\n")}));
                }
                for block in content {
                    match block["type"].as_str() {
                        Some("text") => {
                            let text = block["text"]
                                .as_str()
                                .context("native text block has no text")?;
                            blocks.push(json!({"type": "text", "text": text}));
                        }
                        Some("image") => {
                            anyhow::ensure!(
                                self.supports_images,
                                "Cursor agent does not advertise ACP image prompts; upgrade the Cursor CLI"
                            );
                            let source = &block["source"];
                            anyhow::ensure!(
                                source["type"] == "base64",
                                "Cursor images require base64 content"
                            );
                            let mime_type = source["media_type"]
                                .as_str()
                                .context("native image has no media type")?;
                            let data = source["data"]
                                .as_str()
                                .context("native image has no base64 data")?;
                            blocks.push(
                                json!({"type": "image", "mimeType": mime_type, "data": data}),
                            );
                        }
                        _ => bail!("unsupported content block for Cursor"),
                    }
                }
            }
            _ => bail!("Cursor prompts require text or content blocks"),
        }
        // Effort updates can consume request IDs after early admission checks.
        // Reserve the widest ID so validation remains safe across those updates.
        let mut frame = json!({"jsonrpc": "2.0", "id": u64::MAX,
            "method": "session/prompt", "params": {"sessionId": self.session_id, "prompt": blocks}});
        let limit = crate::frames::FramePolicy::CursorImages.input_limit(&frame);
        anyhow::ensure!(
            serde_json::to_vec(&frame)?.len() < limit,
            "Cursor input exceeds the native ACP frame limit ({limit} bytes); reduce the image or prompt size"
        );
        frame["id"] = json!(self.next_request_id + 1);
        Ok(frame)
    }
}
