//! `config.yaml`: chains and the file picker. Errors are collected for the startup toast; the
//! TUI always starts, and a broken chain is left out.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// rpt's built-in verbs; a chain may not take one of these names.
pub const RPT_VERBS: [&str; 10] =
    ["add", "assemble", "delete", "findings", "plugins", "scope", "set", "severities", "validate", "version"];

/// Top-level keys read by later parts of the TUI; accepted here without a complaint.
const LATER_KEYS: [&str; 2] = ["theme", "notify"];

const CHAIN_KEYS: [&str; 9] =
    ["description", "key", "params", "needs", "ephemeral", "display", "takeover", "notify", "steps"];

const PARAM_KEYS: [&str; 6] = ["kind", "glob", "options", "default", "optional", "remember"];

/// Where a run opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Display {
    #[default]
    Pane,
    Takeover,
}

/// How a run is taken over: emulated draws its grid full-screen; raw hands it the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TakeoverStyle {
    #[default]
    Emulated,
    Raw,
}

/// What a chain needs selected before it can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Needs {
    Finding,
    Problem,
    Any,
}

/// What a parameter takes, and how the form asks for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamKind {
    Text,
    /// Picked with the file picker, filtered by `glob`.
    File {
        glob: Option<String>,
    },
    Choice {
        options: Vec<String>,
    },
    /// `yes` or `no`.
    Flag,
}

/// A value a chain asks for each time it runs, handed to its steps as an environment variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub kind: ParamKind,
    pub default: Option<String>,
    pub optional: bool,
    pub remember: bool,
}

impl Param {
    /// Required unless it has a default or is optional.
    pub fn required(&self) -> bool {
        self.default.is_none() && !self.optional
    }
}

/// One shell line of a chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub run: String,
    pub allow_failure: bool,
}

impl Step {
    /// A step that must succeed.
    pub fn new(run: &str) -> Step {
        Step { run: run.into(), allow_failure: false }
    }
}

/// A command the user defines: steps run in order within one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain {
    pub name: String,
    pub description: String,
    /// The hotkey, as written; checked and bound with the keys.
    pub key: Option<String>,
    pub params: Vec<Param>,
    pub needs: Option<Needs>,
    pub ephemeral: bool,
    pub display: Display,
    pub takeover: TakeoverStyle,
    pub notify: Option<bool>,
    pub steps: Vec<Step>,
}

/// One entry under `keys:`: an action's name and its keys as written; none means unbound.
/// Names and notation are checked with the bindings, which can say which default stays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySetting {
    pub action: String,
    pub keys: Vec<String>,
}

/// `history:`: where run history is kept, as written (see [`history_dir`]), and how much.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HistorySettings {
    pub dir: Option<PathBuf>,
    pub limits: crate::history::Limits,
}

/// What `config.yaml` says, as far as this part of the TUI reads it.
#[derive(Debug, Default)]
pub struct Config {
    /// Valid chains, in file order.
    pub chains: Vec<Chain>,
    /// `file_picker:`: replaces fzf as the file picker.
    pub file_picker: Option<String>,
    /// `keys:`, in file order.
    pub keys: Vec<KeySetting>,
    /// `history:`.
    pub history: HistorySettings,
    /// One message per problem, for the startup toast.
    pub errors: Vec<String>,
}

/// `$XDG_CONFIG_HOME/rpt-tui/config.yaml`, else `~/.config/rpt-tui/config.yaml`.
pub fn path(xdg_config_home: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    let base = match xdg_config_home.filter(|x| !x.is_empty()) {
        Some(x) => PathBuf::from(x),
        None => PathBuf::from(home.filter(|h| !h.is_empty())?).join(".config"),
    };
    Some(base.join("rpt-tui").join("config.yaml"))
}

