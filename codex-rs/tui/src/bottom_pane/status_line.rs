//! Measure and paint status lines for the main conversation and side chats.

use super::footer::inset_footer_hint_area;
use super::footer_content::FooterContent;
use super::footer_content::StatusLineFocus;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::layout::Size;
use std::cell::Cell;

/// The owning composer resets this when its configured status changes.
#[derive(Default)]
pub(crate) struct StatusLine {
    measured_extension: Cell<Option<(Size, u16)>>,
}

impl StatusLine {
    pub(crate) fn height(
        &self,
        content: Option<&FooterContent>,
        width: u16,
        available: u16,
    ) -> u16 {
        let bounds = inset_footer_hint_area(Rect::new(0, 0, width, available)).as_size();
        if bounds.width == 0 || bounds.height == 0 {
            return 0;
        }
        match content {
            Some(FooterContent::Extension { content }) => {
                if let Some((previous, height)) = self.measured_extension.get()
                    && previous == bounds
                {
                    return height;
                }
                let height = content.measure(bounds).height;
                self.measured_extension.set(Some((bounds, height)));
                height
            }
            // ASM native status stays one logical row; extensions retain their height budget.
            Some(native) => u16::from(native.width() > 0),
            None => 0,
        }
    }

    pub(crate) fn render(
        &self,
        content: Option<&FooterContent>,
        area: Rect,
        buf: &mut Buffer,
        focus: StatusLineFocus,
    ) {
        let area = inset_footer_hint_area(area);
        let Some(content) = content.filter(|_| !area.is_empty()) else {
            return;
        };
        content.render_with_focus(area, buf, focus);
    }
}
