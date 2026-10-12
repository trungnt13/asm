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
    pub(super) encrypted_task_unavailable: bool,
    steer: String,
}

impl AdaptiveEvidence {
    pub(super) fn collect(
        &mut self,
        input: &[TurnInput],
        history: &[ResponseItemEnvelope],
        limit: usize,
    ) -> String {
        // Keep scan work bounded, but let larger byte budgets reach more history and parts.
        let item_limit = limit.div_ceil(/*rhs*/ 64).max(/*other*/ 128);
        let part_limit = limit.div_ceil(/*rhs*/ 256).max(/*other*/ 32);
        let mut incoming = String::new();
        let mut encrypted_input = false;
        for item in input.iter().take(item_limit) {
            match item {
                TurnInput::UserInput { content, .. } => {
                    for item in content.iter().take(part_limit) {
                        if let UserInput::Text { text, .. } = item {
                            append_text(&mut incoming, text, limit / 3);
                        }
                    }
                }
                TurnInput::InterAgentCommunication(communication) => {
                    // Assigned tasks and steering are untrusted text, not message metadata.
                    if communication.encrypted_content.is_none() {
                        append_text(&mut incoming, &communication.content, limit / 3);
                    } else {
                        encrypted_input = true;
                        self.encrypted_task_unavailable = true;
                    }
                }
                TurnInput::FunctionCallOutput(_) | TurnInput::ResponseItem(_) => {}
            }
        }
        if !incoming.is_empty() {
            if !encrypted_input {
                self.encrypted_task_unavailable = false;
            }
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
            for envelope in history.iter().rev().take(item_limit) {
                if !envelope.metadata.as_ref().is_some_and(|metadata| {
                    metadata.user_input_order.is_some() && !metadata.compaction_output
                }) {
                    continue;
                }
                if let ResponseItem::Message { role, content, .. } = &envelope.item
                    && role == "user"
                {
                    for item in content.iter().take(part_limit) {
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
        for envelope in history.iter().rev().take(item_limit) {
            if text.len() >= limit {
                break;
            }
            if !envelope
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.compaction_output)
            {
                append_item(&mut text, envelope, limit, part_limit);
            }
        }
        text
    }
}

fn append_text(output: &mut String, text: &str, limit: usize) {
    let remaining = limit.saturating_sub(output.len());
    output.push_str(&text[..text.floor_char_boundary(remaining.min(text.len()))]);
}

fn append_item(
    output: &mut String,
    envelope: &ResponseItemEnvelope,
    limit: usize,
    part_limit: usize,
) {
    match &envelope.item {
        ResponseItem::Message { role, content, .. }
            if role == "assistant"
                || (role == "user"
                    && envelope
                        .metadata
                        .as_ref()
                        .is_some_and(|metadata| metadata.user_input_order.is_some())) =>
        {
            append_text(output, &format!("\n{role}: "), limit);
            for content in content.iter().take(part_limit) {
                if output.len() >= limit {
                    break;
                }
                if let ContentItem::InputText { text } | ContentItem::OutputText { text, .. } =
                    content
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
                    for item in items.iter().take(part_limit) {
                        if output.len() >= limit {
                            break;
                        }
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