/// Where run history goes: `dir` as written, a leading `~/` meaning `home`; without it,
/// `$XDG_STATE_HOME/rpt-tui`, else `~/.local/state/rpt-tui`.
pub fn history_dir(dir: Option<&Path>, xdg_state_home: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    match (dir, xdg_state_home.filter(|x| !x.is_empty())) {
        (Some(d), _) => expand_home(d, home),
        (None, Some(x)) => Some(PathBuf::from(x).join("rpt-tui")),
        (None, None) => expand_home(Path::new("~/.local/state/rpt-tui"), home),
    }
}

/// `path` with a leading `~` meaning `home`; none when it has one and there is no home.
pub fn expand_home(path: &Path, home: Option<&OsStr>) -> Option<PathBuf> {
    match path.strip_prefix("~") {
        Ok(rest) => Some(PathBuf::from(home.filter(|h| !h.is_empty())?).join(rest)),
        Err(_) => Some(path.to_path_buf()),
    }
}

/// Reads the config file; a missing file is an empty config.
pub fn load_file(path: &Path) -> Config {
    match std::fs::read_to_string(path) {
        Ok(text) => load(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
        Err(e) => Config { errors: vec![format!("config.yaml: {e}")], ..Config::default() },
    }
}

/// Parses config text, collecting every problem instead of stopping at the first.
pub fn load(text: &str) -> Config {
    let mut config = Config::default();
    // YAML 1.2 booleans: only `true`/`false`, so a step `yes` or an option `no` stays a word.
    let mut options = serde_saphyr::Options::default();
    options.strict_booleans = true;
    let root: Value = match serde_saphyr::from_str_with_options(text, options) {
        Ok(v) => v,
        Err(e) => {
            let first = e.to_string().lines().next().unwrap_or_default().to_string();
            config.errors.push(format!("config.yaml: {first}"));
            return config;
        }
    };
    let root = match root {
        Value::Null => return config,
        Value::Object(m) => m,
        _ => {
            config.errors.push("config.yaml: not a map of settings".into());
            return config;
        }
    };
    for (key, value) in &root {
        match key.as_str() {
            "chains" => load_chains(value, &mut config),
            "file_picker" => match value {
                Value::String(s) => config.file_picker = Some(s.clone()),
                Value::Null => {}
                _ => config.errors.push("config.yaml: file_picker: not a command line".into()),
            },
            "keys" => load_keys(value, &mut config),
            "history" => load_history(value, &mut config),
            k if LATER_KEYS.contains(&k) => {}
            k => config.errors.push(format!("config.yaml: unknown key `{k}`")),
        }
    }
    config
}

/// `history:`: each bad setting is an error, and keeps its default.
fn load_history(value: &Value, config: &mut Config) {
    let m = match value {
        Value::Object(m) => m,
        Value::Null => return,
        _ => return config.errors.push("config.yaml: history: not a map of settings".into()),
    };
    let h = &mut config.history;
    for (key, v) in m {
        let positive = || v.as_u64().filter(|n| *n > 0);
        let ok = match key.as_str() {
            "dir" => v.as_str().map(|d| h.dir = Some(PathBuf::from(d))).ok_or("not a folder"),
            "max_runs" => positive().map(|n| h.limits.max_runs = n as usize).ok_or("not a positive whole number"),
            "max_days" => positive().map(|n| h.limits.max_days = n).ok_or("not a positive whole number"),
            "max_transcript_mib" => positive()
                .map(|n| h.limits.max_transcript = (n as usize).saturating_mul(1024 * 1024))
                .ok_or("not a positive whole number"),
            _ => Err("unknown key"),
        };
        if let Err(why) = ok {
            config.errors.push(format!("config.yaml: history: {key}: {why}"));
        }
    }
}

fn load_keys(value: &Value, config: &mut Config) {
    let keys = match value {
        Value::Object(m) => m,
        Value::Null => return,
        _ => return config.errors.push("config.yaml: keys: not a map of actions".into()),
    };
    for (action, v) in keys {
        let written = match v {
            Value::Null => Some(vec![]),
            Value::Array(list) => list.iter().map(scalar).collect(),
            v => scalar(v).map(|k| vec![k]),
        };
        match written {
            Some(keys) => config.keys.push(KeySetting { action: action.clone(), keys }),
            None => config.errors.push(format!("config.yaml: keys: {action}: not a key or a list of keys")),
        }
    }
}

fn load_chains(value: &Value, config: &mut Config) {
    let chains = match value {
        Value::Object(m) => m,
        Value::Null => return,
        _ => return config.errors.push("config.yaml: chains: not a map of chains".into()),
    };
    for (name, body) in chains {
        match chain(name, body) {
            Ok(c) => config.chains.push(c),
            Err(why) => config.errors.push(format!("chain {name}: {why}")),
        }
    }
}

fn chain(name: &str, body: &Value) -> Result<Chain, String> {
    let ok_name = !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !ok_name {
        return Err("a name is lowercase letters, digits and hyphens".into());
    }
    if RPT_VERBS.contains(&name) {
        return Err(format!("`{name}` is an rpt built-in"));
    }
    let m = body.as_object().ok_or("not a map of chain keys")?;
    known_keys(m, &CHAIN_KEYS)?;
    let steps = match m.get("steps") {
        Some(Value::Array(steps)) if !steps.is_empty() => steps.iter().map(step).collect::<Result<_, _>>()?,
        Some(Value::Array(_)) => return Err("steps: is empty".into()),
        Some(_) => return Err("steps: not a list".into()),
        None => return Err("steps: missing".into()),
    };
    let params = match m.get("params") {
        None | Some(Value::Null) => vec![],
        Some(Value::Object(p)) => p.iter().map(|(n, v)| param(n, v)).collect::<Result<_, _>>()?,
        Some(_) => return Err("params: not a map of parameters".into()),
    };
    Ok(Chain {
        name: name.into(),
        description: opt_string(m, "description")?.unwrap_or_default(),
        // Kept as written, even when it isn't one value: a bad hotkey costs the chain only its
        // hotkey, found with the keys.
        key: m.get("key").filter(|v| !v.is_null()).map(|v| scalar(v).unwrap_or_else(|| v.to_string())),
        params,
        needs: one_of(m, "needs", &[("finding", Needs::Finding), ("problem", Needs::Problem), ("any", Needs::Any)])?,
        ephemeral: opt_bool(m, "ephemeral")?.unwrap_or(false),
        display: one_of(m, "display", &[("pane", Display::Pane), ("takeover", Display::Takeover)])?.unwrap_or_default(),
        takeover: one_of(m, "takeover", &[("emulated", TakeoverStyle::Emulated), ("raw", TakeoverStyle::Raw)])?
            .unwrap_or_default(),
        notify: opt_bool(m, "notify")?,
        steps,
    })
}

fn step(v: &Value) -> Result<Step, String> {
    match v {
        Value::Object(m) => {
            known_keys(m, &["run", "allow_failure"])?;
            let run = opt_string(m, "run")?.ok_or("a step needs `run`")?;
            Ok(Step { run, allow_failure: opt_bool(m, "allow_failure")?.unwrap_or(false) })
        }
        other => {
            scalar(other).map(|run| Step { run, allow_failure: false }).ok_or("a step is a line or `{ run }`".into())
        }
    }
}

fn param(name: &str, v: &Value) -> Result<Param, String> {
    let env_name = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !env_name {
        return Err(format!("parameter `{name}`: not an environment variable name"));
    }
    let at = |why: String| format!("parameter {name}: {why}");
    let m = v.as_object().ok_or_else(|| at("not a map".into()))?;
    known_keys(m, &PARAM_KEYS).map_err(at)?;
    let default = match (m.get("default"), m.get("kind").and_then(Value::as_str)) {
        // A flag is yes or no; `true`/`false` say the same.
        (Some(Value::Bool(b)), Some("flag")) => Some(if *b { "yes" } else { "no" }.into()),
        (d, _) => d.and_then(scalar),
    };
    let kind = match opt_string(m, "kind").map_err(at)?.as_deref() {
        Some("text") => ParamKind::Text,
        Some("file") => ParamKind::File { glob: opt_string(m, "glob").map_err(at)? },
        Some("flag") => {
            if let Some(d) = default.as_deref().filter(|d| !matches!(*d, "yes" | "no")) {
                return Err(at(format!("a flag's default is yes or no, not `{d}`")));
            }
            ParamKind::Flag
        }
        Some("choice") => {
            let options: Vec<String> = match m.get("options") {
                Some(Value::Array(o)) if !o.is_empty() => o.iter().filter_map(scalar).collect(),
                _ => return Err(at("a choice needs `options`".into())),
            };
            if let Some(d) = default.as_deref().filter(|d| !options.iter().any(|o| o == d)) {
                return Err(at(format!("default `{d}` is not one of the options")));
            }
            ParamKind::Choice { options }
        }
        Some(other) => return Err(at(format!("unknown kind `{other}` (text, file, choice or flag)"))),
        None => return Err(at("`kind` missing (text, file, choice or flag)".into())),
    };
    Ok(Param {
        name: name.into(),
        kind,
        default,
        optional: opt_bool(m, "optional").map_err(at)?.unwrap_or(false),
        remember: opt_bool(m, "remember").map_err(at)?.unwrap_or(false),
    })
}

fn known_keys(m: &Map<String, Value>, known: &[&str]) -> Result<(), String> {
    match m.keys().find(|k| !known.contains(&k.as_str())) {
        Some(k) => Err(format!("unknown key `{k}`")),
        None => Ok(()),
    }
}

/// A scalar as text.
fn scalar(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn opt_string(m: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => scalar(v).map(Some).ok_or(format!("{key}: not text")),
    }
}

fn opt_bool(m: &Map<String, Value>, key: &str) -> Result<Option<bool>, String> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(format!("{key}: not true or false")),
    }
}

