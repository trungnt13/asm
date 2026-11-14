use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadItemPayload;
use codex_app_server_protocol::ThreadTimelineEntry;
use codex_app_server_protocol::Turn;

use crate::transport::OutgoingMessage;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TranscriptSupport {
    Standard,
    AsmV1,
}

pub(crate) fn supported(item: &ThreadItem) -> bool {
    !matches!(item.payload, ThreadItemPayload::InterAgentMessage { .. })
}

pub(crate) fn supported_timeline(entry: &ThreadTimelineEntry) -> bool {
    match entry {
        ThreadTimelineEntry::Item { item, .. } => supported(item),
        ThreadTimelineEntry::Realtime { .. }
        | ThreadTimelineEntry::TurnStarted { .. }
        | ThreadTimelineEntry::TurnCompleted { .. } => true,
    }
}

fn filter_turn(turn: &mut Turn) {
    turn.items.retain(supported);
}

fn filter_thread(thread: &mut Thread) {
    thread.turns.iter_mut().for_each(filter_turn);
}

// Filter only the fork's display projection, never canonical/model-visible history.
// Paging cursors continue to describe the consumed storage rows, including hidden rows.
// An empty page with a next cursor is not the end of history.
pub(crate) fn filter(message: &mut OutgoingMessage) -> bool {
    match message {
        OutgoingMessage::Response(response) => match response.result.as_mut() {
            ClientResponsePayload::ThreadStart(r) => filter_thread(&mut r.thread),
            ClientResponsePayload::ThreadResume(r) => {
                filter_thread(&mut r.thread);
                if let Some(page) = &mut r.initial_turns_page {
                    page.data.iter_mut().for_each(filter_turn);
                }
            }
            ClientResponsePayload::ThreadFork(r) => filter_thread(&mut r.thread),
            ClientResponsePayload::ThreadRead(r) => filter_thread(&mut r.thread),
            ClientResponsePayload::ThreadRevert(r) => filter_thread(&mut r.thread),
            ClientResponsePayload::ThreadUnarchive(r) => filter_thread(&mut r.thread),
            ClientResponsePayload::ThreadMetadataUpdate(r) => filter_thread(&mut r.thread),
            ClientResponsePayload::ThreadList(r) => r.data.iter_mut().for_each(filter_thread),
            ClientResponsePayload::ThreadSearch(r) => {
                r.data
                    .iter_mut()
                    .for_each(|entry| filter_thread(&mut entry.thread));
            }
            ClientResponsePayload::ThreadTurnsList(r) => r.data.iter_mut().for_each(filter_turn),
            ClientResponsePayload::ThreadItemsList(r) => {
                r.data.retain(|entry| supported(&entry.item))
            }
            ClientResponsePayload::ThreadItemsRead(r) => {
                r.data.retain(|entry| supported(&entry.item))
            }
            ClientResponsePayload::ThreadTimelineList(r) => r.data.retain(supported_timeline),
            ClientResponsePayload::TurnStart(r) => filter_turn(&mut r.turn),
            ClientResponsePayload::ThreadQueueStart(r) => filter_turn(&mut r.turn),
            ClientResponsePayload::ReviewStart(r) => filter_turn(&mut r.turn),
            _ => {}
        },
        OutgoingMessage::AppServerNotification(envelope) => match &mut envelope.notification {
            ServerNotification::ThreadStarted(n) => filter_thread(&mut n.thread),
            ServerNotification::TurnStarted(n) => filter_turn(&mut n.turn),
            ServerNotification::TurnCompleted(n) => filter_turn(&mut n.turn),
            ServerNotification::ItemStarted(n) => return supported(&n.item),
            ServerNotification::ItemCompleted(n) => return supported(&n.item),
            _ => {}
        },
        _ => {}
    }
    true
}
