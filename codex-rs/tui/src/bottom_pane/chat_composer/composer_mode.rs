use crate::color::blend;
use crate::style::readable_color_on;
use crate::terminal_palette::best_color;
use crate::terminal_palette::default_bg;
use crate::terminal_palette::default_fg;
use ratatui::buffer::Buffer;
use ratatui::layout::Alignment;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Widget;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ComposerMode {
    #[default]
    Main,
    Side,
    Parallel,
}

impl ComposerMode {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Main => "Main",
            Self::Side => "Side",
            Self::Parallel => "Parallel",
        }
    }

    pub(super) fn style(self) -> Style {
        let background = match self {
            Self::Main => default_fg()
                .zip(default_bg())
                .map_or(Color::Reset, |(fg, bg)| {
                    best_color(blend(fg, bg, /*alpha*/ 0.22))
                }),
            Self::Side | Self::Parallel => best_color((175, 0, 175)),
        };
        if background == Color::Reset {
            // Terminal-owned palettes cannot be resolved to RGB; reverse their own colors.
            let foreground = match self {
                Self::Main => Color::Reset,
                Self::Side | Self::Parallel => Color::Magenta,
            };
            return Style::default()
                .fg(foreground)
                .bg(Color::Reset)
                .bold()
                .not_dim()
                .reversed();
        }
        let foreground = default_fg().map_or_else(|| best_color((255, 255, 255)), best_color);
        Style::default()
            .bg(background)
            .fg(readable_color_on(foreground, Some(background)))
            .bold()
            .not_dim()
            .not_reversed()
    }

    pub(super) fn render(self, area: Rect, buf: &mut Buffer) {
        // Use the existing top padding; input geometry and wrapping stay unchanged.
        let name = self.name();
        if area.height < 3 || area.width < (name.len() + 2) as u16 {
            return;
        }
        Line::from(Span::styled(format!(" {name} "), self.style()))
            .alignment(Alignment::Right)
            .render(Rect { height: 1, ..area }, buf);
    }
}
