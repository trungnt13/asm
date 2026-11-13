//! Measure and paint native and extension status lines within the caller's content rectangle.

use crate::line_truncation::truncate_line_with_ellipsis;
use crate::render::highlight::with_syntax_theme;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::ui_tree::PreparedUiTree;
use crate::ui_tree::UiTree;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::layout::Size;
use ratatui::style::Style;
use ratatui::text::Line;
use std::borrow::Cow;
use std::cell::Ref;
use std::cell::RefCell;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatusLineFocus {
    Focused,
    Unfocused,
}

/// A status line and the cached layout for its content.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StatusLine(StatusContent);

#[derive(Clone, Debug, PartialEq)]
enum StatusContent {
    Native(CachedLayout<Box<HyperlinkLine>, Option<Vec<HyperlinkLine>>>),
    Extension(CachedLayout<Arc<PreparedUiTree>, u16>),
}

#[derive(Debug)]
struct CachedLayout<C, L> {
    content: C,
    layout: RefCell<Option<(Size, L)>>,
}

impl<C, L> CachedLayout<C, L> {
    fn new(content: C) -> Self {
        Self {
            content,
            layout: RefCell::default(),
        }
    }

    fn get(&self, bounds: Size, compute: impl FnOnce(&C) -> L) -> Ref<'_, L> {
        if self
            .layout
            .borrow()
            .as_ref()
            .is_none_or(|(previous, _)| *previous != bounds)
        {
            self.layout.replace(Some((bounds, compute(&self.content))));
        }
        Ref::map(self.layout.borrow(), |entry| {
            let Some((_, layout)) = entry else {
                unreachable!("layout cache was populated");
            };
            layout
        })
    }
}

// Cached geometry belongs to the current surface. Clones and comparisons use only the content.
impl<C: Clone, L> Clone for CachedLayout<C, L> {
    fn clone(&self) -> Self {
        Self::new(self.content.clone())
    }
}

impl<C: PartialEq, L> PartialEq for CachedLayout<C, L> {
    fn eq(&self, other: &Self) -> bool {
        self.content == other.content
    }
}

impl From<UiTree> for StatusLine {
    fn from(tree: UiTree) -> Self {
        Self(StatusContent::Extension(CachedLayout::new(Arc::new(
            tree.prepare(),
        ))))
    }
}

impl From<HyperlinkLine> for StatusLine {
    fn from(line: HyperlinkLine) -> Self {
        Self(StatusContent::Native(CachedLayout::new(Box::new(line))))
    }
}

impl From<Line<'static>> for StatusLine {
    fn from(line: Line<'static>) -> Self {
        Self::from(HyperlinkLine::from(line))
    }
}

impl From<&'static str> for StatusLine {
    fn from(text: &'static str) -> Self {
        Self::from(Line::from(text))
    }
}

impl StatusLine {
    pub(crate) fn with_extension_suffix(&self, suffix: &str, separator: &str) -> Self {
        match &self.0 {
            StatusContent::Extension(extension) => {
                let separator = if extension.content.measure_natural().width > 0 {
                    separator
                } else {
                    ""
                };
                Self(StatusContent::Extension(CachedLayout::new(Arc::new(
                    extension
                        .content
                        .with_suffix(format!("{separator}{suffix}")),
                ))))
            }
            StatusContent::Native(_) => self.clone(),
        }
    }

    pub(crate) fn as_native(&self) -> Option<&HyperlinkLine> {
        match &self.0 {
            StatusContent::Native(native) => Some(&native.content),
            StatusContent::Extension(_) => None,
        }
    }

    /// Measure within the space available for content after the caller applies padding.
    pub(crate) fn height(&self, bounds: Size) -> u16 {
        if bounds.width == 0 || bounds.height == 0 {
            return 0;
        }
        match &self.0 {
            StatusContent::Native(native) => u16::from(native.content.width() > 0),
            StatusContent::Extension(extension) => {
                *extension.get(bounds, |tree| tree.measure(bounds).height)
            }
        }
    }

    /// Paint within the final content rectangle after the caller applies padding.
    pub(crate) fn render(&self, area: Rect, buf: &mut Buffer, focus: StatusLineFocus) {
        if area.is_empty() {
            return;
        }
        match &self.0 {
            StatusContent::Native(native) => {
                render_status_row(&native.content, Rect { height: 1, ..area }, buf, focus);
            }
            StatusContent::Extension(extension) => {
                let plain_style =
                    (focus == StatusLineFocus::Unfocused).then(|| Style::reset().dim());
                with_syntax_theme(|theme| extension.content.render(area, theme, buf, plain_style));
            }
        }
    }
}

fn render_status_row(line: &HyperlinkLine, area: Rect, buf: &mut Buffer, focus: StatusLineFocus) {
    let mut line = Cow::Borrowed(line);
    let width = usize::from(area.width);
    if line.width() > width {
        let line = line.to_mut();
        line.line = truncate_line_with_ellipsis(std::mem::take(&mut line.line), width);
        line.clip_hyperlinks_to_width(line.width().saturating_sub(1));
    }
    if focus == StatusLineFocus::Unfocused {
        let line = line.to_mut();
        line.hyperlinks.clear();
        line.line.style = Style::reset().dim();
        for span in &mut line.line.spans {
            span.style = Style::default();
        }
    }
    line.render_fitted(area, buf);
}

#[cfg(test)]
#[path = "status_line_tests.rs"]
mod tests;
