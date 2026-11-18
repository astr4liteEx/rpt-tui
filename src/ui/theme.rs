//! Themes: the 11 interface colours and the severity colours. Built-ins `charm`, `terminal`
//! and `light`; `theme:` names one or layers overrides on a base; `themes/<name>.yaml` in the
//! config folder are themes of the same shape.

use std::path::Path;

use ratatui::style::Color;
use serde_json::{Map, Value};

/// The built-in themes, in the picker's order.
const BUILT_INS: [&str; 3] = ["charm", "terminal", "light"];

/// The interface tokens, as `colors:` names them.
const TOKENS: [&str; 11] =
    ["bg", "surface", "text", "muted", "border", "border_focus", "border_run", "accent", "ok", "warn", "err"];

/// The keys a theme map may have.
const THEME_KEYS: [&str; 4] = ["base", "colors", "severity", "severity_scale"];

/// A severity colour: a colour, or a place on the scale (how the built-ins name levels, so a
/// new scale moves them too).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sev {
    Colour(Color),
    Scale(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Severity colours by level name (exact, or a glob with `*` and `?`), lowercased: the
    /// theme's own first, then its base's, and so on.
    severity: Vec<Vec<(String, Sev)>>,
    /// Severity colours by rank, most severe first; the last repeats.
    severity_scale: Vec<Color>,
}

/// `0xrrggbb` as its three parts.
const fn split(h: u32) -> (u8, u8, u8) {
    ((h >> 16) as u8, (h >> 8 & 0xff) as u8, (h & 0xff) as u8)
}

/// `0xrrggbb` as a colour.
const fn hex(h: u32) -> Color {
    let (r, g, b) = split(h);
    Color::Rgb(r, g, b)
}

/// A built-in name, whatever its case: theme files may not take one.
fn is_built_in(name: &str) -> bool {
    BUILT_INS.iter().any(|b| b.eq_ignore_ascii_case(name))
}

/// What a theme map may hold, for errors: `` `base`, `colors`, … ``.
fn theme_keys() -> String {
    THEME_KEYS.map(|k| format!("`{k}`")).join(", ")
}

/// ANSI colour `i` (0–15) or a 256-colour index.
const fn ansi(i: u8) -> Color {
    Color::Indexed(i)
}

/// The 16 ANSI colour names, by index.
const ANSI_NAMES: [&str; 16] = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "bright-black",
    "bright-red",
    "bright-green",
    "bright-yellow",
    "bright-blue",
    "bright-magenta",
    "bright-cyan",
    "bright-white",
];

/// xterm's RGB for the 16 ANSI colours.
const XTERM_16: [u32; 16] = [
    0x000000, 0xcd0000, 0x00cd00, 0xcdcd00, 0x0000ee, 0xcd00cd, 0x00cdcd, 0xe5e5e5, 0x7f7f7f, 0xff0000, 0x00ff00,
    0xffff00, 0x5c5cff, 0xff00ff, 0x00ffff, 0xffffff,
];

/// The cube's levels for indexes 16–231.
const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

impl Theme {
    /// The default theme: true colour, the mockups' look.
    pub fn charm() -> Theme {
        Theme::with(
            [
                hex(0x1a1a24),
                hex(0x2a2a3a),
                hex(0xdddddd),
                hex(0x7a7a8c),
                hex(0x44445a),
                hex(0x7d56f4),
                hex(0xf25d94),
                hex(0x7d56f4),
                hex(0x04b575),
                hex(0xffd75f),
                hex(0xff5f87),
            ],
            vec![hex(0xff5f87), hex(0xff875f), hex(0xffd75f), hex(0x87afff), hex(0x8a8a8a)],
        )
    }

    /// The tokens in [`TOKENS`] order and the scale; levels named as rpt's defaults are.
    fn with(tokens: [Color; 11], severity_scale: Vec<Color>) -> Theme {
        let [bg, surface, text, muted, border, border_focus, border_run, accent, ok, warn, err] = tokens;
        let names = crate::rpt::DEFAULT_SEVERITIES.iter().enumerate().map(|(i, n)| (n.to_string(), Sev::Scale(i)));
        Theme {
            bg,
            surface,
            text,
            muted,
            border,
            border_focus,
            border_run,
            accent,
            ok,
            warn,
            err,
            severity: vec![names.collect()],
            severity_scale,
        }
    }

