//! Semantic copy navigation over the selection's frozen transcript revision.
//! Targets retain their original payloads and leave surrounding context when revealed.

use super::*;
use crate::clipboard_copy::CopyFormat;
use crate::history_cell::AgentMarkdownCell;
use crate::history_cell::FinalMessageSeparator;
use crate::history_cell::InterruptionNotice;
use crate::history_cell::ProposedPlanCell;
use crate::history_cell::TurnActivityBoundary;
use crate::history_cell::UserHistoryCell;
use codex_protocol::models::MessagePhase;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use std::ops::Range;

pub(super) struct CopyMode {
    _guard: Option<Arc<crate::copy_input_guard::CopyInputGuard>>,
    turn: Option<Anchor>,
    response: Option<Anchor>,
    pub(super) awaiting_page: Option<KeyCode>,
    ungrouped: bool,
    whole: bool,
    source: Option<String>,
    pub(super) label: &'static str,
}

impl TranscriptView {
    pub(crate) fn begin_copy_mode(
        &mut self,
        cells: &[Arc<dyn HistoryCell>],
        guard: Option<Arc<crate::copy_input_guard::CopyInputGuard>>,
    ) -> bool {
        self.jump_to_latest();
        self.hold_position();
        self.copy_mode = Some(CopyMode {
            _guard: guard,
            turn: None,
            response: None,
            awaiting_page: None,
            ungrouped: false,
            whole: true,
            source: None,
            label: "Whole response",
        });
        self.move_copy_turn(cells, KeyCode::End);
        self.finish_copy_open(cells);
        self.copy_mode.is_some()
    }

    fn finish_copy_open(&mut self, cells: &[Arc<dyn HistoryCell>]) {
        let Some(mode) = &mut self.copy_mode else {
            return;
        };
        if self.selection.is_some() || mode.awaiting_page.is_some() {
            return;
        }
        // Preserve copying visible output when the conversation has no completed reply.
        mode.ungrouped = true;
        if let Some(index) = cells.iter().rposition(is_response) {
            mode.response = Some(cell_anchor(cells, index));
            self.select_copy_response(cells);
        } else {
            self.move_copy_target(cells, KeyCode::Left);
        }
        if self.selection.is_none() {
            self.jump_to_latest();
        }
    }

    pub(super) fn handle_copy_mode_key(
        &mut self,
        key: KeyEvent,
        cells: &[Arc<dyn HistoryCell>],
    ) -> Option<ViewAction> {
        let mode = self.copy_mode.as_mut()?;
        let code = match crate::key_hint::normalize_key_parts(key.code, key.modifiers) {
            (KeyCode::Char('k'), KeyModifiers::NONE) => KeyCode::Up,
            (KeyCode::Char('j'), KeyModifiers::NONE) => KeyCode::Down,
            (KeyCode::Char('g'), KeyModifiers::NONE) => KeyCode::Home,
            (KeyCode::Char('g'), KeyModifiers::SHIFT) => KeyCode::End,
            (KeyCode::Char('h'), KeyModifiers::NONE) => KeyCode::Left,
            (KeyCode::Char('l'), KeyModifiers::NONE) => KeyCode::Right,
            _ => key.code,
        };
        if self.selection.is_none() && mode.awaiting_page.is_some() && code != KeyCode::Esc {
            return Some(ViewAction::Changed);
        }
        match code {
            KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End if !mode.ungrouped => {
                self.move_copy_turn(cells, code)
            }
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End => {
                mode.awaiting_page = None;
                self.move_copy_target(cells, code);
            }
            KeyCode::Esc => self.jump_to_latest(),
            _ if key.code == KeyCode::Enter || crate::text_selection::is_copy_key(key) => {
                if self.selection.is_none() {
                    return Some(ViewAction::Changed);
                }
                mode.awaiting_page = None;
                return Some(ViewAction::CopyAndFollow(
                    self.selected_text(cells).unwrap_or_default(),
                ));
            }
            KeyCode::PageUp | KeyCode::PageDown => return self.handle_scroll_key(key, cells),
            _ => return Some(ViewAction::Changed),
        }
        Some(ViewAction::Changed)
    }

