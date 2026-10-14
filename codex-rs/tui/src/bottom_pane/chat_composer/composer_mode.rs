use ratatui::buffer::Buffer;
use ratatui::layout::Alignment;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Styled;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
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
        // Use the existing vertical padding; input geometry and wrapping stay unchanged.
        if area.height < 3 {
            return;
        }
        Block::default()
            .borders(Borders::TOP | Borders::BOTTOM)
            .border_style(self.style())
            .title(format!(" {} ", self.name()).set_style(self.style()))
            .title_alignment(Alignment::Right)
            .render(area, buf);
    }
}