    /// A built-in theme by name.
    pub fn built_in(name: &str) -> Option<Theme> {
        match name {
            "charm" => Some(Theme::charm()),
            "terminal" => Some(Theme::with(
                [
                    Color::Reset,
                    ansi(0),
                    Color::Reset,
                    ansi(8),
                    ansi(8),
                    ansi(5),
                    ansi(13),
                    ansi(5),
                    ansi(2),
                    ansi(3),
                    ansi(1),
                ],
                vec![ansi(9), ansi(1), ansi(3), ansi(4), ansi(8)],
            )),
            "light" => Some(Theme::with(
                [
                    hex(0xfafafa),
                    hex(0xececf2),
                    hex(0x2a2a3a),
                    hex(0x6c6c80),
                    hex(0xc6c6d4),
                    hex(0x6a3fe0),
                    hex(0xd6336c),
                    hex(0x6a3fe0),
                    hex(0x0a8a5a),
                    hex(0xb07800),
                    hex(0xd6336c),
                ],
                vec![hex(0xd6336c), hex(0xd9622b), hex(0xb07800), hex(0x3a6fd8), hex(0x6c6c80)],
            )),
            _ => None,
        }
    }

    /// The token `colors:` calls `name`.
    fn token(&mut self, name: &str) -> Option<&mut Color> {
        Some(match name {
            "bg" => &mut self.bg,
            "surface" => &mut self.surface,
            "text" => &mut self.text,
            "muted" => &mut self.muted,
            "border" => &mut self.border,
            "border_focus" => &mut self.border_focus,
            "border_run" => &mut self.border_run,
            "accent" => &mut self.accent,
            "ok" => &mut self.ok,
            "warn" => &mut self.warn,
            "err" => &mut self.err,
            _ => return None,
        })
    }

    /// A severity level's colour: the theme's own exact name, then its first matching glob,
    /// then its base's the same way; else by the level's rank on the scale.
    pub fn severity(&self, name: &str, rank: usize) -> Color {
        let name = name.to_lowercase();
        let found = self.severity.iter().find_map(|layer| {
            let exact = layer.iter().find(|(k, _)| !is_glob(k) && *k == name);
            exact.or_else(|| layer.iter().find(|(k, _)| is_glob(k) && glob(k, &name))).map(|(_, s)| *s)
        });
        match found.unwrap_or(Sev::Scale(rank)) {
            Sev::Colour(c) => c,
            Sev::Scale(i) => self.severity_scale[i.min(self.severity_scale.len() - 1)],
        }
    }

    /// What a run's colour queries (OSC 10 and 11) are answered with: `text` and `bg` in RGB;
    /// `default` can't be known, so it answers as charm's.
    pub fn query_colours(&self) -> ((u8, u8, u8), (u8, u8, u8)) {
        let charm = Theme::charm();
        (rgb_or(self.text, charm.text), rgb_or(self.bg, charm.bg))
    }

    /// Every colour, changed by `f`.
    fn map(&mut self, f: impl Fn(Color) -> Color) {
        for t in TOKENS {
            if let Some(c) = self.token(t) {
                *c = f(*c);
            }
        }
        for c in &mut self.severity_scale {
            *c = f(*c);
        }
        for (_, s) in self.severity.iter_mut().flatten() {
            if let Sev::Colour(c) = s {
                *c = f(*c);
            }
        }
    }
}

/// `c` in RGB, or `fallback`'s when it is the terminal's default.
fn rgb_or(c: Color, fallback: Color) -> (u8, u8, u8) {
    let as_rgb = |c: Color| match c {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Indexed(i) => {
            let h = match i {
                0..16 => XTERM_16[i as usize],
                16..232 => {
                    let i = (i - 16) as usize;
                    let l = |n: usize| CUBE[n] as u32;
                    l(i / 36) << 16 | l(i / 6 % 6) << 8 | l(i % 6)
                }
                _ => {
                    let g = 8 + 10 * (i as u32 - 232);
                    g << 16 | g << 8 | g
                }
            };
            Some(split(h))
        }
        _ => None,
    };
    as_rgb(c).or_else(|| as_rgb(fallback)).unwrap_or((0, 0, 0))
}

