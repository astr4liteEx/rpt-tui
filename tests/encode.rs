//! Byte translation: keys and mouse events to what xterm would send, and the raw-tee filter.
//! Expected bytes are from xterm's ctlseqs reference.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rpt_tui::run::encode_key;

fn key(code: KeyCode, mods: KeyModifiers) -> Vec<u8> {
    encode_key(&KeyEvent::new(code, mods), false)
}

const NONE: KeyModifiers = KeyModifiers::NONE;

#[test]
fn printable_characters_are_their_utf8() {
    assert_eq!(key(KeyCode::Char('a'), NONE), b"a");
    assert_eq!(key(KeyCode::Char('A'), KeyModifiers::SHIFT), b"A");
    assert_eq!(key(KeyCode::Char('é'), NONE), "é".as_bytes());
}

#[test]
fn control_and_alt_combinations() {
    assert_eq!(key(KeyCode::Char('a'), KeyModifiers::CONTROL), [0x01]);
    assert_eq!(key(KeyCode::Char('c'), KeyModifiers::CONTROL), [0x03]);
    assert_eq!(key(KeyCode::Char(' '), KeyModifiers::CONTROL), [0x00]);
    assert_eq!(key(KeyCode::Char('x'), KeyModifiers::ALT), b"\x1bx");
    assert_eq!(key(KeyCode::Char('a'), KeyModifiers::CONTROL | KeyModifiers::ALT), b"\x1b\x01");
}

#[test]
fn editing_keys() {
    assert_eq!(key(KeyCode::Enter, NONE), b"\r");
    assert_eq!(key(KeyCode::Tab, NONE), b"\t");
    assert_eq!(key(KeyCode::BackTab, KeyModifiers::SHIFT), b"\x1b[Z");
    assert_eq!(key(KeyCode::Backspace, NONE), [0x7f]);
    assert_eq!(key(KeyCode::Esc, NONE), [0x1b]);
    assert_eq!(key(KeyCode::Delete, NONE), b"\x1b[3~");
    assert_eq!(key(KeyCode::Insert, NONE), b"\x1b[2~");
    assert_eq!(key(KeyCode::PageUp, NONE), b"\x1b[5~");
    assert_eq!(key(KeyCode::PageDown, NONE), b"\x1b[6~");
}

#[test]
fn cursor_keys_follow_application_cursor_mode() {
    assert_eq!(key(KeyCode::Up, NONE), b"\x1b[A");
    assert_eq!(key(KeyCode::Left, NONE), b"\x1b[D");
    assert_eq!(key(KeyCode::Home, NONE), b"\x1b[H");
    assert_eq!(encode_key(&KeyEvent::new(KeyCode::Up, NONE), true), b"\x1bOA");
    assert_eq!(encode_key(&KeyEvent::new(KeyCode::End, NONE), true), b"\x1bOF");
}

#[test]
fn modified_cursor_and_tilde_keys_carry_the_modifier_parameter() {
    assert_eq!(key(KeyCode::Up, KeyModifiers::SHIFT), b"\x1b[1;2A");
    assert_eq!(key(KeyCode::Right, KeyModifiers::CONTROL), b"\x1b[1;5C");
    assert_eq!(key(KeyCode::Delete, KeyModifiers::CONTROL), b"\x1b[3;5~");
    // A modifier wins over application cursor mode.
    assert_eq!(encode_key(&KeyEvent::new(KeyCode::Up, KeyModifiers::ALT), true), b"\x1b[1;3A");
}

#[test]
fn function_keys() {
    assert_eq!(key(KeyCode::F(1), NONE), b"\x1bOP");
    assert_eq!(key(KeyCode::F(4), NONE), b"\x1bOS");
    assert_eq!(key(KeyCode::F(5), NONE), b"\x1b[15~");
    assert_eq!(key(KeyCode::F(12), NONE), b"\x1b[24~");
    assert_eq!(key(KeyCode::F(1), KeyModifiers::SHIFT), b"\x1b[1;2P");
}

mod mouse {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use rpt_tui::run::{Size, encode_mouse};
    use vt100::{MouseProtocolEncoding as Enc, MouseProtocolMode as Mode};

    /// The pane starts at column 10, row 3 and is 40×10.
    const ORIGIN: (u16, u16) = (10, 3);
    const PANE: Size = Size { rows: 10, cols: 40 };

