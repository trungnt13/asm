//! Apply local automatic-title settings without changing manual rename suggestions.

use super::App;
use super::thread_events::ThreadBufferedEvent;
use super::thread_title::THREAD_TITLE_MAX_CHARS;
use super::thread_title::THREAD_TITLE_PROMPT_MAX_BYTES;
use super::thread_title::recent_conversation_messages;
use super::thread_title::thread_title_instructions;
use crate::app_event::ThreadTitleDestination;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_config::AutoRenameConfig;
use codex_config::AutoRenameContext;
use codex_config::AutoRenameFirstTrigger;

impl App {
    #[tracing::instrument(skip_all)]
    pub(super) async fn automatic_thread_title_prompt(
        &self,
        event: &ThreadBufferedEvent,
    ) -> Option<String> {
        let settings = &self.local_settings.auto_rename;
        if !settings.enabled || self.chat_widget.thread_name().is_some() {
            return None;
        }
        let ThreadBufferedEvent::Notification(notification) = event else {
            return None;
        };
        let (turn_id, items) = match (settings.first_trigger, notification.as_ref()) {
            (AutoRenameFirstTrigger::FirstUserMessage, ServerNotification::ItemCompleted(item))
                if matches!(item.item, ThreadItem::UserMessage { .. }) =>
            {
                (item.turn_id.as_str(), std::slice::from_ref(&item.item))
            }
            (
                AutoRenameFirstTrigger::FirstCompletedTurn,
                ServerNotification::TurnCompleted(turn),
            ) if turn.turn.status == TurnStatus::Completed => {
                (turn.turn.id.as_str(), turn.turn.items.as_slice())
            }
            _ => return None,
        };
        if settings.context == AutoRenameContext::UserMessage
            && let Some(source) = items.iter().rev().find_map(user_message_text)
        {
            return (!source.trim().is_empty()).then(|| automatic_title_prompt(settings, &source));
        }
        let thread_id = self.active_thread_id?;
        if self
            .pending_thread_titles
            .contains_key(&(thread_id, ThreadTitleDestination::Automatic))
        {
            return None;
        }
        let channel = self.thread_event_channels.get(&thread_id)?;
        let store = channel.store.lock().await;
        let source = match settings.context {
            AutoRenameContext::UserMessage => store
                .turns
                .iter()
                .filter(|turn| turn.id == turn_id)
                .flat_map(|turn| turn.items.iter())
                .chain(store.buffer.iter().filter_map(|event| {
                    let ThreadBufferedEvent::Notification(notification) = event else {
                        return None;
                    };
                    let ServerNotification::ItemCompleted(item) = notification.as_ref() else {
                        return None;
                    };
                    (item.turn_id == turn_id).then_some(&item.item)
                }))
                .rev()
                .find_map(user_message_text)?,
            AutoRenameContext::RecentConversation => {
                let mut seen = std::collections::HashSet::new();
                recent_conversation_messages(
                    store
                        .turns
                        .iter()
                        .flat_map(|turn| turn.items.iter())
                        .chain(store.buffer.iter().filter_map(|event| {
                            let ThreadBufferedEvent::Notification(notification) = event else {
                                return None;
                            };
                            let ServerNotification::ItemCompleted(item) = notification.as_ref()
                            else {
                                return None;
                            };
                            Some(&item.item)
                        }))
                        .chain(items.iter())
                        .filter(|item| seen.insert(item.id().to_string())),
                    Some(settings),
                )?
            }
        };
        (!source.trim().is_empty()).then(|| automatic_title_prompt(settings, &source))
    }
}

pub(super) fn automatic_title_prefix(settings: &AutoRenameConfig) -> String {
    let max_title_chars = settings.max_title_chars.unwrap_or(THREAD_TITLE_MAX_CHARS);
    let mut prefix = thread_title_instructions(max_title_chars);
    if let Some(instructions) = &settings.instructions
        && !instructions.trim().is_empty()
    {
        prefix.push_str("\nAdditional naming guidance:\n");
        prefix.push_str(instructions.trim());
    }
    match settings.context {
        AutoRenameContext::UserMessage => prefix.push_str("\n\nUser prompt:\n"),
        AutoRenameContext::RecentConversation => prefix.push_str(
            "\nPrioritize the current task and latest substantive user request.\n\nRecent conversation messages:\n",
        ),
    }
    prefix
}

pub(super) fn automatic_context_bytes(settings: &AutoRenameConfig) -> usize {
    settings.max_context_bytes.unwrap_or_else(|| {
        THREAD_TITLE_PROMPT_MAX_BYTES.saturating_sub(
            automatic_title_prefix(&AutoRenameConfig {
                context: settings.context,
                ..Default::default()
            })
            .len(),
        )
    })
}

pub(super) fn automatic_title_prompt(settings: &AutoRenameConfig, source: &str) -> String {
    if *settings == AutoRenameConfig::default() {
        return super::thread_title::thread_title_prompt(source);
    }
    let prefix = automatic_title_prefix(settings);
    let source = source.trim();
    let mut end = source
        .len()
        .min(automatic_context_bytes(settings))
        .min(9500_usize.saturating_sub(prefix.len()));
    while !source.is_char_boundary(end) {
        end -= 1;
    }
    format!("{prefix}{}", &source[..end])
}

fn user_message_text(item: &ThreadItem) -> Option<String> {
    let ThreadItem::UserMessage { content, .. } = item else {
        return None;
    };
    Some(
        content
            .iter()
            .filter_map(|input| match input {
                UserInput::Text { text, .. } => {
                    Some(crate::ide_context::extract_prompt_request_with_offset(text).0)
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}
