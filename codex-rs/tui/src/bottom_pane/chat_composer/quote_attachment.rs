//! Frozen transcript quotes share the composer attachment surface and submission pipeline.

use super::*;

const MAX_QUOTED_INPUT_BYTES: usize = 10_000;

pub(crate) enum PendingQuote {
    Text(String),
    TooLarge,
}

impl PendingQuote {
    pub(crate) fn from_selection(text: &str) -> Self {
        if text.len() > MAX_QUOTED_INPUT_BYTES {
            Self::TooLarge
        } else {
            Self::Text(text.to_owned())
        }
    }
}

impl ChatComposer {
    pub(crate) fn attach_quote(&mut self, quote: PendingQuote) {
        match quote {
            PendingQuote::Text(text) => {
                let mut quotes = self.attachments.quotes.clone();
                quotes.push(text);
                if Self::quote_prefix(&quotes, "").len() > MAX_QUOTED_INPUT_BYTES {
                    self.attach_quote(PendingQuote::TooLarge);
                    return;
                }
                self.history.reset_navigation();
                self.set_quotes(quotes);
            }
            PendingQuote::TooLarge => {
                self.show_footer_flash(
                    Line::from("Quote too long; limit 10,000 UTF-8 bytes".red()),
                    Duration::from_secs(5),
                );
            }
        }
    }

    pub(in crate::bottom_pane) fn set_quotes(&mut self, quotes: Vec<String>) {
        self.attachments.quotes = quotes;
        if !self.attachments.quotes.is_empty() && self.draft.is_bash_mode {
            let cursor = self.draft.textarea.cursor();
            self.draft.textarea.insert_str_at(/*pos*/ 0, "!");
            self.draft.textarea.set_cursor(cursor + 1);
            self.draft.is_bash_mode = false;
        }
        self.sync_popups();
    }

    pub(super) fn render_attachments(&self, area: Rect, buf: &mut Buffer) {
        self.attachments.attachment_area.set(area);
        if area.is_empty() {
            return;
        }
        let total = self.attachments.quotes.len() + self.attachments.remote_image_urls.len();
        let height = usize::from(area.height);
        let mut first = self
            .attachments
            .attachment_scroll
            .get()
            .min(total.saturating_sub(height));
        if let Some(selected) = self.attachments.selected_remote_image_index {
            let selected = selected + self.attachments.quotes.len();
            first = first.min(selected).max(selected.saturating_sub(height - 1));
        }
        self.attachments.attachment_scroll.set(first);
        let quotes = self.attachments.quotes.iter().map(|quote| {
            let preview = quote
                .split_whitespace()
                .take(/*n*/ 24)
                .collect::<Vec<_>>()
                .join(" ");
            Line::from(vec!["[×] Quote · ".cyan().bold(), preview.dim()])
        });
        Paragraph::new(
            quotes
                .chain(self.attachments.remote_image_lines())
                .skip(first)
                .take(height)
                .collect::<Vec<_>>(),
        )
        .style(user_message_style())
        .render(area, buf);
    }

    pub(crate) fn quote_prefix(quotes: &[String], comment: &str) -> String {
        let mut quoted = String::new();
        for (index, quote) in quotes.iter().enumerate() {
            if index > 0 {
                quoted.push('\n');
            }
            for line in quote.split('\n') {
                quoted.push_str("> ");
                quoted.push_str(line);
                quoted.push('\n');
            }
        }
        if !quotes.is_empty() && !comment.is_empty() {
            quoted.push('\n');
        }
        quoted
    }

    pub(super) fn prepend_quote(
        &mut self,
        text: &mut String,
        elements: &mut [TextElement],
    ) -> bool {
        if self.attachments.quotes.is_empty() {
            return true;
        };
        let mut quoted = Self::quote_prefix(&self.attachments.quotes, text);
        if quoted.len().saturating_add(text.len()) > MAX_QUOTED_INPUT_BYTES {
            return false;
        }
        for element in elements {
            element.byte_range.start += quoted.len();
            element.byte_range.end += quoted.len();
        }
        quoted.push_str(text);
        *text = quoted;
        true
    }
}
