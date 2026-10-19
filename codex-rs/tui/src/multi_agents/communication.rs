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
        lines
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        std::iter::once(Line::from(self.title.clone()))
            .chain(self.text.lines().map(|line| Line::from(line.to_owned())))
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
    Some(InterAgentCommunicationCell {
        title,
        text,
        cwd: cwd.to_path_buf(),
    })
}
