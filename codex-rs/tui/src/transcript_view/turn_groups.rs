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
    hidden: Vec<(std::ops::Range<usize>, usize)>,
    layout_dirty: bool,
    empty_layout: Option<Arc<TextLayout>>,
}

struct TurnGroup {
    id: String,
    leader: usize,
    label: String,
}

impl TurnGroups {
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
                    if self.pending.as_ref().is_none_or(|(pending, begin)| {
                        pending != id
                            || !cells[*begin..index]
                                .iter()
                                .any(|cell| TurnActivityBoundary::is_activity(cell.as_ref()))
                    }) {
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
                            self.layout_dirty = true;
                            self.hide(index, group);
                        } else if let Some((pending_id, begin)) = self.pending.take()
                            && pending_id == turn_id
                            && let Some((answer_turn, answer)) = self.legacy_answer.take()
                            && answer_turn == turn_id
                        {
                            self.fold(cells, begin..answer, turn_id, completion);
                            if self.groups.last().is_some_and(|group| group.id == id) {
                                let group = self.groups.len() - 1;
                                self.members[index] = Some(group);
                                self.hide(index, group);
                            }
                        }
                    }
                }
            }
        }
        self.first = cells.first().map(EntryKey::cell);
        self.last = cells.last().map(EntryKey::cell);
    }

    fn hide(&mut self, index: usize, group: usize) {
        if let Some((range, previous_group)) = self.hidden.last_mut()
            && range.end == index
            && *previous_group == group
        {
            range.end += 1;
        } else {
            self.hidden.push((index..index + 1, group));
        }
    }

    fn fold(
        &mut self,
        cells: &[Arc<dyn HistoryCell>],
        range: std::ops::Range<usize>,
        turn_id: &str,
        completion: &crate::history_cell::FinalMessageSeparator,
    ) {
        let leader = range.start - 1;
        let mut has_activity = false;
        for index in range {
            if TurnActivityBoundary::is_activity(cells[index].as_ref()) {
                has_activity = true;
                self.hide(index, self.groups.len());
                self.members[index] = Some(self.groups.len());
            }
        }
        if has_activity {
            self.members[leader] = Some(self.groups.len());
            self.layout_dirty = true;
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
        if index == group.leader {
            Some(vec![group.id.clone()])
        } else if self.disclosure.expanded.contains(&group.id) {
            None
        } else {
            Some(Vec::new())
        }
    }

    pub(super) fn turn_group_layout(
        &mut self,
        cells: &[Arc<dyn HistoryCell>],
        index: usize,
    ) -> Option<Arc<TextLayout>> {
        if self.detailed || self.mode != HistoryRenderMode::Rich {
            return None;
        }
        let mut turns = self.turn_groups.borrow_mut();
        turns.sync(cells);
        if std::mem::take(&mut turns.layout_dirty) {
            self.cache.clear();
        }
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
        if !leader {
            return None;
        }
        let label = group.label.clone();
        drop(turns);
        Some(
            self.cache
                .turn_group(cell, width, expanded, /*separated*/ index > 0, || {
                    let marker = if expanded { "▾" } else { "▸" };
                    TextLayout::new(Vec::new(), width)
                        .with_disclosure_header(format!("{marker} {label}"))
                }),
        )
    }

    pub(super) fn skip_collapsed_turn(
        &self,
        cells: &[Arc<dyn HistoryCell>],
        index: usize,
        direction: isize,
    ) -> usize {
        if self.detailed
            || self.mode != HistoryRenderMode::Rich
            || self.snapshot().is_some()
            || self.search.has_active_query()
        {
            return index;
        }
        let mut turns = self.turn_groups.borrow_mut();
        turns.sync(cells);
        let next = turns
            .hidden
            .partition_point(|(range, _)| range.start <= index);
        let Some((range, group)) = next
            .checked_sub(1)
            .and_then(|previous| turns.hidden.get(previous))
        else {
            return index;
        };
        if range.contains(&index) && !self.disclosure.expanded.contains(&turns.groups[*group].id) {
            if direction < 0 {
                range.start.saturating_sub(1)
            } else {
                range.end
            }
        } else {
            index
        }
    }
}
