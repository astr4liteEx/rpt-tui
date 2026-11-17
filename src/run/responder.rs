//! Answers the terminal queries vt100 leaves unhandled. Without it nvim shows E1568 and
//! prompt_toolkit stalls for 2 s. Off during raw takeover, when the real terminal answers.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The colours OSC 10 (text) and OSC 11 (background) report, as RGB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colors {
    pub fg: (u8, u8, u8),
    pub bg: (u8, u8, u8),
}

impl Default for Colors {
    /// A dark grey on near-black; the UI passes its theme's colours.
    fn default() -> Colors {
        Colors { fg: (0xdd, 0xdd, 0xdd), bg: (0x1a, 0x1a, 0x24) }
    }
}

pub(crate) struct Responder {
    enabled: Arc<AtomicBool>,
    colors: Colors,
    /// Bytes to write back to the child, drained by the reader thread.
    pub(crate) replies: Vec<u8>,
}

impl Responder {
    pub(crate) fn new(colors: Colors, enabled: Arc<AtomicBool>) -> Responder {
        Responder { enabled, colors, replies: vec![] }
    }

    fn reply(&mut self, s: &str) {
        if self.enabled.load(Ordering::Relaxed) {
            self.replies.extend_from_slice(s.as_bytes());
        }
    }
}

/// An X11 colour spec, `rgb:rrrr/gggg/bbbb`, as xterm reports it.
fn x11(c: (u8, u8, u8)) -> String {
    format!("rgb:{0:02x}{0:02x}/{1:02x}{1:02x}/{2:02x}{2:02x}", c.0, c.1, c.2)
}

impl vt100::Callbacks for Responder {
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        _i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        let p0 = params.first().and_then(|p| p.first()).copied().unwrap_or(0);
        match (i1, c, p0) {
            // DSR: "terminal OK".
            (None, 'n', 5) => self.reply("\x1b[0n"),
            // CPR: cursor position, 1-based.
            (None, 'n', 6) => {
                let (row, col) = screen.cursor_position();
                self.reply(&format!("\x1b[{};{}R", row + 1, col + 1));
            }
            // DA1: a VT220 with ANSI colour.
            (None, 'c', 0) => self.reply("\x1b[?62;22c"),
            // DA2: terminal type 0, version 10.
            (Some(b'>'), 'c', 0) => self.reply("\x1b[>0;10;1c"),
            _ => {}
        }
    }

    fn unhandled_osc(&mut self, _screen: &mut vt100::Screen, params: &[&[u8]]) {
        let colour = match params {
            [b"10", b"?", ..] => self.colors.fg,
            [b"11", b"?", ..] => self.colors.bg,
            _ => return,
        };
        let which = String::from_utf8_lossy(params[0]).into_owned();
        self.reply(&format!("\x1b]{which};{}\x1b\\", x11(colour)));
    }
}
