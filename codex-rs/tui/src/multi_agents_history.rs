//! Keep the source of unnamed collab rows until background thread discovery can label them.

use super::wait_history::AgentWaitHistoryCell;
use super::*;
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct CollabHistoryCell {
    rendered: LabeledToolCell<WebHyperlinkHistoryCell>,
    unresolved: Option<Box<Unresolved>>,
}

#[derive(Debug)]
struct Unresolved {
    item: ThreadItemPayload,
    spawn_request: Option<SpawnRequestSummary>,
    missing: Vec<ThreadId>,
}

pub(crate) fn tool_call_history_cell(
    item: &ThreadItemPayload,
    cached_spawn_request: Option<&SpawnRequestSummary>,
    metadata: impl Fn(ThreadId) -> AgentMetadata,
) -> Option<CollabHistoryCell> {
    let ThreadItemPayload::CollabAgentToolCall { agents_states, .. } = item else {
        return None;
    };
    let mut missing = Vec::new();
    let rendered = render_tool_call(item, cached_spawn_request, |id| {
        let agent = metadata(id);
        if agent.nickname().is_none()
            && agents_states
                .get(&id.to_string())
                .is_none_or(|s| s.status != CollabAgentStatus::NotFound)
        {
            missing.push(id);
        }
        agent
    })?;
    let unresolved = (!missing.is_empty()).then(|| {
        Box::new(Unresolved {
            item: item.clone(),
            spawn_request: cached_spawn_request.cloned(),
            missing,
        })
    });
    Some(CollabHistoryCell {
        rendered,
        unresolved,
    })
}

pub(crate) fn resolve_collab_history_cell(
    cell: &mut dyn HistoryCell,
    metadata: impl Fn(ThreadId) -> AgentMetadata,
) -> bool {
    let preview = if cell.as_any().is::<AgentWaitHistoryCell>() {
        cell.as_any_mut()
            .downcast_mut::<AgentWaitHistoryCell>()
            .map(|wait| &mut wait.preview)
    } else {
        cell.as_any_mut().downcast_mut::<CollabHistoryCell>()
    };
    let Some(preview) = preview else {
        return false;
    };
    if let Some(updated) = preview.refreshed(metadata) {
        *preview = updated;
    }
    preview.unresolved.is_some()
}

pub(crate) fn refresh_collab_history_cell(
    cell: &dyn HistoryCell,
    metadata: impl Fn(ThreadId) -> AgentMetadata,
) -> Option<Arc<dyn HistoryCell>> {
    if let Some(wait) = cell.as_any().downcast_ref::<AgentWaitHistoryCell>() {
        let preview = wait.preview.refreshed(metadata)?;
        return Some(Arc::new(wait.with_refreshed_preview(preview)));
    }
    let preview = cell.as_any().downcast_ref::<CollabHistoryCell>()?;
    Some(Arc::new(preview.refreshed(metadata)?))
}

impl CollabHistoryCell {
    fn refreshed(&self, metadata: impl Fn(ThreadId) -> AgentMetadata) -> Option<Self> {
        let source = self.unresolved.as_ref()?;
        if !source
            .missing
            .iter()
            .any(|id| metadata(*id).nickname().is_some())
        {
            return None;
        }
        tool_call_history_cell(&source.item, source.spawn_request.as_ref(), metadata)
    }
}

impl HistoryCell for CollabHistoryCell {
    fn tool_call_labels(&self) -> Vec<String> {
        self.rendered.tool_call_labels()
    }

    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.rendered.display_lines(width)
    }

    fn display_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        self.rendered.display_hyperlink_lines(width)
    }

    fn transcript_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        self.rendered.transcript_hyperlink_lines(width)
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        self.rendered.raw_lines()
    }
}
