use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use codex_protocol::models::ResponseItem;

// Serialized bytes conservatively bound text tokens, including envelope overhead.
const MAX_INJECTED_ITEM_BYTES: usize = 10_000;

pub(crate) fn validate_injected_item_size(item: &ResponseItem) -> Result<()> {
    let bytes =
        serde_json::to_vec(item).map_err(|error| CodexErr::InvalidRequest(error.to_string()))?;
    if bytes.len() > MAX_INJECTED_ITEM_BYTES {
        return Err(CodexErr::InvalidRequest(format!(
            "injected context exceeds the {MAX_INJECTED_ITEM_BYTES}-byte conservative bound for the 10,000-token item limit"
        )));
    }
    Ok(())
}
