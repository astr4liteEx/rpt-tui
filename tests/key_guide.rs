//! The guide that fixes key clashes at startup, and the fixes it saves to `config.yaml`.
mod common;

use common::AppScreen;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

const JUMP: &str = "# my config\nchains:\n  jump:\n    description: Jump around\n    key: j # mine\n    steps: ['echo jumped; sleep 5']\n";

/// Moves to option `n` (from 0) and chooses it.
fn pick(s: &mut AppScreen, n: usize) {
    for _ in 0..n {
        s.key(KeyCode::Down);
    }
    s.key(KeyCode::Enter);
}

#[test]
fn a_clash_opens_the_guide_at_startup() {
    let mut s = AppScreen::from_file(JUMP);

    let out = s.draw();

    insta::assert_snapshot!(out);
}

#[test]
fn leaving_the_chain_without_a_hotkey_removes_its_key_line_and_backs_up_the_file() {
    let mut s = AppScreen::from_file(JUMP);

    pick(&mut s, 1);

    assert_eq!(s.config(), JUMP.replace("    key: j # mine\n", ""));
    assert_eq!(s.backup().as_deref(), Some(JUMP));
    let out = s.draw();
    assert!(!out.contains("Fix keys"), "the guide closes when nothing is left: {out}");
}

#[test]
fn a_new_key_that_clashes_again_is_refused_and_another_taken() {
    let mut s = AppScreen::from_file(JUMP);
    pick(&mut s, 0);
    assert!(s.draw().contains("press the new key for chain jump"));

    s.typ("k");
    let out = s.draw();
    assert!(out.contains("key k is taken by up (lists)"), "{out}");

    s.typ("h");
    assert_eq!(s.config(), JUMP.replace("key: j # mine", "key: h"));
    s.typ("h");
    assert!(s.until("jumped").contains("jumped"));
}

#[test]
fn moving_the_action_off_the_key_frees_it_for_the_chain() {
    let mut s = AppScreen::from_file(JUMP);
    pick(&mut s, 2);

    s.typ("n");

    assert_eq!(s.config(), format!("{JUMP}keys:\n  down: [n, down]\n"));
    s.typ("j");
    assert!(s.until("jumped").contains("jumped"));
}

#[test]
fn unbinding_the_key_from_the_action_frees_it_too() {
    let mut s = AppScreen::from_file(&format!("{JUMP}keys:\n  quit: Q\n"));

    pick(&mut s, 3);

    assert_eq!(s.config(), format!("{JUMP}keys:\n  quit: Q\n  down: down\n"));
}

#[test]
fn a_rebound_action_in_a_clash_can_drop_the_key_and_keep_its_default() {
    let text = "keys:\n  validate: q\n  help: f1\n";
    let mut s = AppScreen::from_file(text);
    assert!(s.draw().contains("keys: validate: key q is taken by quit (global)"));

    pick(&mut s, 1);

    assert_eq!(s.config(), "keys:\n  help: f1\n");
    s.typ("v");
    assert!(s.until("rpt validate").contains("rpt validate"));
}

#[test]
fn esc_skips_and_fix_keys_in_the_palette_reopens_the_guide() {
    let mut s = AppScreen::from_file(JUMP);

    s.key(KeyCode::Esc);
    let out = s.draw();
    assert!(!out.contains("Fix keys"), "{out}");
    assert_eq!(s.config(), JUMP, "skipping changes nothing");

    s.typ(":fix-keys");
    s.key(KeyCode::Enter);
    assert!(s.draw().contains("chain jump: key j is taken by down (lists)"));
}

#[test]
fn a_file_that_cannot_be_edited_safely_is_fixed_for_the_session_with_the_line_to_add() {
    let text = "chains:\n  jump: { key: j, steps: ['echo jumped; sleep 5'] }\n";
    let mut s = AppScreen::from_file(text);

    pick(&mut s, 0);
    s.typ("h");
    let out = s.draw();

    assert_eq!(s.config(), text);
    assert_eq!(s.backup(), None);
    assert!(out.contains("key: h"), "the toast gives the line: {out}");
    s.typ("h");
    assert!(s.until("jumped").contains("jumped"));
}

#[test]
fn problems_are_taken_one_at_a_time() {
    let text = "chains:\n  one:\n    key: j\n    steps: [true]\n  two:\n    key: hyper-x\n    steps: [true]\n";
    let mut s = AppScreen::from_file(text);
    assert!(s.draw().contains("1 of 2"));

    pick(&mut s, 1);
    let out = s.draw();

    assert!(out.contains("chain two: `hyper-x`: `hyper` is not a modifier"), "{out}");
    pick(&mut s, 1);
    assert_eq!(s.config(), "chains:\n  one:\n    steps: [true]\n  two:\n    steps: [true]\n");
}

#[test]
fn toggle_takeover_takes_only_a_one_byte_key() {
    let mut s = AppScreen::from_file("keys:\n  toggle_takeover: f5\n");
    pick(&mut s, 0);

    s.key(KeyCode::F(6));
    assert!(s.draw().contains("f6 is not one byte"));
    s.app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));

    assert_eq!(s.config(), "keys:\n  toggle_takeover: ctrl-o\n");
}
