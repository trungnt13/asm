//! Generated comment prefixes retain quote identity while ordinary text remains freely editable.

use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QuoteComment {
    pub(crate) quote_index: usize,
    pub(crate) range: Range<usize>,
}

impl QuoteComment {
    fn empty_line(&self, text: &str, next_prefix: Option<usize>) -> Option<Range<usize>> {
        let end = next_prefix.unwrap_or(text.len());
        if !text.get(self.range.end..end)?.trim().is_empty()
            || (self.range.start > 0 && text.as_bytes()[self.range.start - 1] != b'\n')
        {
            return None;
        }
        Some(if next_prefix.is_none() && self.range.start > 0 {
            self.range.start - 1..end
        } else {
            self.range.start..end
        })
    }
}

impl ChatComposer {
    pub(super) fn quote_comments(&self) -> Vec<QuoteComment> {
        self.draft
            .textarea
            .text_element_snapshots()
            .into_iter()
            .filter_map(|element| {
                self.draft
                    .quote_comments
                    .get(&element.id)
                    .map(|&quote_index| QuoteComment {
                        quote_index,
                        range: element.range,
                    })
            })
            .collect()
    }

    pub(crate) fn set_quote_comments(&mut self, comments: Vec<QuoteComment>) {
        self.draft.quote_comments.clear();
        for comment in comments.into_iter().rev() {
            if comment.quote_index >= self.attachments.quotes.len() {
                continue;
            }
            let Some(id) = self
                .draft
                .textarea
                .element_id_for_exact_range(comment.range.clone())
                .or_else(|| self.draft.textarea.add_element_range(comment.range.clone()))
            else {
                continue;
            };
            let prefix = format!("{}. ", comment.quote_index + 1);
            if self.draft.textarea.text().get(comment.range.clone()) == Some(prefix.as_str()) {
                self.draft.quote_comments.insert(id, comment.quote_index);
            } else {
                self.draft
                    .textarea
                    .replace_range(comment.range.clone(), &prefix);
                if let Some(id) = self
                    .draft
                    .textarea
                    .add_element_range(comment.range.start..comment.range.start + prefix.len())
                {
                    self.draft.quote_comments.insert(id, comment.quote_index);
                }
            }
        }
    }

    pub(super) fn append_quote_comment(&mut self) {
        self.draft.textarea.enter_vim_insert_mode();
        let comments = self.quote_comments();
        let empty_lines = comments
            .iter()
            .enumerate()
            .filter_map(|(index, comment)| {
                comment.empty_line(
                    self.draft.textarea.text(),
                    comments.get(index + 1).map(|next| next.range.start),
                )
            })
            .collect::<Vec<_>>();
        for mut range in empty_lines.into_iter().rev() {
            range.end = range.end.min(self.draft.textarea.text().len());
            self.draft.textarea.replace_range(range, "");
        }
        let live_ids = self
            .draft
            .textarea
            .text_element_snapshots()
            .into_iter()
            .map(|element| element.id)
            .collect::<std::collections::HashSet<_>>();
        self.draft
            .quote_comments
            .retain(|id, _| live_ids.contains(id));
        let index = self.attachments.quotes.len() - 1;
        if index == 0 {
            self.draft.textarea.set_cursor(/*pos*/ 0);
        } else {
            self.move_cursor_to_end();
            if !self.draft.textarea.text().is_empty() && !self.draft.textarea.text().ends_with('\n')
            {
                self.draft.textarea.insert_str("\n");
            }
        }
        let id = self
            .draft
            .textarea
            .insert_element(&format!("{}. ", index + 1));
        self.draft.quote_comments.insert(id, index);
        self.move_cursor_to_end();
        self.draft.textarea_state.get_mut().follow_cursor();
    }

    pub(super) fn remove_quote_comment(&mut self, index: usize) {
        let comments = self.quote_comments();
        let empty_lines = comments
            .iter()
            .enumerate()
            .map(|(position, comment)| {
                comment.empty_line(
                    self.draft.textarea.text(),
                    comments.get(position + 1).map(|next| next.range.start),
                )
            })
            .collect::<Vec<_>>();
        for (comment, empty_line) in comments.iter().zip(empty_lines).rev() {
            if comment.quote_index == index {
                let range = empty_line.unwrap_or(comment.range.clone());
                self.draft.textarea.replace_range(range, "");
            } else if comment.quote_index > index {
                let prefix = format!("{}. ", comment.quote_index);
                self.draft
                    .textarea
                    .replace_range(comment.range.clone(), &prefix);
                if let Some(id) = self
                    .draft
                    .textarea
                    .add_element_range(comment.range.start..comment.range.start + prefix.len())
                {
                    self.draft
                        .quote_comments
                        .insert(id, comment.quote_index - 1);
                }
            }
        }
    }

    pub(super) fn active_empty_quote_comment(&self) -> Option<Range<usize>> {
        let comments = self.quote_comments();
        let text = self.draft.textarea.text();
        let cursor = self.draft.textarea.cursor();
        comments.iter().enumerate().find_map(|(index, comment)| {
            let line_end = comment.range.end
                + text[comment.range.end..]
                    .find('\n')
                    .unwrap_or(text.len() - comment.range.end);
            (cursor >= comment.range.end
                && cursor <= line_end
                && comment
                    .empty_line(text, comments.get(index + 1).map(|next| next.range.start))
                    .is_some())
            .then_some(comment.range.start..line_end)
        })
    }

    pub(super) fn leave_empty_quote_comment(&mut self) -> bool {
        let Some(range) = self.active_empty_quote_comment() else {
            return false;
        };
        let started_vim_edit = self.begin_direct_vim_edit();
        if let Some(element) = self
            .draft
            .textarea
            .text_element_snapshots()
            .into_iter()
            .find(|element| element.range.start == range.start)
        {
            self.draft.quote_comments.remove(&element.id);
        }
        self.draft.textarea.replace_range(range.clone(), "");
        self.draft.textarea.set_cursor(range.start);
        if started_vim_edit {
            self.finish_vim_edit();
        }
        true
    }

    pub(crate) fn prepare_quote_comments(
        text: &mut String,
        elements: &mut Vec<TextElement>,
        comments: &[QuoteComment],
    ) {
        elements.retain(|element| {
            !comments.iter().any(|comment| {
                element.byte_range.start == comment.range.start
                    && element.byte_range.end == comment.range.end
            })
        });
        let empty_lines = comments
            .iter()
            .enumerate()
            .map(|(index, comment)| {
                comment.empty_line(text, comments.get(index + 1).map(|next| next.range.start))
            })
            .collect::<Vec<_>>();
        for (comment, empty_line) in comments.iter().zip(empty_lines).rev() {
            if let Some(mut range) = empty_line {
                range.end = range.end.min(text.len());
                text.replace_range(range.clone(), "");
                for element in elements.iter_mut() {
                    if element.byte_range.start >= range.end {
                        element.byte_range.start -= range.len();
                        element.byte_range.end -= range.len();
                    }
                }
            } else {
                let dot = comment.range.end - 2;
                text.insert(dot, '\\');
                for element in elements.iter_mut() {
                    if element.byte_range.start >= dot {
                        element.byte_range.start += 1;
                        element.byte_range.end += 1;
                    }
                }
            }
        }
    }
}
