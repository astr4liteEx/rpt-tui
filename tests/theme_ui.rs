//! Themes in the app: the built-ins and theme files drawn, bad colours, the live picker, and
//! the colours a run is told.
mod common;

use common::AppScreen;
use ratatui::crossterm::event::KeyCode;
use ratatui::style::Color;

fn rgb(h: u32) -> Color {
    Color::Rgb((h >> 16) as u8, (h >> 8 & 0xff) as u8, (h & 0xff) as u8)
}

/// The header's name badge (accent) and the screen's background, as drawn.
fn colours(s: &mut AppScreen) -> (Color, Color) {
    s.draw();
    let buf = s.term.backend().buffer();
    (buf[(1, 0)].bg, buf[(99, 12)].bg)
}

#[test]
fn each_built_in_theme_draws_with_its_colours() {
    for (name, accent, bg) in [
        ("charm", rgb(0x7d56f4), rgb(0x1a1a24)),
        ("terminal", Color::Indexed(5), Color::Reset),
        ("light", rgb(0x6a3fe0), rgb(0xfafafa)),
    ] {
        let mut s = AppScreen::new(&format!("theme: {name}\n"), tempfile::tempdir().unwrap());
        assert_eq!(colours(&mut s), (accent, bg), "{name}");
    }
}

#[test]
fn a_theme_file_beside_the_config_draws_too() {
    let mut s = AppScreen::from_files(
        "theme: night\n",
        &[("themes/night.yaml", "base: light\ncolors: { accent: '#123456' }\n")],
    );

    assert_eq!(colours(&mut s), (rgb(0x123456), rgb(0xfafafa)));
}

#[test]
fn a_bad_colour_is_in_the_startup_toast_and_the_base_colour_is_used() {
    let mut s = AppScreen::new("theme: { colors: { accent: nope } }\n", tempfile::tempdir().unwrap());

    assert!(s.draw().contains("`nope` is not a colour"));
    assert_eq!(colours(&mut s).0, rgb(0x7d56f4));
}

#[test]
fn the_picker_previews_each_theme_and_esc_puts_it_back() {
    let config = "# mine\ntheme: charm  # the look\n";
    let mut s = AppScreen::from_file(config);

    s.choose("theme");
    let out = s.draw();
    assert!(out.contains("charm") && out.contains("terminal") && out.contains("light"), "{out}");
    s.typ("j");
    assert_eq!(colours(&mut s).0, Color::Indexed(5), "terminal, previewed");
    s.key(KeyCode::Esc);

    assert_eq!(colours(&mut s).0, rgb(0x7d56f4));
    assert_eq!(s.config(), config, "nothing written");
}

#[test]
fn enter_in_the_picker_keeps_the_theme_and_writes_only_its_line() {
    let mut s = AppScreen::from_file("# mine\ntheme: charm  # the look\nkeys: { quit: Q }\n");

    s.choose("theme");
    s.typ("jj");
    s.key(KeyCode::Enter);

    assert_eq!(colours(&mut s), (rgb(0x6a3fe0), rgb(0xfafafa)));
    assert_eq!(s.config(), "# mine\ntheme: light  # the look\nkeys: { quit: Q }\n");
    assert!(s.backup().is_some(), "backed up first");
}

#[test]
fn the_picker_keeps_a_theme_maps_overrides_on_each_choice() {
    let mut s = AppScreen::from_file("theme:\n  base: charm\n  colors: { accent: '#123456' }\n");

    s.choose("theme");
    s.typ("jj");
    assert_eq!(colours(&mut s), (rgb(0x123456), rgb(0xfafafa)), "light, with the override");
    s.key(KeyCode::Enter);

    assert_eq!(s.config(), "theme:\n  base: light\n  colors: { accent: '#123456' }\n");
}

#[test]
fn a_run_asking_for_the_background_is_told_the_themes() {
    let mut s = AppScreen::new(
        "theme: { base: terminal, colors: { bg: 196 } }\nchains:\n  ask:\n    key: A\n    steps: [\"printf '\\\\033]11;?\\\\033\\\\\\\\'; IFS= read -r -s -d '\\\\' -t 2 r; echo \\\"got ${r#*;}\\\"\"]\n",
        tempfile::tempdir().unwrap(),
    );

    s.typ("A");
    let out = s.until("got rgb");

    assert!(out.contains("got rgb:ffff/0000/0000"), "{out}");
}

#[test]
fn a_previewed_theme_with_a_bad_colour_says_so() {
    let mut s = AppScreen::from_files("theme: charm\n", &[("themes/night.yaml", "colors: { accent: nope }\n")]);

    s.choose("theme");
    s.typ("jjj");

    assert!(s.draw().contains("`nope` is not a colour"));
}
