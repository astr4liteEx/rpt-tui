//! What xterm would send for a key or mouse event, so a child sees a normal terminal.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use vt100::{MouseProtocolEncoding, MouseProtocolMode};

use super::Size;

/// xterm's modifier parameter: 1 + shift + 2·alt + 4·ctrl.
fn modifier_param(m: KeyModifiers) -> u8 {
    1 + m.contains(KeyModifiers::SHIFT) as u8
        + 2 * m.contains(KeyModifiers::ALT) as u8
        + 4 * m.contains(KeyModifiers::CONTROL) as u8
}

/// The control byte for ctrl+`c`, as xterm maps it.
fn control_byte(c: char) -> Option<u8> {
    Some(match c {
        'a'..='z' => c as u8 - b'a' + 1,
        'A'..='Z' => c as u8 - b'A' + 1,
        ' ' | '@' | '2' => 0,
        '[' | '3' => 27,
        '\\' | '4' => 28,
        ']' | '5' => 29,
        '^' | '6' => 30,
        '_' | '7' | '/' => 31,
        '8' | '?' => 127,
        _ => return None,
    })
}

/// Bytes for a key. `app_cursor` is the child's application cursor mode (DECCKM).
pub fn encode_key(k: &KeyEvent, app_cursor: bool) -> Vec<u8> {
    let m = modifier_param(k.modifiers);
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let alt = k.modifiers.contains(KeyModifiers::ALT);
    // Cursor keys: CSI 1;m X with modifiers, SS3 X in application mode, else CSI X.
    let cursor = |fin: char| -> Vec<u8> {
        if m > 1 {
            format!("\x1b[1;{m}{fin}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{fin}").into_bytes()
        } else {
            format!("\x1b[{fin}").into_bytes()
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if m > 1 { format!("\x1b[{n};{m}~").into_bytes() } else { format!("\x1b[{n}~").into_bytes() }
    };
    let mut out = match k.code {
        KeyCode::Char(c) if ctrl => match control_byte(c) {
            Some(b) => vec![b],
            None => c.to_string().into_bytes(),
        },
        KeyCode::Char(c) => c.to_string().into_bytes(),
        KeyCode::Enter => b"\r".to_vec(),
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => cursor('A'),
        KeyCode::Down => cursor('B'),
        KeyCode::Right => cursor('C'),
        KeyCode::Left => cursor('D'),
        KeyCode::Home => cursor('H'),
        KeyCode::End => cursor('F'),
        KeyCode::Insert => tilde(2),
        KeyCode::Delete => tilde(3),
        KeyCode::PageUp => tilde(5),
        KeyCode::PageDown => tilde(6),
        KeyCode::F(n @ 1..=4) => {
            let fin = ['P', 'Q', 'R', 'S'][n as usize - 1];
            if m > 1 { format!("\x1b[1;{m}{fin}").into_bytes() } else { format!("\x1bO{fin}").into_bytes() }
        }
        KeyCode::F(n @ 5..=12) => tilde([15, 17, 18, 19, 20, 21, 23, 24][n as usize - 5]),
        _ => vec![],
    };
    if alt && matches!(k.code, KeyCode::Char(_)) {
        out.insert(0, 0x1b);
    }
    out
}

/// Bytes for a mouse event over a pane at `origin` (column, row) of `size`, in the mouse
/// mode and encoding the child enabled. Empty when the child didn't ask for this event.
pub fn encode_mouse(
    ev: &MouseEvent,
    origin: (u16, u16),
    size: Size,
    mode: MouseProtocolMode,
    encoding: MouseProtocolEncoding,
) -> Vec<u8> {
    use MouseProtocolMode as Mode;
    let inside = ev.column >= origin.0
        && ev.row >= origin.1
        && ev.column < origin.0 + size.cols
        && ev.row < origin.1 + size.rows;
    if mode == Mode::None || !inside {
        return vec![];
    }
    let (x, y) = (ev.column - origin.0 + 1, ev.row - origin.1 + 1);
    let button = |b: MouseButton| match b {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    let (code, release) = match ev.kind {
        MouseEventKind::Down(b) => (button(b), false),
        MouseEventKind::Up(b) if mode != Mode::Press => (button(b), true),
        MouseEventKind::Drag(b) if matches!(mode, Mode::ButtonMotion | Mode::AnyMotion) => (32 + button(b), false),
        MouseEventKind::Moved if mode == Mode::AnyMotion => (35, false),
        MouseEventKind::ScrollUp => (64, false),
        MouseEventKind::ScrollDown => (65, false),
        _ => return vec![],
    };
    let m = ev.modifiers;
    let code = code
        + 4 * m.contains(KeyModifiers::SHIFT) as u16
        + 8 * m.contains(KeyModifiers::ALT) as u16
        + 16 * m.contains(KeyModifiers::CONTROL) as u16;
    match encoding {
        MouseProtocolEncoding::Sgr => format!("\x1b[<{code};{x};{y}{}", if release { 'm' } else { 'M' }).into_bytes(),
        _ => {
            // X10: one byte each, offset by 32; a release doesn't say which button.
            let code = if release { 3 } else { code };
            if x > 223 || y > 223 {
                return vec![];
            }
            vec![0x1b, b'[', b'M', (32 + code) as u8, (32 + x) as u8, (32 + y) as u8]
        }
    }
}

/// Bytes for pasted text: wrapped in `ESC[200~ … ESC[201~` only when the child enabled
/// bracketed paste.
pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed { format!("\x1b[200~{text}\x1b[201~").into_bytes() } else { text.as_bytes().to_vec() }
}

/// Alternate-screen switches the raw tee drops, so the child can't leave the TUI's screen.
const ALT_SCREEN: [&[u8]; 6] =
    [b"\x1b[?1049h", b"\x1b[?1049l", b"\x1b[?1047h", b"\x1b[?1047l", b"\x1b[?47h", b"\x1b[?47l"];

/// Drops alternate-screen switches from a byte stream, holding back a partial escape at the
/// end of one read so a switch split across reads is still caught.
#[derive(Debug, Default)]
pub struct AltScreenFilter {
    carry: Vec<u8>,
}

impl AltScreenFilter {
    pub fn filter(&mut self, data: &[u8]) -> Vec<u8> {
        let mut buf = std::mem::take(&mut self.carry);
        buf.extend_from_slice(data);
        let mut out = Vec::with_capacity(buf.len());
        let mut i = 0;
        'bytes: while i < buf.len() {
            if buf[i] == 0x1b {
                let rest = &buf[i..];
                if let Some(seq) = ALT_SCREEN.iter().find(|seq| rest.starts_with(seq)) {
                    i += seq.len();
                    continue 'bytes;
                }
                if ALT_SCREEN.iter().any(|seq| seq.starts_with(rest)) {
                    self.carry.extend_from_slice(rest);
                    return out;
                }
            }
            out.push(buf[i]);
            i += 1;
        }
        out
    }
}
