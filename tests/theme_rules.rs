//! Themes: colours, the built-ins, theme files and overrides, and severity colours.
use std::path::Path;

use ratatui::style::Color;
use rpt_tui::ui::theme::{self, Theme};
use serde_json::{Value, json};

fn rgb(h: u32) -> Color {
    Color::Rgb((h >> 16) as u8, (h >> 8 & 0xff) as u8, (h & 0xff) as u8)
}

/// The theme `setting` (the `theme:` value as YAML) names, with theme files from `dir`.
fn resolve(setting: &str, dir: Option<&Path>) -> (Theme, Vec<String>) {
    let v: Value = rpt_tui::config::load(&format!("theme: {setting}\n")).theme.unwrap();
    theme::resolve(Some(&v), dir, true)
}

fn file(dir: &Path, name: &str, text: &str) {
    std::fs::create_dir_all(dir.join("themes")).unwrap();
    std::fs::write(dir.join("themes").join(format!("{name}.yaml")), text).unwrap();
}

#[test]
fn colours_are_hex_names_indexes_or_default() {
    let c = |v: Value| theme::parse_colour(&v);
    assert_eq!(c(json!("#7d56f4")), Ok(rgb(0x7d56f4)));
    assert_eq!(c(json!("red")), Ok(Color::Indexed(1)));
    assert_eq!(c(json!("Bright-Black")), Ok(Color::Indexed(8)), "names ignore case");
    assert_eq!(c(json!(208)), Ok(Color::Indexed(208)));
    assert_eq!(c(json!("208")), Ok(Color::Indexed(208)));
    assert_eq!(c(json!("default")), Ok(Color::Reset));
    for bad in [json!("#ff00"), json!("crimson"), json!(256), json!(-1), json!(true)] {
        assert!(c(bad.clone()).is_err(), "{bad}");
    }
}

#[test]
fn without_true_colour_hex_maps_to_the_nearest_of_the_cube_or_greys() {
    assert_eq!(theme::nearest_256(rgb(0xff0000)), Color::Indexed(196));
    assert_eq!(theme::nearest_256(rgb(0x000000)), Color::Indexed(16), "never 0–15");
    assert_eq!(theme::nearest_256(rgb(0x808080)), Color::Indexed(244));
    assert_eq!(theme::nearest_256(Color::Indexed(3)), Color::Indexed(3), "only hex is mapped");

    let (t, _) = theme::resolve(None, None, false);
    assert_eq!(t.bg, theme::nearest_256(rgb(0x1a1a24)));
}

#[test]
fn the_built_ins_have_the_colours_the_spec_gives() {
    let (charm, errors) = theme::resolve(None, None, true);
    assert!(errors.is_empty());
    assert_eq!((charm.bg, charm.accent, charm.border_run), (rgb(0x1a1a24), rgb(0x7d56f4), rgb(0xf25d94)));
    let (terminal, _) = resolve("terminal", None);
    assert_eq!((terminal.bg, terminal.text, terminal.surface), (Color::Reset, Color::Reset, Color::Indexed(0)));
    assert_eq!((terminal.accent, terminal.border_run), (Color::Indexed(5), Color::Indexed(13)));
    let (light, _) = resolve("light", None);
    assert_eq!((light.bg, light.text, light.accent), (rgb(0xfafafa), rgb(0x2a2a3a), rgb(0x6a3fe0)));
}

#[test]
fn a_mapping_layers_its_colours_over_its_base() {
    let (t, errors) = resolve("{ base: light, colors: { accent: '#ff79c6', border_run: magenta } }", None);

    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!((t.accent, t.border_run), (rgb(0xff79c6), Color::Indexed(5)));
    assert_eq!(t.bg, rgb(0xfafafa), "the rest is the base's");
    let (t, _) = resolve("{ colors: { accent: red } }", None);
    assert_eq!(t.bg, rgb(0x1a1a24), "the base defaults to charm");
}

#[test]
fn a_bad_colour_or_key_is_an_error_and_the_base_value_stays() {
    let (t, errors) = resolve("{ base: light, colors: { accent: '#ff00', shadow: red }, size: 3 }", None);

    assert_eq!(t.accent, rgb(0x6a3fe0));
    assert!(
        errors.contains(
            &"theme: colors.accent: `#ff00` is not a colour (#rrggbb, a name like red, 0–255 or default)".to_string()
        ),
        "{errors:?}"
    );
    assert!(errors.iter().any(|e| e.contains("colors.shadow") && e.contains("unknown")), "{errors:?}");
    assert!(errors.iter().any(|e| e.contains("size") && e.contains("unknown")), "{errors:?}");
}

