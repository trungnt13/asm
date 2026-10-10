use super::input_queue::TurnInput;
use codex_history::ResponseItemEnvelope;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::user_input::UserInput;

#[derive(Default)]
pub(super) struct AdaptiveEvidence {
    pub(super) task: String,
    steer: String,
    pub(super) previous: String,
}

impl AdaptiveEvidence {
    pub(super) fn collect(
        &mut self,
        input: &[TurnInput],
        history: &[ResponseItemEnvelope],
        limit: usize,
    ) -> String {
        let mut incoming = String::new();
        for item in input.iter().take(/*n*/ 128) {
            if let TurnInput::UserInput { content, .. } = item {
                for item in content.iter().take(/*n*/ 32) {
                    if let UserInput::Text { text, .. } = item {
                        append_text(&mut incoming, text, limit / 3);
                    }
                }
            }
        }
        if !incoming.is_empty() {
            if self.task.is_empty() {
                self.task = incoming;
            } else {
                self.steer = if incoming == self.task {
                    String::new()
                } else {
                    incoming
                };
            }
        }
        if self.task.is_empty() {
            for envelope in history.iter().rev().take(/*n*/ 128) {
                if !envelope.metadata.as_ref().is_some_and(|metadata| {
                    metadata.user_input_order.is_some() && !metadata.compaction_output
                }) {
                    continue;
                }
                if let ResponseItem::Message { role, content, .. } = &envelope.item
                    && role == "user"
                {
                    for item in content.iter().take(/*n*/ 32) {
                        if let ContentItem::InputText { text } = item {
                            append_text(&mut self.task, text, limit / 3);
                        }
                    }
                    if !self.task.is_empty() {
                        break;
                    }
                }
            }
        }
        let mut text = String::new();
        append_text(&mut text, "Current user task:\n", limit);
        append_text(&mut text, &self.task, limit);
        if !self.steer.is_empty() {
            append_text(&mut text, "\nLatest user steer:\n", limit);
            append_text(&mut text, &self.steer, limit * 2 / 3);
        }
        append_text(&mut text, "\nRecent evidence (newest first):\n", limit);
        for envelope in history.iter().rev().take(/*n*/ 128) {
            if text.len() >= limit {
                break;
            }
            if !envelope
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.compaction_output)
            {
                append_item(&mut text, &envelope.item, limit);
            }
        }
        text
    }
}

fn append_text(output: &mut String, text: &str, limit: usize) {
    let remaining = limit.saturating_sub(output.len());
    output.push_str(&text[..text.floor_char_boundary(remaining.min(text.len()))]);
}

fn append_item(output: &mut String, item: &ResponseItem, limit: usize) {
    match item {
        ResponseItem::Message { role, content, .. } if role == "assistant" => {
            append_text(output, &format!("\n{role}: "), limit);
            for content in content.iter().take(/*n*/ 32) {
                if let ContentItem::InputText { text } | ContentItem::OutputText { text } = content
                {
                    append_text(output, text, limit);
                }
            }
        }
        ResponseItem::FunctionCallOutput {
            output: payload, ..
        }
        | ResponseItem::CustomToolCallOutput {
            output: payload, ..
        } => {
            append_text(output, "\ntool: ", limit);
            match &payload.body {
                FunctionCallOutputBody::Text(text) => append_text(output, text, limit),
                FunctionCallOutputBody::ContentItems(items) => {
                    for item in items.iter().take(/*n*/ 32) {
                        if let FunctionCallOutputContentItem::InputText { text } = item {
                            append_text(output, text, limit);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "adaptive_reasoning_evidence_tests.rs"]
mod tests;
