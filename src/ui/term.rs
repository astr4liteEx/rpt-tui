//! Draws a run's vt100 screen into a ratatui buffer: cells copied, wide-character
//! continuation cells skipped, the child's default colours mapped to the theme's.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Widget,
};

pub struct TermView<'a> {
    pub screen: &'a vt100::Screen,
    /// What the child's default foreground and background become.
    pub fg: Color,
    pub bg: Color,
}

fn colour(c: vt100::Color, default: Color) -> Color {
    match c {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

impl Widget for TermView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for row in 0..area.height {
            for col in 0..area.width {
                let Some(cell) = self.screen.cell(row, col) else { continue };
                if cell.is_wide_continuation() {
                    // Hidden behind the wide character before it; backends skip it when drawing.
                    if let Some(out) = buf.cell_mut((area.x + col, area.y + row)) {
                        out.reset();
                    }
                    continue;
                }
                let mut style = Style::new().fg(colour(cell.fgcolor(), self.fg)).bg(colour(cell.bgcolor(), self.bg));
                for (on, m) in [
                    (cell.bold(), Modifier::BOLD),
                    (cell.dim(), Modifier::DIM),
                    (cell.italic(), Modifier::ITALIC),
                    (cell.underline(), Modifier::UNDERLINED),
                    (cell.inverse(), Modifier::REVERSED),
                ] {
                    if on {
                        style = style.add_modifier(m);
                    }
                }
                if let Some(out) = buf.cell_mut((area.x + col, area.y + row)) {
                    out.set_symbol(if cell.has_contents() { cell.contents() } else { " " }).set_style(style);
                }
            }
        }
    }
}