    fn move_copy_turn(&mut self, cells: &[Arc<dyn HistoryCell>], direction: KeyCode) {
        let snapshot = self.snapshot_cells();
        let cells = snapshot.as_deref().unwrap_or(cells);
        let Some(mode) = &self.copy_mode else { return };
        let current = mode.turn.map(|turn| self.resolve(cells, turn));
        let mut ends = match direction {
            KeyCode::Up => 0..current.unwrap_or(cells.len()),
            KeyCode::Down => current.map_or(0, |index| index + 1)..cells.len(),
            _ => 0..cells.len(),
        };
        let find_reply = |end| {
            let (range, answer_start) = copy_turn_range(cells, end)?;
            (answer_start..range.end)
                .rev()
                .find(|&index| is_whole_response(&cells[index]))
                .map(|index| (end, index))
        };
        let target = if matches!(direction, KeyCode::Up | KeyCode::End) {
            ends.rev().find_map(find_reply)
        } else {
            ends.find_map(find_reply)
        };
        let mode = self.copy_mode.as_mut().unwrap();
        mode.awaiting_page = (target.is_none()
            && matches!(direction, KeyCode::Up | KeyCode::End)
            && self.history.has_unloaded_history())
        .then_some(direction);
        if let Some((end, index)) = target {
            mode.turn = Some(cell_anchor(cells, end));
            mode.response = Some(cell_anchor(cells, index));
            self.select_copy_response(cells);
        }
    }

    pub(crate) fn resume_copy_navigation(&mut self, cells: &[Arc<dyn HistoryCell>]) {
        match self
            .copy_mode
            .as_mut()
            .and_then(|mode| mode.awaiting_page.take())
        {
            Some(KeyCode::Left) => self.move_copy_target(cells, KeyCode::Left),
            Some(direction) => {
                self.move_copy_turn(cells, direction);
                if direction == KeyCode::End {
                    self.finish_copy_open(cells);
                }
            }
            None => {}
        }
    }