fn one_of<T: Copy>(m: &Map<String, Value>, key: &str, values: &[(&str, T)]) -> Result<Option<T>, String> {
    let Some(given) = opt_string(m, key)? else { return Ok(None) };
    match values.iter().find(|(n, _)| *n == given) {
        Some((_, v)) => Ok(Some(*v)),
        None => {
            let names: Vec<&str> = values.iter().map(|(n, _)| *n).collect();
            Err(format!("{key}: `{given}` is not one of {}", names.join(", ")))
        }
    }
}

/// A key as a YAML scalar: bare when it is plain letters, digits and hyphens, else quoted.
fn yaml_key(k: &str) -> String {
    let plain = k.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !matches!(k.to_ascii_lowercase().as_str(), "null" | "true" | "false");
    if plain { k.to_string() } else { format!("'{}'", k.replace('\'', "''")) }
}

/// Keys as a YAML value: one key, a flow list, or `null` for none.
pub fn yaml_keys(keys: &[String]) -> String {
    match keys {
        [] => "null".into(),
        [k] => yaml_key(k),
        ks => format!("[{}]", ks.iter().map(|k| yaml_key(k)).collect::<Vec<_>>().join(", ")),
    }
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn blank(line: &str) -> bool {
    let t = line.trim();
    t.is_empty() || t.starts_with('#')
}

/// `line` is `name:` with nothing after it but a comment.
fn opens_block(line: &str, name: &str) -> bool {
    line.trim_start().strip_prefix(name).and_then(|r| r.strip_prefix(':')).is_some_and(blank)
}

/// The lines after `at` that are inside its block (blank, or indented deeper than `outer`):
/// the end index (exclusive) and the indent of their first entry.
fn block(lines: &[&str], at: usize, outer: usize) -> (usize, Option<usize>) {
    let mut end = at + 1;
    let mut inner = None;
    while end < lines.len() && (blank(lines[end]) || indent(lines[end]) > outer) {
        if !blank(lines[end]) {
            inner.get_or_insert(indent(lines[end]));
        }
        end += 1;
    }
    // Trailing blank lines belong after the block.
    while end > at + 1 && blank(lines[end - 1]) {
        end -= 1;
    }
    (end, inner)
}

/// The text with chain `name`'s `key:` set to `key` (or removed), or `None` when the file's
/// shape can't be edited safely (flow style, a quoted name). The result is read back and must
/// differ only there, which also refuses anchors and merges the edit would change.
pub fn set_chain_key(text: &str, name: &str, key: Option<&str>) -> Option<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let chains = lines.iter().position(|l| indent(l) == 0 && opens_block(l, "chains"))?;
    let (end, ci) = block(&lines, chains, 0);
    let head = (chains + 1..end).find(|&i| Some(indent(lines[i])) == ci && opens_block(lines[i], name))?;
    let (body_end, ki) = block(&lines, head, ci?);
    let ki = ki?;
    let line = key.map(|k| format!("{}key: {}\n", " ".repeat(ki), yaml_key(k)));
    let at = (head + 1..body_end).find(|&i| indent(lines[i]) == ki && lines[i].trim_start().starts_with("key:"));
    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    match (at, line) {
        (Some(i), Some(l)) => out[i] = l,
        (Some(i), None) => {
            out.remove(i);
        }
        (None, Some(l)) => out.insert(head + 1, l),
        (None, None) => {}
    }
    let out = out.concat();
    let mut want = load(text);
    want.chains.iter_mut().find(|c| c.name == name)?.key = key.map(String::from);
    same(&want, &load(&out)).then_some(out)
}

