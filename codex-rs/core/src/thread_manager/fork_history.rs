use crate::context_manager::missing_call_outputs;
use codex_history::RolloutItem;

pub(super) fn append_interrupted_call_outputs(
    items: &mut Vec<RolloutItem>,
    active_turn_start_index: Option<usize>,
) {
    let inherited_items: Vec<_> = items
        .iter()
        .skip(active_turn_start_index.unwrap_or_default())
        .filter_map(|item| {
            let RolloutItem::ResponseItem(envelope) = item else {
                return None;
            };
            let mut envelope = envelope.clone();
            // Only this copied snapshot, never the source's live history, may lack results.
            envelope
                .metadata
                .get_or_insert_default()
                .inherited_user_message = true;
            Some(envelope)
        })
        .collect();
    // Keep referenced ancestor records unchanged; persist completion in the child's suffix
    // so cold resume also sees a closed call without inheriting the parent's live execution.
    items.extend(
        missing_call_outputs(&inherited_items, /*restored_through_sequence*/ 0)
            .into_iter()
            .map(|(_, output)| RolloutItem::ResponseItem(output)),
    );
}

#[cfg(test)]
#[path = "fork_history_tests.rs"]
mod tests;
