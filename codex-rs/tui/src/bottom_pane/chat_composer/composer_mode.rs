use ratatui::buffer::Buffer;
use ratatui::layout::Alignment;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Widget;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ComposerMode {
    #[default]
    Main,
    Side,
    Chat,
    Parallel,
}

impl ComposerMode {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Main => "Main",
            Self::Side => "Side",
            Self::Chat => "Chat",
            Self::Parallel => "Parallel",
        }
    }

    pub(super) fn style(self) -> Style {
        match self {
            Self::Main => Style::default().dim(),
            Self::Side | Self::Chat | Self::Parallel => Style::default().magenta(),
        }
    }

    pub(super) fn render(self, area: Rect, buf: &mut Buffer) {
        // Use the existing top padding; input geometry and wrapping stay unchanged.
        let name = self.name();
        if area.height < 3 || area.width < name.len() as u16 {
            return;
        }
        Line::from(Span::styled(name, self.style()))
            .alignment(Alignment::Right)
            .render(Rect { height: 1, ..area }, buf);
    }
}
