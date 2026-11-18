//! Key notation, shared by `keys:` and a chain's `key:`: parsing, and matching terminal keys.
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rpt_tui::keys::Key;

fn ev(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, mods)
}

fn matches(notation: &str, event: KeyEvent) -> bool {
    Key::parse(notation).unwrap().matches(&event)
}

#[test]
fn single_characters_are_case_sensitive() {
    assert!(matches("g", ev(KeyCode::Char('g'), KeyModifiers::NONE)));
    assert!(!matches("g", ev(KeyCode::Char('G'), KeyModifiers::SHIFT)));
    assert!(matches("G", ev(KeyCode::Char('G'), KeyModifiers::SHIFT)), "terminals send shift with a capital");
    assert!(matches("?", ev(KeyCode::Char('?'), KeyModifiers::SHIFT)));
    assert!(matches(":", ev(KeyCode::Char(':'), KeyModifiers::NONE)));
}

#[test]
fn named_keys_match_their_terminal_keys() {
    for (name, code) in [
        ("enter", KeyCode::Enter),
        ("esc", KeyCode::Esc),
        ("tab", KeyCode::Tab),
        ("space", KeyCode::Char(' ')),
        ("backspace", KeyCode::Backspace),
        ("up", KeyCode::Up),
        ("down", KeyCode::Down),
        ("left", KeyCode::Left),
        ("right", KeyCode::Right),
        ("home", KeyCode::Home),
        ("end", KeyCode::End),
        ("pgup", KeyCode::PageUp),
        ("pgdn", KeyCode::PageDown),
        ("f1", KeyCode::F(1)),
        ("f12", KeyCode::F(12)),
    ] {
        assert!(matches(name, ev(code, KeyModifiers::NONE)), "{name}");
    }
}

#[test]
fn modifiers_are_joined_with_hyphens() {
    assert!(matches("ctrl-n", ev(KeyCode::Char('n'), KeyModifiers::CONTROL)));
    assert!(!matches("ctrl-n", ev(KeyCode::Char('n'), KeyModifiers::NONE)));
    assert!(matches("alt-i", ev(KeyCode::Char('i'), KeyModifiers::ALT)));
    assert!(matches("ctrl-alt-x", ev(KeyCode::Char('x'), KeyModifiers::CONTROL | KeyModifiers::ALT)));
    assert!(matches("alt-enter", ev(KeyCode::Enter, KeyModifiers::ALT)));
    assert!(matches("-", ev(KeyCode::Char('-'), KeyModifiers::NONE)));
    assert!(matches("ctrl--", ev(KeyCode::Char('-'), KeyModifiers::CONTROL)));
}

#[test]
fn ctrl_punctuation_matches_what_crossterm_reports_for_it() {
    // A terminal sends ctrl-] as byte 0x1d, which crossterm reports as ctrl-5.
    assert!(matches("ctrl-]", ev(KeyCode::Char('5'), KeyModifiers::CONTROL)));
    assert!(matches("ctrl-\\", ev(KeyCode::Char('4'), KeyModifiers::CONTROL)));
    assert!(matches("ctrl-]", ev(KeyCode::Char(']'), KeyModifiers::CONTROL)), "and as itself, by other protocols");
}

#[test]
fn bad_notation_says_what_is_wrong() {
    for bad in ["", "ctrl-", "shift-x", "hyper-x", "ctrl-foo", "f13", "xy", "enterr"] {
        assert!(Key::parse(bad).is_err(), "{bad:?} parsed");
    }
    assert_eq!(Key::parse("ctrl-foo").unwrap_err(), "`ctrl-foo`: `foo` is not a key");
}

#[test]
fn a_key_is_written_back_in_notation() {
    for k in ["g", "G", "ctrl-n", "ctrl-alt-x", "enter", "space", "f5", "ctrl-]", "-"] {
        assert_eq!(Key::parse(k).unwrap().to_string(), k);
    }
}

#[test]
fn one_byte_keys_are_characters_and_ctrl_keys() {
    let byte = |k: &str| Key::parse(k).unwrap().byte();
    assert_eq!(byte("ctrl-t"), Some(0x14));
    assert_eq!(byte("ctrl-]"), Some(0x1d));
    assert_eq!(byte("ctrl-space"), Some(0x00));
    assert_eq!(byte("x"), Some(b'x'));
    assert_eq!(byte("f5"), None);
    assert_eq!(byte("alt-t"), None);
    assert_eq!(byte("ctrl-1"), None);
    assert_eq!(byte("é"), None);
}