    fn move_copy_target(&mut self, cells: &[Arc<dyn HistoryCell>], direction: KeyCode) {
        let snapshot = self.snapshot_cells();
        let cells = snapshot.as_deref().unwrap_or(cells);
        self.normalize_selection(cells);
        if direction == KeyCode::End && self.select_copy_response(cells) {
            return;
        }
        let Some(mode) = &self.copy_mode else { return };
        let label = mode.label;
        let whole = mode.whole;
        let turn = mode.turn;
        let edge = matches!(direction, KeyCode::Home | KeyCode::End);
        let older = matches!(direction, KeyCode::Left | KeyCode::Up | KeyCode::End);
        if whole && !older && !edge {
            return;
        }
        let current = self
            .selection
            .as_ref()
            .filter(|_| !whole && !edge)
            .map(|s| (s.start.index, s.start.offset..s.end.offset));
        let range = turn
            .and_then(|anchor| copy_turn_range(cells, self.resolve(cells, anchor)))
            .map_or(0..cells.len(), |(range, _)| range);
        let indices: Box<dyn Iterator<Item = usize>> = if older {
            Box::new(
                (range.start..current.as_ref().map_or(range.end, |(index, _)| index + 1)).rev(),
            )
        } else {
            Box::new(current.as_ref().map_or(range.start, |(index, _)| *index)..range.end)
        };
        for index in indices {
            let user = cells[index].as_any().downcast_ref::<UserHistoryCell>();
            if user.is_none() && !is_response(&cells[index]) {
                continue;
            }
            let Some(layout) = self.layout(cells, index) else {
                continue;
            };
            let literal = self.mode == HistoryRenderMode::Raw && !self.detailed
                || user.is_some_and(|user| !user.uses_markdown());
            let mut fenced = Vec::new();
            let mut ranges = if literal {
                literal_block_ranges(layout.text(), user, &mut fenced)
            } else {
                fenced = layout.fenced_offsets();
                layout.copy_block_ranges()
            };
            let user_source = user
                .map(|user| user.message.as_str())
                .filter(|source| !source.is_empty())
                .map(|source| crate::history_cell::sanitize_user_text(source.into()));
            if user.is_some()
                && (!layout.text().trim().is_empty()
                    || user_source
                        .as_ref()
                        .is_some_and(|source| !source.trim().is_empty()))
            {
                ranges.insert(
                    0,
                    (
                        0..layout.text().len(),
                        "User message",
                        user_source.map(std::borrow::Cow::into_owned),
                    ),
                );
            }
            ranges.sort_by_key(|(range, _, _)| (range.start, std::cmp::Reverse(range.end)));
            if let Some(source) = user
                .map(|user| user.message.as_str())
                .or_else(|| cells[index].copy_source())
            {
                let targets = crate::markdown::extract_copy_targets(source);
                let mut codes = targets
                    .iter()
                    .filter_map(|target| match target {
                        crate::markdown::CopyTarget::Code { content, .. } => {
                            Some(Arc::clone(content))
                        }
                        crate::markdown::CopyTarget::Quote(_) => None,
                    })
                    .collect::<Vec<_>>();
                let mut depth = 0;
                let mut quotes = Vec::new();
                for (event, span) in pulldown_cmark::Parser::new(source).into_offset_iter() {
                    match event {
                        pulldown_cmark::Event::Start(pulldown_cmark::Tag::BlockQuote) => {
                            if depth == 0 {
                                let raw = &source[span];
                                quotes.push((raw, crate::markdown_copy::quote_content(raw)));
                            }
                            depth += 1;
                        }
                        pulldown_cmark::Event::End(pulldown_cmark::TagEnd::BlockQuote) => {
                            depth -= 1
                        }
                        _ => {}
                    }
                }
                let normalized = normalized;
                for (range, label, payload) in &mut ranges {
                    if *label == "Code block" && TextLayout::is_fenced_range(&fenced, range) {
                        // Match rendered text, not ordinal position: indented code and
                        // source-preserving tables need not have a fenced counterpart.
                        let displayed = payload.as_deref().unwrap_or(&layout.text()[range.clone()]);
                        if let Some(code) = take_matching_code(&mut codes, displayed) {
                            *payload = Some(code.to_string());
                        }
                    } else if *label == "Blockquote"
                        && let Some(displayed) = payload.as_deref()
                        && let Some(at) = quotes.iter().position(|(_, quote)| {
                            if normalized(quote) == normalized(displayed) {
                                return true;
                            }
                            if user.is_some() {
                                return false;
                            }
                            let visible = quote
                                .split_inclusive('\n')
                                .map(|line| {
                                    crate::git_action_directives::strip_line_directives(line).0
                                })
                                .collect::<String>();
                            normalized(&visible) == normalized(displayed)
                        })
                    {
                        let (raw, quote) = quotes.remove(at);
                        *payload = Some(if user.is_some() {
                            crate::history_cell::sanitize_user_text(raw.into()).into_owned()
                        } else {
                            quote
                                .split_inclusive('\n')
                                .map(|line| {
                                    crate::git_action_directives::strip_line_directives(line).0
                                })
                                .collect()
                        });
                    }
                }
            }
            let boundary =
                current
                    .as_ref()
                    .filter(|(at, _)| *at == index)
                    .and_then(|(_, range)| {
                        ranges.iter().position(|(candidate, candidate_label, _)| {
                            candidate == range && *candidate_label == label
                        })
                    });
            let target = if older {
                ranges[..boundary.unwrap_or(ranges.len())].last()
            } else {
                ranges.get(boundary.map_or(0, |at| at + 1))
            };
            if let Some((range, label, source)) = target {
                let range = range.clone();
                if let Some(mode) = &mut self.copy_mode {
                    mode.whole = false;
                    mode.label = label;
                    mode.source = source.clone();
                }
                self.select_copy_range(cells, index, range);
                return;
            }
        }
        if !older {
            self.select_copy_response(cells);
        } else if range.start == 0 && self.history.has_unloaded_history() {
            if turn.is_some() {
                self.copy_mode.as_mut().unwrap().awaiting_page = Some(KeyCode::Left);
            } else {
                self.jump_to_entry(cells, /*index*/ 0);
            }
        }
    }

