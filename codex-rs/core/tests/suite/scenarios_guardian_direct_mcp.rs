//! Prepare captured responses for direct MCP review.

use anyhow::Result;
use codex_history::RolloutItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::input_text;
use pretty_assertions::assert_eq;

pub(in crate::suite) async fn recorded_mcp_items(
    thread: &codex_core::CodexThread,
    prompt: &str,
) -> Result<Vec<ResponseItem>> {
    thread.flush_rollout().await?;
    let history = thread.load_history(/*include_archived*/ false).await?;
    let items = history
        .items
        .into_iter()
        .filter_map(|entry| match entry {
            RolloutItem::ResponseItem(item) => match item.item.as_ref() {
                ResponseItem::Message { role, content, .. }
                    if role == "user" && content == &[input_text(prompt)] =>
                {
                    Some(item.into_item())
                }
                ResponseItem::FunctionCall { .. } => Some(item.into_item()),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(items.len(), 2);
    Ok(items)
}
