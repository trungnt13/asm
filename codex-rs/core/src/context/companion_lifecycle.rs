use std::collections::HashSet;

use super::ContextualUserFragment;
use super::UserGoalUpdate;
use codex_protocol::ResponseItemId;
use codex_protocol::companion::CompanionLifecycleEvent;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ContentItemKind;
use codex_protocol::models::ResponseItem;

pub(crate) struct CompanionLifecycle(pub(crate) CompanionLifecycleEvent);

impl CompanionLifecycle {
    const KIND: &str = "companion.lifecycle";

    fn matches(item: &ResponseItem) -> bool {
        matches!(item, ResponseItem::Message {
            role,
            content,
            internal_chat_message_metadata_passthrough: Some(metadata),
            ..
        } if role == "user"
            && matches!(content.as_slice(), [ContentItem::InputText { .. }])
            && metadata.content_item_kinds.as_deref().is_some_and(|kinds| {
                kinds.len() == 1 && kinds[0].0 == Self::KIND
            }))
    }
}

pub(crate) fn is_retained_host_context_message(item: &ResponseItem) -> bool {
    UserGoalUpdate::message_text(item).is_some() || CompanionLifecycle::matches(item)
}

pub(crate) fn retained_host_context_message_ids<'a>(
    items: impl Iterator<Item = &'a ResponseItem>,
) -> HashSet<ResponseItemId> {
    items
        .filter(|item| is_retained_host_context_message(item))
        .filter_map(ResponseItem::id)
        .cloned()
        .collect()
}

impl ContextualUserFragment for CompanionLifecycle {
    fn role(&self) -> &'static str {
        "user"
    }

    fn content_kind(&self) -> ContentItemKind {
        ContentItemKind(Self::KIND.to_owned())
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        (
            "<codex_internal_context source=\"companion_lifecycle\">",
            "</codex_internal_context>",
        )
    }

    fn body(&self) -> String {
        format!(
            "\nCompanion lifecycle information only; not a user request or authorization to work.\n{}\n",
            serde_json::json!(self.0)
        )
    }
}
