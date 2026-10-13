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
                self.history.reset_navigation();
                self.set_quote(Some(text));
            }
            PendingQuote::TooLarge => {
                self.show_footer_flash(
                    Line::from("Quote too long; limit 10,000 UTF-8 bytes".red()),
                    Duration::from_secs(5),
                );
            }
        }
    }

    pub(in crate::bottom_pane) fn set_quote(&mut self, quote: Option<String>) {
        self.attachments.quote = quote;
        if self.attachments.quote.is_some() && self.draft.is_bash_mode {
            let cursor = self.draft.textarea.cursor();
            self.draft.textarea.insert_str_at(/*pos*/ 0, "!");
            self.draft.textarea.set_cursor(cursor + 1);
            self.draft.is_bash_mode = false;
        }
        self.sync_popups();
    }

    pub(super) fn render_quote_attachment(&self, area: Rect, buf: &mut Buffer) -> Rect {
        self.attachments.quote_area.set(Rect::default());
        let Some(quote) = &self.attachments.quote else {
            return area;
        };
        if area.is_empty() {
            return area;
        }
        let row = Rect { height: 1, ..area };
        self.attachments.quote_area.set(row);
        let preview = quote
            .split_whitespace()
            .take(/*n*/ 24)
            .collect::<Vec<_>>()
            .join(" ");
        Paragraph::new(Line::from(vec![
            "[×] Quote · ".cyan().bold(),
            preview.dim(),
        ]))
        .style(user_message_style())
        .render(row, buf);
        Rect {
            y: area.y.saturating_add(/*rhs*/ 1),
            height: area.height - 1,
            ..area
        }
    }

    pub(crate) fn quote_prefix(quote: &str, comment: &str) -> String {
        let mut quoted = String::new();
        for line in quote.split('\n') {
            quoted.push_str("> ");
            quoted.push_str(line);
            quoted.push('\n');
        }
        if !comment.is_empty() {
            quoted.push('\n');
        }
        quoted
    }

    pub(super) fn prepend_quote(
        &mut self,
        text: &mut String,
        elements: &mut [TextElement],
    ) -> bool {
        let Some(quote) = &self.attachments.quote else {
            return true;
        };
        let mut quoted = Self::quote_prefix(quote, text);
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
