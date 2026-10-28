use super::CursorRuntime;
use crate::InputBlock;
use crate::Prompt;
use anyhow::Result;
use serde_json::Value;
use serde_json::json;

impl CursorRuntime {
    /// Rejects unsupported images and oversized frames before ASM records pending input.
    pub fn validate_prompt(&self, prompt: &Prompt) -> Result<()> {
        self.prompt_frame(prompt).map(|_| ())
    }

    pub(super) fn prompt_frame(&self, prompt: &Prompt) -> Result<Value> {
        let mut blocks = Vec::new();
        match prompt {
            Prompt::Text(text) => {
                let text = match &self.bridge_instructions {
                    Some(instructions) => format!("{instructions}\n\n{text}"),
                    None => text.to_owned(),
                };
                blocks.push(json!({"type": "text", "text": text}));
            }
            Prompt::Blocks(content) => {
                if let Some(instructions) = &self.bridge_instructions {
                    blocks.push(json!({"type": "text", "text": format!("{instructions}\n\n")}));
                }
                for block in content {
                    match block {
                        InputBlock::Text { text } => {
                            blocks.push(json!({"type": "text", "text": text}));
                        }
                        InputBlock::Image { mime_type, data } => {
                            anyhow::ensure!(
                                self.supports_images,
                                "Cursor agent does not advertise ACP image prompts; upgrade the Cursor CLI"
                            );
                            blocks.push(
                                json!({"type": "image", "mimeType": mime_type, "data": data}),
                            );
                        }
                    }
                }
            }
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
