//! Optional presentation groups over unchanged history cells.
//! Each group opens to the original previews, not full tool output.

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Weak;

use crate::history_cell::ToolCallSummary;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;

use super::*;

#[derive(Default)]
pub(super) struct CollapsedToolGroups {
    enabled: bool,
    summaries: RefCell<HashMap<EntryKey, CachedToolSummary>>,
    layouts: HashMap<(usize, usize, usize), Arc<TextLayout>>,
}

struct CachedToolSummary {
    source: Weak<dyn HistoryCell>,
    summary: Option<ToolCallSummary>,
}

impl CollapsedToolGroups {
    pub(super) fn invalidate_layouts(&mut self) {
        self.summaries
            .get_mut()
            .retain(|_, entry| entry.source.strong_count() > 0);
        self.layouts.clear();
    }

    fn summary(&self, cell: &Arc<dyn HistoryCell>) -> Option<ToolCallSummary> {
        if !cell.has_stable_transcript_height() {
            return cell.tool_call_summary();
        }
        self.summaries
            .borrow_mut()
            .entry(EntryKey::cell(cell))
            .or_insert_with(|| CachedToolSummary {
                source: Arc::downgrade(cell),
                summary: cell.tool_call_summary(),
            })
            .summary
            .clone()
    }
}

impl TranscriptView {
    pub(crate) fn set_collapse_tool_calls(&mut self, enabled: bool) {
        if self.collapsed_tools.enabled != enabled {
            self.collapsed_tools.enabled = enabled;
            self.collapsed_tools.invalidate_layouts();
            self.cache.clear();
            self.live_key = None;
        }
    }

    pub(super) fn groups_tools(&self) -> bool {
        self.collapsed_tools.enabled && !self.detailed && self.mode == HistoryRenderMode::Rich
    }

    fn pinned_tool_group(&self, cells: &[Arc<dyn HistoryCell>], index: usize) -> bool {
        cells.get(index).is_some_and(|cell| {
            self.snapshot()
                .and_then(|snapshot| snapshot.pinned.get(&EntryKey::cell(cell)))
                .is_some_and(|layout| layout.disclosure && layout.row_count() > 0)
        })
    }

    pub(super) fn tool_group_start(
        &self,
        cells: &[Arc<dyn HistoryCell>],
        index: usize,
    ) -> Option<usize> {
        if !self.groups_tools() || self.collapsed_tools.summary(cells.get(index)?).is_none() {
            return None;
        }
        let mut start = index;
        while start > 0
            && !self.pinned_tool_group(cells, start)
            && self.collapsed_tools.summary(&cells[start - 1]).is_some()
        {
            start -= 1;
        }
        Some(start)
    }

    pub(super) fn tool_group_ids(
        &self,
        cells: &[Arc<dyn HistoryCell>],
        index: usize,
    ) -> Option<Vec<String>> {
        if !self.groups_tools() || self.collapsed_tools.summary(cells.get(index)?).is_none() {
            return None;
        }
        let start = index;
        if index > 0
            && !self.pinned_tool_group(cells, index)
            && self.collapsed_tools.summary(&cells[index - 1]).is_some()
        {
            return Some(Vec::new());
        }
        Some(
            cells[start..]
                .iter()
                .enumerate()
                .take_while(|(offset, cell)| {
                    (*offset == 0 || !self.pinned_tool_group(cells, start + offset))
                        && self.collapsed_tools.summary(cell).is_some()
                })
                .flat_map(|(_, cell)| cell.activity_ids())
                .collect(),
        )
    }

    pub(super) fn tool_group_layout(
        &mut self,
        cells: &[Arc<dyn HistoryCell>],
        index: usize,
    ) -> Option<Arc<TextLayout>> {
        if !self.groups_tools() || self.collapsed_tools.summary(cells.get(index)?).is_none() {
            return None;
        }
        let width = self.area.width.max(/*other*/ 1);
        // Followers stay in canonical history for full transcripts, search, and pagination.
        if index > 0
            && !self.pinned_tool_group(cells, index)
            && self.collapsed_tools.summary(&cells[index - 1]).is_some()
        {
            return Some(Arc::new(TextLayout::new(Vec::new(), width)));
        }
        let key = (cells.as_ptr() as usize, cells.len(), index);
        if let Some(layout) = self.collapsed_tools.layouts.get(&key) {
            return Some(Arc::clone(layout));
        }
        let end = index
            + cells[index..]
                .iter()
                .enumerate()
                .take_while(|(offset, cell)| {
                    (*offset == 0 || !self.pinned_tool_group(cells, index + offset))
                        && self.collapsed_tools.summary(cell).is_some()
                })
                .count();
        let members = &cells[index..end];
        let ids = members
            .iter()
            .flat_map(|cell| cell.activity_ids())
            .collect::<Vec<_>>();
        let expanded = self.disclosure.is_expanded(&ids);
        if expanded {
            self.disclosure.expanded.extend(ids);
        }
        let mut summary = ToolCallSummary {
            count: 0,
            count_key: None,
            names: Vec::new(),
            running: false,
        };
        let mut counted = HashSet::new();
        for cell in members {
            if let Some(member) = self.collapsed_tools.summary(cell) {
                if member.count_key.is_none_or(|key| counted.insert(key)) {
                    summary.count += member.count;
                }
                summary.running |= member.running;
                for name in member.names {
                    if !summary.names.contains(&name) {
                        summary.names.push(name);
                    }
                }
            }
        }
        let mut lines = vec![summary_line(&summary, width, expanded)];
        if expanded {
            for cell in members {
                lines.extend(cell.compact_hyperlink_lines(width));
            }
        }
        let mut layout = TextLayout::new(lines, width);
        // The summary is itself the focus target; avoid an extra "Show details" row.
        layout.disclosure = true;
        if index > 0 {
            layout = layout.with_leading_separator();
        }
        let layout = Arc::new(layout);
        self.collapsed_tools
            .layouts
            .insert(key, Arc::clone(&layout));
        Some(layout)
    }
}

pub(crate) fn summary_line(summary: &ToolCallSummary, width: u16, expanded: bool) -> HyperlinkLine {
    let marker = if expanded { "▾" } else { "▸" };
    let verb = if summary.running { "Running" } else { "Ran" };
    let noun = if summary.count == 1 {
        "tool call"
    } else {
        "tool calls"
    };
    let header = format!("{marker} {verb} {} {noun}", summary.count);
    let mut visible = summary.names.len();
    let text = loop {
        let mut names = summary.names[..visible].join(", ");
        let omitted = summary.names.len() - visible;
        if omitted > 0 {
            if !names.is_empty() {
                names.push_str(", ");
            }
            names.push_str(&format!("+{omitted} more"));
        }
        let text = if names.is_empty() {
            header.clone()
        } else {
            format!("{header} ({names})")
        };
        if Line::from(text.clone()).width() <= usize::from(width) || visible == 0 {
            break text;
        }
        visible -= 1;
    };
    truncate_line_with_ellipsis_if_overflow(Line::from(text).dim(), usize::from(width)).into()
}

#[cfg(test)]
#[path = "tool_groups_tests.rs"]
pub(super) mod tests;

#[cfg(test)]
#[path = "tool_group_navigation_tests.rs"]
mod navigation_tests;
