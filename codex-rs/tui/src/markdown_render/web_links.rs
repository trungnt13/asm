//! Styling and destination visibility for web links in semantic Markdown output.
//!
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use unicode_segmentation::UnicodeSegmentation;

use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::annotate_web_urls_in_line;
use crate::terminal_hyperlinks::web_destination;
use crate::width::display_width;

/// Give detected URLs the same affordance as explicit Markdown links, preserving surrounding text.
pub(super) fn style_bare_web_urls(span: Span<'static>, link_style: Style) -> HyperlinkLine {
    let mut line = annotate_web_urls_in_line(Line::from(span));
    if line.hyperlinks.is_empty() {
        return line;
    }
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut links = line.hyperlinks.iter().peekable();
    let mut column = 0;
    let span = &line.line.spans[0];
    for grapheme in span.content.graphemes(/*is_extended*/ true) {
        while links.peek().is_some_and(|link| link.columns.end <= column) {
            links.next();
        }
        let style = if links
            .peek()
            .is_some_and(|link| link.columns.contains(&column))
        {
            span.style.patch(link_style)
        } else {
            span.style
        };
        if let Some(last) = spans.last_mut()
            && last.style == style
        {
            last.content.to_mut().push_str(grapheme);
        } else {
            spans.push(Span::styled(grapheme.to_owned(), style));
        }
        column += display_width(grapheme);
    }
    line.line.spans = spans;
    line
}

pub(crate) fn hide_link_destination(destination: &str) -> bool {
    web_destination(destination).is_some() || super::local_links::is_markdown_file_link(destination)
}

#[cfg(test)]
#[path = "web_links_tests.rs"]
mod tests;