    /// Keep fitting targets inside a central band, scrolling only to the nearest margin.
    /// Oversized targets show their beginning with context above rather than clipping it.
    pub(super) fn reveal_copy_context(&mut self, cells: &[Arc<dyn HistoryCell>]) {
        if self.copy_mode.is_none() {
            return;
        }
        let Some(selection) = &self.selection else {
            return;
        };
        let (start, end) = (selection.start, selection.end);
        let index = self.resolve(cells, start);
        let Some(layout) = self.layout(cells, index) else {
            return;
        };
        let first = layout.row_for_offset(start.offset);
        // Match block painting: whole-entry highlights include final padding.
        let finish = if start.offset == 0 && end.offset == layout.text().len() {
            end.offset
        } else {
            end.offset.saturating_sub(/*rhs*/ 1)
        };
        let last = layout.row_for_offset(finish);
        let height = usize::from(self.area.height);
        if height == 0 {
            return;
        }
        let target_height = last.saturating_sub(first) + 1;
        // Reserve a quarter of the viewport, reducing the margin for taller targets.
        let margin = (height / 4)
            .max(1)
            .min(height.saturating_sub(target_height) / 2);
        let bottom_margin = (height / 4)
            .max(1)
            .min(height.saturating_sub(target_height).div_ceil(2));
        let top = self.start(cells);
        let inner_top = self.move_rows(cells, top.0, top.1, margin as isize);
        let inner_bottom = self.move_rows(
            cells,
            top.0,
            top.1,
            height.saturating_sub(bottom_margin + 1) as isize,
        );
        if target_height < height && (index, first) >= inner_top && (index, last) <= inner_bottom {
            return;
        }
        let inset = if target_height >= height {
            height / 4
        } else if (index, first) < inner_top {
            margin
        } else {
            height - bottom_margin - target_height
        };
        let (index, row) = self.move_rows(cells, index, first, -(inset as isize));
        if let Some(layout) = self.layout(cells, index) {
            let offset = layout.position_at(row, /*column*/ 0);
            self.position = Position::Reading(Anchor {
                key: self.entry_key(cells, index),
                index,
                offset,
                row_bias: layout.row_for_offset(offset) as isize - row as isize,
            });
        }
    }

    fn select_copy_response(&mut self, cells: &[Arc<dyn HistoryCell>]) -> bool {
        let Some(response) = self.copy_mode.as_ref().and_then(|mode| mode.response) else {
            return false;
        };
        let index = self.resolve(cells, response);
        let Some(layout) = self.layout(cells, index) else {
            return false;
        };
        let mode = self.copy_mode.as_mut().unwrap();
        mode.whole = true;
        mode.source = None;
        mode.label = "Whole response";
        self.select_copy_range(cells, index, 0..layout.text().len());
        true
    }

    pub(super) fn copy_mode_payload(
        &self,
        cells: &[Arc<dyn HistoryCell>],
    ) -> Option<(String, CopyFormat)> {
        let mode = self.copy_mode.as_ref()?;
        if !mode.whole {
            return mode
                .source
                .clone()
                .map(|text| (text, CopyFormat::PlainText));
        }
        let snapshot = self.snapshot_cells();
        let cells = snapshot.as_deref().unwrap_or(cells);
        let cell = cells.get(self.resolve(cells, mode.response?))?;
        let text = if cell.as_any().is::<AgentMarkdownCell>() {
            // The visible source excludes hidden directives.
            let markdown = cell
                .raw_lines()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            crate::markdown_render::followup_labels(&markdown).into_owned()
        } else {
            cell.copy_source()?.to_owned()
        };
        Some((text, CopyFormat::Markdown))
    }
}

