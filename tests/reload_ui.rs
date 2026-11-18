//! Reading the config again: on `reload-config`, and after a run when config.yaml or a theme
//! file changed.
mod common;

use common::AppScreen;
use ratatui::style::Color;

/// The screen's background, as drawn.
fn bg(s: &mut AppScreen) -> Color {
    s.draw();
    s.term.backend().buffer()[(99, 12)].bg
}

const LIGHT: Color = Color::Rgb(0xfa, 0xfa, 0xfa);

#[test]
fn reload_config_takes_in_new_chains_keys_and_theme() {
    let mut s = AppScreen::from_file("theme: charm\n");
    s.write_config("config.yaml", "theme: light\nchains:\n  hello:\n    key: H\n    steps: ['echo hello-there']\n");

    s.choose("reload-config");

    assert_eq!(bg(&mut s), LIGHT);
    s.typ("H");
    assert!(s.until("hello-there").contains("hello-there"));
}

#[test]
fn reload_config_toasts_the_new_configs_errors() {
    let mut s = AppScreen::from_file("");
    s.write_config("config.yaml", "notify: { bell: loud }\ntheme: { colors: { accent: nope } }\n");

    s.choose("reload-config");

    let out = s.draw();
    assert!(out.contains("notify: bell: not true or false"), "{out}");
    assert!(out.contains("`nope` is not a colour"), "{out}");
}

#[test]
fn history_settings_wait_for_the_next_start() {
    let mut s = AppScreen::from_file("history: { max_runs: 5 }\n");
    s.write_config("config.yaml", "history: { max_runs: 9 }\n");

    s.choose("reload-config");

    assert!(s.draw().contains("history: changes apply at the next start"));
}

#[test]
fn new_key_clashes_open_the_guide() {
    let mut s = AppScreen::from_file("");
    s.write_config("config.yaml", "chains:\n  jump:\n    key: j\n    steps: [true]\n");

    s.choose("reload-config");

    assert!(s.draw().contains("Fix keys"));
}

#[test]
fn a_run_that_changes_a_theme_file_reloads_when_it_ends() {
    let mut s = AppScreen::from_files(
        "theme: night\nchains:\n  wait:\n    key: W\n    steps: ['sleep 0.5']\n",
        &[("themes/night.yaml", "base: charm\n")],
    );
    assert_ne!(bg(&mut s), LIGHT);

    s.typ("W");
    s.draw();
    s.write_config("themes/night.yaml", "base: light\n");
    assert_ne!(bg(&mut s), LIGHT, "not while the run lives");
    s.until("✔ 0");

    assert_eq!(bg(&mut s), LIGHT);
}

#[test]
fn reload_config_leaves_out_a_chain_named_like_an_rpt_verb() {
    let mut s = AppScreen::from_file("");
    s.write_config("config.yaml", "chains:\n  new:\n    steps: ['echo mine']\n");

    s.choose("reload-config");

    let out = s.draw();
    assert!(out.contains("chain new: `new` is an rpt built-in"), "{out}");
}
