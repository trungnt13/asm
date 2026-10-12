use super::ContextualUserFragment;
use codex_protocol::models::ContentItemKind;

pub struct PeerMessage(String);

impl PeerMessage {
    pub(crate) const KIND: &'static str = "peer.message";
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
