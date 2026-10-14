use super::ContextualUserFragment;
use codex_protocol::models::ContentItemKind;
use codex_protocol::models::ResponseItem;

pub struct PeerMessage(String);

impl PeerMessage {
    const KIND: &'static str = "peer.message";
    pub(crate) fn matches(item: &ResponseItem) -> bool {
        matches!(item, ResponseItem::Message {
            role,
            internal_chat_message_metadata_passthrough: Some(metadata),
            ..
        } if role == "assistant" && metadata.content_item_kinds.as_deref().is_some_and(|kinds| {
            kinds.len() == 1 && kinds[0].0 == Self::KIND
        }))
    }

    pub fn new(text: String) -> codex_protocol::error::Result<Self> {
        let message = Self(text);
        super::validate_injected_item_size(&message.render_fragment().into())?;
        Ok(message)
    }
}

impl ContextualUserFragment for PeerMessage {
    fn role(&self) -> &'static str {
        "assistant"
    }

    fn content_kind(&self) -> ContentItemKind {
        ContentItemKind(Self::KIND.to_string())
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("", "")
    }

    fn body(&self) -> String {
        self.0.clone()
    }
}