/// A severity key with `*` or `?` is a glob, not a name.
fn is_glob(k: &str) -> bool {
    k.contains(['*', '?'])
}

/// Whether `name` matches `pattern`: `*` any run of characters, `?` any one.
fn glob(pattern: &str, name: &str) -> bool {
    fn go(p: &[char], n: &[char]) -> bool {
        match p.split_first() {
            None => n.is_empty(),
            Some(('*', rest)) => (0..=n.len()).any(|i| go(rest, &n[i..])),
            Some((c, rest)) => n.split_first().is_some_and(|(x, nr)| (*c == '?' || c == x) && go(rest, nr)),
        }
    }
    let (p, n): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    go(&p, &n)
}

/// A colour as written: `#rrggbb`, an ANSI name, `0`–`255`, or `default`.
pub fn parse_colour(v: &Value) -> Result<Color, String> {
    let bad = || {
        let shown = v.as_str().map_or_else(|| v.to_string(), String::from);
        format!("`{shown}` is not a colour (#rrggbb, a name like red, 0–255 or default)")
    };
    let index = |n: u64| u8::try_from(n).map(Color::Indexed).map_err(|_| bad());
    match v {
        Value::Number(n) => n.as_u64().ok_or_else(bad).and_then(index),
        Value::String(s) => {
            let s = s.trim().to_lowercase();
            if s == "default" {
                return Ok(Color::Reset);
            }
            if let Some(h) = s.strip_prefix('#') {
                return match u32::from_str_radix(h, 16) {
                    Ok(n) if h.len() == 6 => Ok(hex(n)),
                    _ => Err(bad()),
                };
            }
            if let Some(i) = ANSI_NAMES.iter().position(|n| *n == s) {
                return Ok(ansi(i as u8));
            }
            s.parse::<u64>().map_err(|_| bad()).and_then(index)
        }
        _ => Err(bad()),
    }
}

/// A hex colour as the nearest of the 256 (the cube or the greys, never the user's 0–15);
/// other colours as they are.
pub fn nearest_256(c: Color) -> Color {
    let Color::Rgb(r, g, b) = c else { return c };
    let dist = |(x, y, z): (u8, u8, u8)| {
        let d = |a: u8, b: u8| (a as i32 - b as i32).pow(2);
        d(r, x) + d(g, y) + d(b, z)
    };
    (16..=255u8).min_by_key(|&i| dist(rgb_or(Color::Indexed(i), Color::Reset))).map_or(c, Color::Indexed)
}

/// The theme `setting` (`theme:` as written; none is charm) names, with theme files from
/// `<config_dir>/themes/`; hex colours become the nearest of the 256 without `truecolor`.
/// Errors are for the startup toast; each falls back to the base theme's value.
pub fn resolve(setting: Option<&Value>, config_dir: Option<&Path>, truecolor: bool) -> (Theme, Vec<String>) {
    let mut errors = vec![];
    for name in file_names(config_dir).iter().filter(|n| is_built_in(n)) {
        errors.push(format!("themes/{name}.yaml: named like a built-in theme; ignored"));
    }
    let mut theme = match setting.filter(|v| !v.is_null()) {
        None => Theme::charm(),
        Some(v) => layer(v, "theme", config_dir, &mut vec![], &mut errors),
    };
    if !truecolor {
        theme.map(nearest_256);
    }
    (theme, errors)
}

/// The themes the picker offers: the built-ins, then theme files A–Z.
pub fn names(config_dir: Option<&Path>) -> Vec<String> {
    let mut out: Vec<String> = BUILT_INS.iter().map(|s| s.to_string()).collect();
    out.extend(file_names(config_dir).into_iter().filter(|n| !is_built_in(n)));
    out
}

/// The names of the `themes/*.yaml` files, A–Z.
fn file_names(config_dir: Option<&Path>) -> Vec<String> {
    let Some(dir) = config_dir.and_then(|d| std::fs::read_dir(d.join("themes")).ok()) else { return vec![] };
    let mut names: Vec<String> =
        dir.flatten().filter_map(|e| e.file_name().to_str()?.strip_suffix(".yaml").map(String::from)).collect();
    names.sort();
    names
}