/// The text with `keys:`'s entry for `action` set to `keys` (`Some(&[])` is `null`), or
/// removed with `None`; `keys:` is added at the end when missing. `None` when the file's shape
/// can't be edited safely; the result is read back and must differ only there.
pub fn set_action_keys(text: &str, action: &str, keys: Option<&[String]>) -> Option<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    let entry = |i: usize| keys.map(|k| format!("{}{action}: {}\n", " ".repeat(i), yaml_keys(k)));
    match lines.iter().position(|l| indent(l) == 0 && l.starts_with("keys:")) {
        Some(at) => {
            if !opens_block(lines[at], "keys") {
                return None;
            }
            let (end, inner) = block(&lines, at, 0);
            let ki = inner.unwrap_or(2);
            let found = (at + 1..end).find(|&i| indent(lines[i]) == ki && opens_entry(lines[i], action));
            match found {
                Some(i) => {
                    // The entry's own lines: deeper ones, or a list's `- ` items at its indent.
                    let mut last = i + 1;
                    while last < end
                        && (indent(lines[last]) > ki
                            || indent(lines[last]) == ki && lines[last].trim_start().starts_with("- "))
                    {
                        last += 1;
                    }
                    out.splice(i..last, entry(ki));
                }
                None => {
                    if let Some(l) = entry(ki) {
                        out.insert(end, l);
                    }
                }
            }
        }
        None => {
            if let Some(l) = entry(2) {
                if out.last().is_some_and(|l| !l.ends_with('\n')) {
                    out.push("\n".into());
                }
                out.push(format!("keys:\n{l}"));
            }
        }
    }
    let out = out.concat();
    let mut want = load(text);
    let at = want.keys.iter().position(|s| s.action == action);
    match (at, keys) {
        (Some(i), Some(k)) => want.keys[i].keys = k.to_vec(),
        (Some(i), None) => {
            want.keys.remove(i);
        }
        (None, Some(k)) => want.keys.push(KeySetting { action: action.into(), keys: k.to_vec() }),
        (None, None) => {}
    }
    same(&want, &load(&out)).then_some(out)
}

/// `line` is the entry `name:` (with any value).
fn opens_entry(line: &str, name: &str) -> bool {
    line.trim_start().strip_prefix(name).is_some_and(|r| r.starts_with(':'))
}

fn same(a: &Config, b: &Config) -> bool {
    a.chains == b.chains && a.keys == b.keys && a.file_picker == b.file_picker && a.errors == b.errors
}
