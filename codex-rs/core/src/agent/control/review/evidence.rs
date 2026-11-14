use super::*;
use crate::compact::content_items_to_text;
use crate::compact::is_summary_message;
use crate::context::is_guardian_context_message;
use crate::context::is_user_authorization_message;
use crate::session::session::Session;
use codex_history::RetainedContextEntry;
use codex_protocol::models::ContentItem::InputText;
use codex_protocol::models::ContentItem::OutputText;
use codex_protocol::models::ResponseItem;
use serde_json::Value;
use serde_json::json;
use std::fmt::Write;

impl ReviewRun {
    pub(super) async fn pin_evidence(&mut self, root: &Session) {
        let history = root.clone_history().await;
        let retained = history.retained_context();
        self.omissions = !retained.verified_answers_complete()
            || retained.has_missing_user_messages()
            || retained.has_omitted_assistant_messages();
        for envelope in history.annotated_items() {
            let item = &envelope.item;
            let metadata = envelope.metadata.as_ref();
            if metadata.is_some_and(|m| m.compaction_output)
                || matches!(
                    item,
                    ResponseItem::Compaction { .. } | ResponseItem::ContextCompaction { .. }
                )
            {
                self.omissions = true;
                continue;
            }
            let ResponseItem::Message {
                role,
                content,
                phase,
                ..
            } = item
            else {
                continue;
            };
            let text = content_items_to_text(content.as_slice()).unwrap_or_default();
            if role != "assistant"
                && (!is_user_authorization_message(item)
                    || is_guardian_context_message(item)
                    || is_summary_message(&text))
            {
                continue;
            }
            let source = metadata.and_then(|m| m.retained_source.as_ref());
            let origin =
                (role == "user").then(|| codex_history::UserInputOrigin::from_message(item));
            let complete = source.is_none_or(|s| s.complete)
                && content
                    .iter()
                    .all(|c| matches!(c, InputText { .. } | OutputText { .. }));
            self.append(
                json!({"source":"pinned_history_order", "role":role, "complete":complete, "text":text,
                "message_id":item.id(), "phase":phase, "turn_id":item.turn_id(), "origin":origin, "retained_source":source}),
            );
        }
        for (_, entry) in retained.ordered_entries() {
            let (role, message) = match entry {
                RetainedContextEntry::UserMessage(m) => ("user", m),
                RetainedContextEntry::AssistantMessage(m) => ("assistant", m),
                RetainedContextEntry::VerifiedAnswer(a) => {
                    self.append(
                        json!({"role":"user","source":"host_verified_clarification","complete":!a.questions.is_empty(),
                      "turn_id":a.turn_id,"call_id":a.call_id,"text":codex_guardian_context::render_original_verified_answer(a)}),
                    );
                    continue;
                }
            };
            let mut record = json!(message);
            record["role"] = role.into();
            record["source"] = "retained_original_supplement".into();
            self.append(record);
        }
    }

    fn append(&mut self, record: Value) {
        self.omissions |= record["complete"] == false;
        writeln!(self.evidence, "{record}").expect("writing to String");
    }

    pub(crate) fn page(&self, cursor: Option<&str>) -> Result<String, String> {
        let prefix = format!("{}:", self.id);
        let offset = match cursor {
            None => 0,
            Some(s) => s
                .strip_prefix(&prefix)
                .and_then(|s| s.parse().ok())
                .ok_or("invalid snapshot cursor")?,
        };
        let rest = self
            .evidence
            .get(offset..)
            .ok_or("invalid evidence offset")?;
        let mut size = rest.floor_char_boundary(rest.len().min(512));
        loop {
            let end = offset + size;
            let page = json!({
                "snapshot_id":self.id, "root_session_id":self.root, "omissions_observed":self.omissions,
                "coverage":"Concatenate JSONL. Retained supplements may repeat IDs and follow history, not global chronology. Tools/media omitted; legacy coverage may be incomplete.",
                "text":&rest[..size], "next_cursor":(end < self.evidence.len()).then(|| format!("{prefix}{end}")),
            }).to_string();
            if size == 0 && !rest.is_empty() {
                return Err("snapshot page cannot fit evidence".into());
            }
            if page.len() <= 900 {
                return Ok(page);
            }
            size = rest.floor_char_boundary(size / 2);
        }
    }
}