/// The theme a `theme:` value (or a theme file's body) describes; `at` names it in errors.
/// `seen` holds the theme files being read, to catch a `base:` loop.
fn layer(v: &Value, at: &str, dir: Option<&Path>, seen: &mut Vec<String>, errors: &mut Vec<String>) -> Theme {
    let m = match v {
        Value::String(name) => return named(name, dir, seen, errors),
        Value::Object(m) => m,
        _ => {
            errors.push(format!("{at}: not a theme name or a map of {}", theme_keys()));
            return Theme::charm();
        }
    };
    for k in m.keys().filter(|k| !THEME_KEYS.contains(&k.as_str())) {
        errors.push(format!("{at}: {k}: unknown key"));
    }
    let mut t = match m.get("base") {
        None | Some(Value::Null) => Theme::charm(),
        Some(Value::String(base)) => named(base, dir, seen, errors),
        Some(_) => {
            errors.push(format!("{at}: base: not a theme name"));
            Theme::charm()
        }
    };
    overrides(m, at, &mut t, errors);
    t
}

/// A theme's own `colors`, `severity` and `severity_scale`, over its base.
fn overrides(m: &Map<String, Value>, at: &str, t: &mut Theme, errors: &mut Vec<String>) {
    let map = |key: &str, errors: &mut Vec<String>| match m.get(key) {
        None | Some(Value::Null) => None,
        Some(Value::Object(o)) => Some(o.clone()),
        Some(_) => {
            errors.push(format!("{at}: {key}: not a map"));
            None
        }
    };
    for (k, v) in map("colors", errors).unwrap_or_default() {
        match (t.token(&k), parse_colour(&v)) {
            (None, _) => errors.push(format!("{at}: colors.{k}: unknown colour (one of {})", TOKENS.join(", "))),
            (Some(_), Err(why)) => errors.push(format!("{at}: colors.{k}: {why}")),
            (Some(c), Ok(colour)) => *c = colour,
        }
    }
    let mut own = vec![];
    for (k, v) in map("severity", errors).unwrap_or_default() {
        match parse_colour(&v) {
            Ok(c) => own.push((k.to_lowercase(), Sev::Colour(c))),
            Err(why) => errors.push(format!("{at}: severity.{k}: {why}")),
        }
    }
    if !own.is_empty() {
        t.severity.insert(0, own);
    }
    match m.get("severity_scale") {
        None | Some(Value::Null) => {}
        Some(Value::Array(list)) if !list.is_empty() => {
            match list.iter().map(parse_colour).collect::<Result<Vec<Color>, String>>() {
                Ok(scale) => t.severity_scale = scale,
                Err(why) => errors.push(format!("{at}: severity_scale: {why}")),
            }
        }
        Some(_) => errors.push(format!("{at}: severity_scale: not a list of one or more colours")),
    }
}

/// A theme by name: a built-in, else `themes/<name>.yaml`.
fn named(name: &str, dir: Option<&Path>, seen: &mut Vec<String>, errors: &mut Vec<String>) -> Theme {
    if let Some(t) = Theme::built_in(name) {
        return t;
    }
    let at = format!("themes/{name}.yaml");
    // A file named like a built-in is ignored (resolve says so).
    let file = dir.filter(|_| !is_built_in(name)).and_then(|d| std::fs::read_to_string(d.join(&at)).ok());
    let Some(text) = file else {
        errors.push(format!("theme {name}: no such theme ({}, or a file in themes/)", BUILT_INS.join(", ")));
        return Theme::charm();
    };
    if seen.iter().any(|s| s == name) {
        errors.push(format!("{at}: base: a loop ({} → {name})", seen.join(" → ")));
        return Theme::charm();
    }
    let v = match crate::config::parse_yaml(&text) {
        Ok(v @ Value::Object(_)) => v,
        Ok(_) => {
            errors.push(format!("{at}: not a map of {}", theme_keys()));
            return Theme::charm();
        }
        Err(e) => {
            errors.push(format!("{at}: {e}"));
            return Theme::charm();
        }
    };
    seen.push(name.to_string());
    let t = layer(&v, &at, dir, seen, errors);
    seen.pop();
    t
}
