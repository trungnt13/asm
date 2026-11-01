//! Presentation-only turn folds; canonical cells stay individually addressable by copy and search.

use super::*;
use crate::history_cell::TurnActivityBoundary;
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct TurnGroups {
    first: Option<EntryKey>,
    last: Option<EntryKey>,
    members: Vec<Option<usize>>,
    pending: Option<(String, usize)>,
    groups: Vec<TurnGroup>,
    by_turn: HashMap<String, usize>,
    legacy_answer: Option<(String, usize)>,
    layouts: HashMap<(EntryKey, u16, bool), Arc<TextLayout>>,
    empty_layout: Option<Arc<TextLayout>>,
}

struct TurnGroup {
    id: String,
    leader: usize,
    label: String,
}

impl TurnGroups {
    pub(super) fn clear_layouts(&mut self) {
        self.layouts.clear();
    }

    fn sync(&mut self, cells: &[Arc<dyn HistoryCell>]) {
        let count = self.members.len();
        if count > cells.len()
            || self.first != cells.first().map(EntryKey::cell)
            || (count > 0 && self.last != cells.get(count - 1).map(EntryKey::cell))
        {
            *self = Self::default();
        }
        let start = self.members.len();
        self.members.resize(cells.len(), /*value*/ None);
        for index in start..cells.len() {
            let cell = &cells[index];
            match cell.as_any().downcast_ref::<TurnActivityBoundary>() {
                Some(TurnActivityBoundary::Begin(id)) => {
                    if self
                        .pending
                        .as_ref()
                        .is_none_or(|(pending, _)| pending != id)
                    {
                        self.pending = Some((id.clone(), index + 1));
                    }
                }
                Some(TurnActivityBoundary::LegacyAnswer { turn_id }) => {
                    if let Some((id, begin)) = &self.pending
                        && turn_id.as_ref().is_none_or(|turn_id| turn_id == id)
                        && let Some(answer) = (*begin..index).rev().find(|&index| {
                            cells[index]
                                .as_any()
                                .is::<crate::history_cell::AgentMarkdownCell>()
                        })
                    {
                        self.legacy_answer = Some((id.clone(), answer));
                    }
                }
                Some(TurnActivityBoundary::FinalAnswer {
                    turn_id,
                    elapsed_seconds,
                }) => {
                    if let Some((id, begin)) = self.pending.take()
                        && turn_id.as_ref().is_none_or(|turn_id| turn_id == &id)
                    {
                        let completion = crate::history_cell::FinalMessageSeparator::new(
                            *elapsed_seconds,
                            /*runtime_metrics*/ None,
                        );
                        self.fold(cells, begin..index, &id, &completion);
                    }
                }
                None => {
                    if let Some(completion) =
                        cell.as_any()
                            .downcast_ref::<crate::history_cell::FinalMessageSeparator>()
                        && let Some((_, turn_id)) = completion.session_turn()
                    {
                        let id = format!("turn-activity:{turn_id}");
                        if let Some(group) = self.by_turn.get(&id).copied() {
                            self.groups[group].label = completion
                                .raw_lines()
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(" ");
                            self.members[index] = Some(group);
                            self.layouts.clear();
                        } else if let Some((pending_id, begin)) = self.pending.take()
                            && pending_id == turn_id
                            && let Some((answer_turn, answer)) = self.legacy_answer.take()
                            && answer_turn == turn_id
                        {
                            self.fold(cells, begin..answer, turn_id, completion);
                            if self.groups.last().is_some_and(|group| group.id == id) {
                                self.members[index] = Some(self.groups.len() - 1);
                            }
                        }
                    }
                }
            }
        }
        self.first = cells.first().map(EntryKey::cell);
        self.last = cells.last().map(EntryKey::cell);
    }

    fn fold(
        &mut self,
        cells: &[Arc<dyn HistoryCell>],
        range: std::ops::Range<usize>,
        turn_id: &str,
        completion: &crate::history_cell::FinalMessageSeparator,
    ) {
        let mut leader = None;
        for index in range {
            let cell = &cells[index];
            if cell.as_any().is::<crate::history_cell::AgentMarkdownCell>()
                || cell.as_any().is::<crate::history_cell::AgentMessageCell>()
                || cell
                    .as_any()
                    .is::<crate::history_cell::ReasoningSummaryCell>()
                || cell.tool_call_summary().is_some()
            {
                leader.get_or_insert(index);
                self.members[index] = Some(self.groups.len());
            }
        }
        if let Some(leader) = leader {
            self.by_turn
                .insert(format!("turn-activity:{turn_id}"), self.groups.len());
            self.groups.push(TurnGroup {
                id: format!("turn-activity:{turn_id}"),
                leader,
                label: completion
                    .raw_lines()
                    .first()
                    .map_or_else(|| "Worked".to_owned(), ToString::to_string),
            });
        }
    }
}

impl TranscriptView {
    pub(super) fn turn_group_ids(
        &self,
        cells: &[Arc<dyn HistoryCell>],
        index: usize,
    ) -> Option<Vec<String>> {
        if self.detailed || self.mode != HistoryRenderMode::Rich {
            return None;
        }
        let mut turns = self.turn_groups.borrow_mut();
        turns.sync(cells);
        let group = turns.groups.get(*turns.members.get(index)?.as_ref()?)?;
        Some(if index == group.leader {
            vec![group.id.clone()]
        } else {
            Vec::new()
        })
    }

    pub(super) fn turn_group_layout(
        &self,
        cells: &[Arc<dyn HistoryCell>],
        index: usize,
    ) -> Option<Arc<TextLayout>> {
        if self.detailed || self.mode != HistoryRenderMode::Rich {
            return None;
        }
        let mut turns = self.turn_groups.borrow_mut();
        turns.sync(cells);
        let group = turns.groups.get(*turns.members.get(index)?.as_ref()?)?;
        let expanded = self.disclosure.expanded.contains(&group.id);
        let leader = index == group.leader;
        let width = self.area.width.max(/*other*/ 1);
        let cell = cells.get(index)?;
        if !leader
            && (!expanded
                || cell
                    .as_any()
                    .is::<crate::history_cell::FinalMessageSeparator>())
        {
            let layout = turns
                .empty_layout
                .get_or_insert_with(|| Arc::new(TextLayout::new(Vec::new(), width)));
            return Some(Arc::clone(layout));
        }
        let key = (EntryKey::cell(cell), width, expanded);
        if let Some(layout) = turns.layouts.get(&key) {
            return Some(Arc::clone(layout));
        }
        let lines = if expanded
            && !cell
                .as_any()
                .is::<crate::history_cell::FinalMessageSeparator>()
        {
            cell.compact_hyperlink_lines(width)
        } else {
            Vec::new()
        };
        let empty = lines.is_empty();
        let mut layout = TextLayout::new(lines, width);
        if leader {
            let marker = if expanded { "▾" } else { "▸" };
            let label = format!("{marker} {}", group.label);
            if empty {
                layout = TextLayout::new(
                    crate::terminal_hyperlinks::plain_hyperlink_lines(
                        crate::wrapping::word_wrap_lines(
                            [Line::from(label).dim()],
                            usize::from(width),
                        ),
                    ),
                    width,
                );
                layout.disclosure = true;
            } else {
                layout = layout.with_disclosure_control_at(label, /*source_offset*/ 0);
            }
        }
        if index > 0 {
            layout = layout.with_leading_separator();
        }
        let layout = Arc::new(layout);
        if turns.layouts.len() >= 64 {
            turns.layouts.clear();
        }
        turns.layouts.insert(key, Arc::clone(&layout));
        Some(layout)
    }
}
