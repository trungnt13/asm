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

    pub fn validate_retained_prompt(
        prompt: &Prompt,
        session_id: &str,
        messaging: bool,
    ) -> Result<()> {
        prompt_frame(
            prompt,
            session_id,
            messaging.then_some(super::BRIDGE_INSTRUCTIONS),
            /*supports_images*/ false,
        )
        .map(|_| ())
    }

    pub(super) fn prompt_frame(&self, prompt: &Prompt) -> Result<Value> {
        let mut frame = prompt_frame(
            prompt,
            &self.session_id,
            self.bridge_instructions.as_deref(),
            self.supports_images,
        )?;
        frame["id"] = json!(self.next_request_id + 1);
        Ok(frame)
    }
}

fn prompt_frame(
    prompt: &Prompt,
    session_id: &str,
    bridge_instructions: Option<&str>,
    supports_images: bool,
) -> Result<Value> {
    let mut blocks = Vec::new();
    match prompt {
        Prompt::Text(text) => {
            let text = match bridge_instructions {
                Some(instructions) => format!("{instructions}\n\n{text}"),
                None => text.to_owned(),
            };
            blocks.push(json!({"type": "text", "text": text}));
        }
        Prompt::Blocks(content) => {
            if let Some(instructions) = bridge_instructions {
                blocks.push(json!({"type": "text", "text": format!("{instructions}\n\n")}));
            }
            for block in content {
                match block {
                    InputBlock::Text { text } => {
                        blocks.push(json!({"type": "text", "text": text}));
                    }
                    InputBlock::Image { mime_type, data } => {
                        anyhow::ensure!(
                            supports_images,
                            "Cursor agent does not advertise ACP image prompts; upgrade the Cursor CLI"
                        );
                        blocks.push(json!({"type": "image", "mimeType": mime_type, "data": data}));
                    }
                }
            }
        }
    }
    // Effort updates can consume request IDs after early admission checks.
    // Reserve the widest ID so validation remains safe across those updates.
    let frame = json!({"jsonrpc": "2.0", "id": u64::MAX,
            "method": "session/prompt", "params": {"sessionId": session_id, "prompt": blocks}});
    let limit = crate::frames::FramePolicy::CursorImages.input_limit(&frame);
    anyhow::ensure!(
        serde_json::to_vec(&frame)?.len() < limit,
        "Cursor input exceeds the native ACP frame limit ({limit} bytes); reduce the image or prompt size"
    );
    Ok(frame)
}
