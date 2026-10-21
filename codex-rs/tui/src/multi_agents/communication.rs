//! Fork-only transcript presentation; delivery and model history stay unchanged.

use crate::history_cell::HistoryCell;
use ratatui::style::Stylize;
use ratatui::text::Line;
use std::path::PathBuf;

#[derive(Debug)]
struct InterAgentCommunicationCell {
    title: String,
    text: String,
    cwd: PathBuf,
    spawn_arguments: Option<Vec<String>>,
}

impl HistoryCell for InterAgentCommunicationCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = vec![Line::from(self.title.clone().bold())];
        crate::markdown::append_markdown(
            &self.text,
            Some(usize::from(width)),
            Some(&self.cwd),
            &mut lines,
        );
        if let Some(arguments) = &self.spawn_arguments {
            lines.push(Line::default());
            lines.push(Line::from("Spawn arguments:").dim());
            for text in arguments {
                lines.extend(crate::wrapping::word_wrap_lines(
                    [Line::from(text.clone()).dim()],
                    crate::wrapping::RtOptions::new(usize::from(width)),
                ));
            }
        }
        lines
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        std::iter::once(Line::from(self.title.clone()))
            .chain(self.text.lines().map(|line| Line::from(line.to_owned())))
            .chain(self.spawn_arguments.iter().flat_map(|arguments| {
                std::iter::once(Line::default())
                    .chain(std::iter::once(Line::from("Spawn arguments:")))
                    .chain(arguments.iter().map(|line| Line::from(line.clone())))
            }))
            .collect()
    }
}

pub(crate) fn communication_history_cell(
    item: &codex_app_server_protocol::ThreadItem,
    cwd: &std::path::Path,
    own_agent_role: Option<&str>,
) -> Option<impl HistoryCell + 'static> {
    use codex_app_server_protocol::CollabAgentTool;
    use codex_app_server_protocol::CollabAgentToolCallStatus;
    use codex_app_server_protocol::ThreadItem;

    let (title, text) = match item {
        ThreadItem::InterAgentMessage {
            author,
            recipient,
            text,
            ..
        } => {
            let role = own_agent_role
                .filter(|_| text.starts_with("Message Type: NEW_TASK\n"))
                .map(|role| format!(" [{role}]"))
                .unwrap_or_default();
            (
                format!("Received · {author} → {recipient}{role}"),
                text.clone(),
            )
        }
        ThreadItem::CollabAgentToolCall {
            tool,
            status,
            prompt: Some(text),
            receiver_thread_ids,
            ..
        } if !matches!(status, CollabAgentToolCallStatus::InProgress)
            && matches!(
                tool,
                CollabAgentTool::SpawnAgent
                    | CollabAgentTool::SendMessage
                    | CollabAgentTool::FollowupTask
                    | CollabAgentTool::SendInput
            ) =>
        {
            let action = match tool {
                CollabAgentTool::SpawnAgent => "Task",
                CollabAgentTool::FollowupTask => "Follow-up",
                _ => "Message",
            };
            let recipients = receiver_thread_ids.join(", ");
            (
                format!("{action} to {recipients} · {status:?}"),
                text.clone(),
            )
        }
        _ => return None,
    };
    let spawn_arguments = match item {
        ThreadItem::InterAgentMessage {
            spawn_arguments, ..
        } => spawn_arguments.as_deref(),
        _ => None,
    }
    .and_then(|arguments| {
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(arguments).ok()
    })
    .map(|arguments| {
        arguments
            .into_iter()
            .map(|(key, value)| {
                let value = match value {
                    serde_json::Value::String(value) => value,
                    value => value.to_string(),
                };
                format!("{key}: {value}")
            })
            .collect()
    });
    Some(InterAgentCommunicationCell {
        title,
        text,
        cwd: cwd.to_path_buf(),
        spawn_arguments,
    })
}
