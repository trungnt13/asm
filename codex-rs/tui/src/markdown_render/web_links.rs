//! Styling bare links and controlling web destination visibility in semantic Markdown output.
//!
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use unicode_segmentation::UnicodeSegmentation;

use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::TerminalHyperlink;
use crate::terminal_hyperlinks::annotate_web_urls_in_line;
use crate::terminal_hyperlinks::source_text_width;
use crate::terminal_hyperlinks::web_destination;
use crate::width::display_width;

/// Give detected links the same affordance as explicit Markdown links, preserving surrounding text.
pub(super) fn style_bare_links(
    span: Span<'static>,
    link_style: Style,
    is_hidden_destination: &dyn Fn(&str) -> bool,
    file_range_prefix: Option<&str>,
) -> HyperlinkLine {
    let mut line = annotate_web_urls_in_line(Line::from(span));
    let text = &line.line.spans[0].content;
    if let Some(prefix) = file_range_prefix {
        let mut previous_byte = 0;
        let mut previous_column = 0;
        for (range, file) in super::local_links::bare_file_ranges(text, prefix) {
            let token = &text[range.clone()];
            if !is_hidden_destination(token) {
                continue;
            }
            let start = previous_column + source_text_width(&text[previous_byte..range.start]);
            let end = start + source_text_width(token);
            previous_byte = range.end;
            previous_column = end;
            let mut link = TerminalHyperlink::document(start..end, file);
            link.markdown_source = Some(prefix.len() + range.start);
            line.hyperlinks.push(link);
        }
        line.hyperlinks.sort_by_key(|link| link.columns.start);
    }
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

pub(crate) fn hide_web_link_destination(destination: &str) -> bool {
    hide_link_destination(destination)
}

#[cfg(test)]
#[path = "web_links_tests.rs"]
mod tests;
