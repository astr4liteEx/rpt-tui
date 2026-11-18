//! `config.yaml`: chains and the file picker. Errors are collected for the startup toast; the
//! TUI always starts, and a broken chain is left out.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// rpt's built-in verbs; a chain may not take one of these names.
pub const RPT_VERBS: [&str; 10] =
    ["add", "assemble", "delete", "findings", "plugins", "scope", "set", "severities", "validate", "version"];

/// Top-level keys read by later parts of the TUI; accepted here without a complaint.
const LATER_KEYS: [&str; 4] = ["theme", "keys", "history", "notify"];

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

/// What `config.yaml` says, as far as this part of the TUI reads it.
#[derive(Debug, Default)]
pub struct Config {
    /// Valid chains, in file order.
    pub chains: Vec<Chain>,
    /// `file_picker:`: replaces fzf as the file picker.
    pub file_picker: Option<String>,
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
            k if LATER_KEYS.contains(&k) => {}
            k => config.errors.push(format!("config.yaml: unknown key `{k}`")),
        }
    }
    config
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
        key: opt_string(m, "key")?,
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