    fn ev(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE }
    }

    fn enc(e: MouseEvent, mode: Mode, encoding: Enc) -> Vec<u8> {
        encode_mouse(&e, ORIGIN, PANE, mode, encoding)
    }

    const LEFT_DOWN: MouseEventKind = MouseEventKind::Down(MouseButton::Left);
    const LEFT_UP: MouseEventKind = MouseEventKind::Up(MouseButton::Left);

    #[test]
    fn nothing_is_sent_unless_the_child_asked_for_the_mouse() {
        assert_eq!(enc(ev(LEFT_DOWN, 12, 5), Mode::None, Enc::Sgr), b"");
    }

    #[test]
    fn sgr_coordinates_are_one_based_and_relative_to_the_pane() {
        assert_eq!(enc(ev(LEFT_DOWN, 12, 5), Mode::PressRelease, Enc::Sgr), b"\x1b[<0;3;3M");
        assert_eq!(enc(ev(LEFT_UP, 12, 5), Mode::PressRelease, Enc::Sgr), b"\x1b[<0;3;3m");
        assert_eq!(
            enc(ev(MouseEventKind::Down(MouseButton::Right), 10, 3), Mode::PressRelease, Enc::Sgr),
            b"\x1b[<2;1;1M"
        );
    }

    #[test]
    fn x10_encoding_offsets_by_32_and_releases_as_button_3() {
        assert_eq!(enc(ev(LEFT_DOWN, 12, 5), Mode::PressRelease, Enc::Default), b"\x1b[M ##");
        assert_eq!(enc(ev(LEFT_UP, 12, 5), Mode::PressRelease, Enc::Default), b"\x1b[M###");
    }

    #[test]
    fn press_only_mode_drops_releases_and_motion() {
        assert_eq!(enc(ev(LEFT_UP, 12, 5), Mode::Press, Enc::Sgr), b"");
        assert_eq!(enc(ev(MouseEventKind::Drag(MouseButton::Left), 12, 5), Mode::PressRelease, Enc::Sgr), b"");
    }

    #[test]
    fn motion_wheel_and_modifiers() {
        assert_eq!(
            enc(ev(MouseEventKind::Drag(MouseButton::Left), 12, 5), Mode::ButtonMotion, Enc::Sgr),
            b"\x1b[<32;3;3M"
        );
        assert_eq!(enc(ev(MouseEventKind::Moved, 12, 5), Mode::AnyMotion, Enc::Sgr), b"\x1b[<35;3;3M");
        assert_eq!(enc(ev(MouseEventKind::ScrollUp, 12, 5), Mode::PressRelease, Enc::Sgr), b"\x1b[<64;3;3M");
        assert_eq!(enc(ev(MouseEventKind::ScrollDown, 12, 5), Mode::PressRelease, Enc::Sgr), b"\x1b[<65;3;3M");
        let mut e = ev(LEFT_DOWN, 12, 5);
        e.modifiers = KeyModifiers::CONTROL;
        assert_eq!(enc(e, Mode::PressRelease, Enc::Sgr), b"\x1b[<16;3;3M");
    }

    #[test]
    fn events_outside_the_pane_are_dropped() {
        assert_eq!(enc(ev(LEFT_DOWN, 9, 5), Mode::PressRelease, Enc::Sgr), b"");
        assert_eq!(enc(ev(LEFT_DOWN, 50, 5), Mode::PressRelease, Enc::Sgr), b"");
        assert_eq!(enc(ev(LEFT_DOWN, 12, 13), Mode::PressRelease, Enc::Sgr), b"");
    }
}

#[test]
fn paste_is_bracketed_only_when_the_child_enabled_it() {
    assert_eq!(rpt_tui::run::encode_paste("ls -l", true), b"\x1b[200~ls -l\x1b[201~");
    assert_eq!(rpt_tui::run::encode_paste("ls -l", false), b"ls -l");
}

mod alt_screen_filter {
    use rpt_tui::run::AltScreenFilter;

    #[test]
    fn every_alternate_screen_switch_is_dropped() {
        for seq in ["\x1b[?1049h", "\x1b[?1049l", "\x1b[?1047h", "\x1b[?1047l", "\x1b[?47h", "\x1b[?47l"] {
            let mut f = AltScreenFilter::default();
            assert_eq!(f.filter(format!("a{seq}b").as_bytes()), b"ab", "{seq:?}");
        }
    }

    #[test]
    fn a_switch_split_across_reads_is_still_dropped() {
        let mut f = AltScreenFilter::default();
        assert_eq!(f.filter(b"x\x1b[?10"), b"x");
        assert_eq!(f.filter(b"49lhello\x1b[1m"), b"hello\x1b[1m");
    }

    #[test]
    fn look_alikes_pass_through_whole_even_when_split() {
        let mut f = AltScreenFilter::default();
        assert_eq!(f.filter(b"\x1b[?10"), b"");
        assert_eq!(f.filter(b"00h"), b"\x1b[?1000h");
        assert_eq!(f.filter(b"\x1b"), b"");
        assert_eq!(f.filter(b"[2J"), b"\x1b[2J");
        assert_eq!(f.filter(b"\x1b[?25l"), b"\x1b[?25l");
    }
}