#[test]
fn a_theme_file_is_named_like_a_built_in_and_may_base_on_another_file() {
    let dir = tempfile::tempdir().unwrap();
    file(dir.path(), "night", "base: terminal\ncolors: { accent: '#123456' }\n");
    file(dir.path(), "deeper", "base: night\ncolors: { ok: blue }\n");

    let (t, errors) = resolve("deeper", Some(dir.path()));

    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!((t.ok, t.accent, t.bg), (Color::Indexed(4), rgb(0x123456), Color::Reset));
    assert_eq!(theme::names(Some(dir.path())), ["charm", "terminal", "light", "deeper", "night"]);
}

#[test]
fn unknown_names_loops_and_files_named_like_built_ins_fall_back_with_errors() {
    let dir = tempfile::tempdir().unwrap();
    file(dir.path(), "a", "base: b\n");
    file(dir.path(), "b", "base: a\n");
    file(dir.path(), "charm", "colors: { bg: red }\n");
    file(dir.path(), "Light", "colors: { bg: red }\n");

    let (t, errors) = resolve("nope", Some(dir.path()));
    assert_eq!(t.bg, rgb(0x1a1a24));
    assert!(
        errors.iter().any(|e| e == "theme nope: no such theme (charm, terminal, light, or a file in themes/)"),
        "{errors:?}"
    );
    assert!(errors.iter().any(|e| e.contains("themes/charm.yaml") && e.contains("built-in")), "{errors:?}");
    assert!(errors.iter().any(|e| e.contains("themes/Light.yaml") && e.contains("built-in")), "{errors:?}");
    assert!(!theme::names(Some(dir.path())).contains(&"Light".to_string()));

    let (t, errors) = resolve("a", Some(dir.path()));
    assert_eq!(t.bg, rgb(0x1a1a24));
    assert!(errors.iter().any(|e| e.contains("loop")), "{errors:?}");

    let (t, _) = resolve("charm", Some(dir.path()));
    assert_eq!(t.bg, rgb(0x1a1a24), "the built-in, not the file");
}

#[test]
fn severity_colours_go_by_exact_name_then_glob_then_rank() {
    let (t, errors) = resolve(
        "{ severity: { P1: '#ff0000', '*crit*': '#00ff00', 'p?': '#0000ff' }, severity_scale: ['#111111', '#222222'] }",
        None,
    );
    assert!(errors.is_empty(), "{errors:?}");

    assert_eq!(t.severity("p1", 4), rgb(0xff0000), "exact, ignoring case");
    assert_eq!(t.severity("Very-Critical", 4), rgb(0x00ff00), "the first glob that matches");
    assert_eq!(t.severity("p2", 4), rgb(0x0000ff));
    assert_eq!(t.severity("whatever", 0), rgb(0x111111), "by rank");
    assert_eq!(t.severity("whatever", 7), rgb(0x222222), "the last repeats");
    assert_eq!(t.severity("high", 9), rgb(0x222222), "built-in names follow the scale");
    assert_eq!(t.severity("critical", 9), rgb(0x00ff00), "the theme's glob wins over the built-in name");
}

#[test]
fn the_built_in_severity_names_are_their_place_on_the_scale() {
    let (t, _) = theme::resolve(None, None, true);

    assert_eq!(t.severity("high", 0), rgb(0xff875f), "high is second, whatever the report's rank");
    assert_eq!(t.severity("info", 0), rgb(0x8a8a8a));
    assert_eq!(t.severity("weird", 2), rgb(0xffd75f));
}

#[test]
fn a_theme_answers_colour_queries_in_rgb() {
    let (light, _) = resolve("light", None);
    assert_eq!(light.query_colours(), ((0x2a, 0x2a, 0x3a), (0xfa, 0xfa, 0xfa)), "text and bg");
    let (terminal, _) = resolve("{ base: terminal, colors: { text: red, bg: 196 } }", None);
    assert_eq!(terminal.query_colours(), ((0xcd, 0x00, 0x00), (0xff, 0x00, 0x00)), "xterm's colours");
    let (terminal, _) = resolve("terminal", None);
    assert_eq!(terminal.query_colours(), ((0xdd, 0xdd, 0xdd), (0x1a, 0x1a, 0x24)), "default answers as charm");
}