pub(super) fn take_matching_code(codes: &mut Vec<Arc<str>>, displayed: &str) -> Option<Arc<str>> {
    let displayed = normalized(displayed);
    let at = codes
        .iter()
        .position(|code| normalized(code) == displayed)?;
    Some(codes.remove(at))
}

pub(super) fn normalized(text: &str) -> String {
    crate::markdown::normalize_markdown_for_rendering(text)
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

fn cell_anchor(cells: &[Arc<dyn HistoryCell>], index: usize) -> Anchor {
    Anchor {
        key: EntryKey::cell(&cells[index]),
        index,
        offset: 0,
        row_bias: 0,
    }
}

fn copy_turn_range(cells: &[Arc<dyn HistoryCell>], end: usize) -> Option<(Range<usize>, usize)> {
    let turn_id = cells[end]
        .as_any()
        .downcast_ref::<FinalMessageSeparator>()?
        .turn_id
        .as_deref()?;
    let mut start = end;
    let mut begin = None;
    while start > 0 {
        let index = start - 1;
        let cell = cells[index].as_any();
        let boundary = cell.downcast_ref::<TurnActivityBoundary>();
        if cell.is::<FinalMessageSeparator>()
            || cell.is::<InterruptionNotice>()
            || matches!(boundary, Some(TurnActivityBoundary::UnsuccessfulEnd))
        {
            break;
        }
        if let Some(TurnActivityBoundary::Begin(id)) = boundary {
            if turn_id != id {
                start = begin.unwrap_or(start);
                break;
            }
            begin = Some(index);
        }
        start = index;
    }
    Some((start..end, begin.unwrap_or(start)))
}

fn is_whole_response(cell: &Arc<dyn HistoryCell>) -> bool {
    cell.as_any().is::<ProposedPlanCell>()
        || cell
            .as_any()
            .downcast_ref::<AgentMarkdownCell>()
            .is_some_and(|message| matches!(message.phase, None | Some(MessagePhase::FinalAnswer)))
}

fn is_response(cell: &Arc<dyn HistoryCell>) -> bool {
    cell.as_any().is::<AgentMarkdownCell>() || cell.as_any().is::<ProposedPlanCell>()
}

fn literal_block_ranges(
    text: &str,
    user: Option<&UserHistoryCell>,
    fenced: &mut Vec<usize>,
) -> Vec<(Range<usize>, &'static str, Option<String>)> {
    use pulldown_cmark::Event;
    use pulldown_cmark::Tag;
    use pulldown_cmark::TagEnd;
    let mut ranges = Vec::new();
    let mut depth = 0;
    let mut code = None;
    for (event, range) in pulldown_cmark::Parser::new(text).into_offset_iter() {
        match event {
            Event::Start(Tag::BlockQuote) => {
                if depth == 0 {
                    let source = user
                        .is_none_or(|user| user.message.is_empty())
                        .then(|| crate::markdown_copy::quote_content(&text[range.clone()]));
                    ranges.push((range, "Blockquote", source));
                }
                depth += 1;
            }
            Event::End(TagEnd::BlockQuote) => depth -= 1,
            Event::Start(Tag::CodeBlock(kind)) => {
                code = Some((range.start..range.start, String::new(), kind));
            }
            Event::Text(text) => {
                if let Some((source, content, _)) = code.as_mut() {
                    content.push_str(&text);
                    if source.start == source.end {
                        source.start = range.start;
                    }
                    source.end = range.end;
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((range, content, kind)) =
                    code.take().filter(|(range, _, _)| !range.is_empty())
                {
                    if matches!(kind, pulldown_cmark::CodeBlockKind::Fenced(_)) {
                        fenced.push(range.start);
                    }
                    ranges.push((range, "Code block", Some(content)));
                }
            }
            _ => {}
        }
    }
    ranges
}

#[cfg(test)]
#[path = "copy_mode_tests.rs"]
mod tests;
