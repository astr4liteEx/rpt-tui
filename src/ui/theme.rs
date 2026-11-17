//! Colours. Only the built-in `charm` theme for now; theme config comes with the Themes ticket.

use ratatui::style::Color;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub bg: Color,
    pub surface: Color,
    pub text: Color,
    pub muted: Color,
    pub border: Color,
    pub border_focus: Color,
    pub border_run: Color,
    pub accent: Color,
    pub ok: Color,
    pub warn: Color,
    pub err: Color,
    /// Severity colours by position, most severe first; the last repeats.
    pub severity_scale: [Color; 5],
}

const fn hex(h: u32) -> Color {
    Color::Rgb((h >> 16) as u8, (h >> 8 & 0xff) as u8, (h & 0xff) as u8)
}

impl Theme {
    pub const CHARM: Theme = Theme {
        bg: hex(0x1a1a24),
        surface: hex(0x2a2a3a),
        text: hex(0xdddddd),
        muted: hex(0x7a7a8c),
        border: hex(0x44445a),
        border_focus: hex(0x7d56f4),
        border_run: hex(0xf25d94),
        accent: hex(0x7d56f4),
        ok: hex(0x04b575),
        warn: hex(0xffd75f),
        err: hex(0xff5f87),
        severity_scale: [hex(0xff5f87), hex(0xff875f), hex(0xffd75f), hex(0x87afff), hex(0x8a8a8a)],
    };

    /// The colour of a severity level: by its usual name, else by its position in the report's levels.
    pub fn severity(&self, name: &str, position: usize) -> Color {
        let named = crate::rpt::DEFAULT_SEVERITIES.iter().position(|n| *n == name);
        let i = named.unwrap_or(position).min(self.severity_scale.len() - 1);
        self.severity_scale[i]
    }
}
