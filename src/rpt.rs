//! Client for the rpt binary: runs its `--json` verbs in the report and parses the records.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

// ---- Report lookup ----
/// The report config file that marks a report root.
pub const REPORT_CONFIG: &str = "rpt.yaml";

/// Returns the report root: `RPT_REPORT_DIR` when given, otherwise the nearest
/// folder at or above `start` holding `rpt.yaml`. `None` when there is no report.
pub fn find_report(start: &Path, report_dir_env: Option<&OsStr>) -> Option<PathBuf> {
    if let Some(dir) = report_dir_env.filter(|d| !d.is_empty()) {
        let dir = PathBuf::from(dir);
        return dir.join(REPORT_CONFIG).is_file().then_some(dir);
    }
    start.ancestors().find(|dir| dir.join(REPORT_CONFIG).is_file()).map(Path::to_path_buf)
}

/// rpt's default severity levels, used when `rpt.yaml` has none.
pub const DEFAULT_SEVERITIES: [&str; 5] = ["critical", "high", "medium", "low", "info"];

/// rpt's default severity levels, owned.
pub fn default_severities() -> Vec<String> {
    DEFAULT_SEVERITIES.iter().map(|s| s.to_string()).collect()
}

/// rpt's fixed sections with their default headings, used when `rpt sections` fails.
pub fn default_sections() -> Vec<Section> {
    [("description", "Description"), ("remediation", "Remediation"), ("poc", "Proof of concept")]
        .into_iter()
        .map(|(key, heading)| Section { key: key.into(), heading: heading.into(), fixed: true, required: true })
        .collect()
}

/// A `location` record from `rpt ls`: one report at or under the folder searched.
#[derive(Debug, Clone, Deserialize)]
pub struct Location {
    /// The report root, absolute.
    pub path: PathBuf,
    /// How many of its finding files parse; absent when its `rpt.yaml` doesn't.
    pub findings: Option<u32>,
}

/// `rpt ls --json` in `dir`: the reports at or under it, in path order. It runs outside any
/// report, so without a [`Client`]. If it takes longer than `timeout`, its process group is
/// killed and the result is [`Error::TimedOut`].
pub fn list_reports(bin: &Path, dir: &Path, timeout: Duration) -> Result<Vec<Location>, Error> {
    use std::os::unix::process::CommandExt;
    let mut child = Command::new(bin)
        .args(["ls", "--json"])
        .process_group(0)
        .current_dir(dir)
        .env_remove("RPT_REPORT_DIR")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(Error::Spawn)?;
    // Read on a thread, so a full pipe can't stall rpt while we wait; the deadline covers the
    // read too, in case something rpt started keeps the pipe open after it exits.
    let mut stdout = child.stdout.take().expect("piped");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut out = String::new();
        let _ = tx.send(stdout.read_to_string(&mut out).map(|_| out));
    });
    let deadline = Instant::now() + timeout;
    let stop = |child: &mut std::process::Child| {
        // The whole group, so nothing rpt started outlives it. SAFETY: killpg only sends a
        // signal; the id is the child's own, positive, group.
        if let Ok(group) = i32::try_from(child.id()) {
            unsafe { libc::killpg(group, libc::SIGKILL) };
        }
        let _ = child.wait();
        Error::TimedOut { verb: "ls".into() }
    };
    let status = loop {
        match child.try_wait().map_err(Error::Spawn)? {
            Some(status) => break status,
            None if Instant::now() >= deadline => return Err(stop(&mut child)),
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    let out = match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(read) => read.map_err(Error::Spawn)?,
        Err(_) => return Err(stop(&mut child)),
    };
    if !status.success() {
        return Err(Error::Failed { verb: "ls".into(), code: status.code(), stderr: String::new() });
    }
    parse_records(&out, "ls", "location")
}

/// The record schema version this rpt-tui understands.
pub const SCHEMA_VERSION: u32 = 1;

/// Finds the `rpt` executable in a `PATH` value, skipping empty and relative entries
/// as rpt's plugin lookup does.
pub fn find_on_path(path: &OsStr) -> Option<PathBuf> {
    which(path, "rpt")
}

/// The first executable `name` in a `PATH` value, skipping empty and relative entries.
pub fn which(path: &OsStr, name: &str) -> Option<PathBuf> {
    std::env::split_paths(path).filter(|dir| dir.is_absolute()).map(|dir| dir.join(name)).find(|bin| is_executable(bin))
}

fn is_executable(p: &Path) -> bool {
    p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// The `version` record: which rpt, record schema and report format.
#[derive(Debug, Clone, Deserialize)]
pub struct Version {
    pub version: String,
    pub schema_version: u32,
    pub format: u32,
}

/// A `finding` record. Members rpt-tui doesn't use ride in `extra`.
#[derive(Debug, Clone, Deserialize)]
pub struct Finding {
    pub id: String,
    pub title: String,
    pub severity: String,
    /// The finding file, relative to the report root.
    pub path: String,
    #[serde(default)]
    pub scope: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
    /// Body sections by name, e.g. `description`, `remediation`, `poc`.
    #[serde(default)]
    pub sections: BTreeMap<String, String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A `severity` record: one level the report allows, with its rank (1 is the most severe).
#[derive(Debug, Clone, Deserialize)]
struct Severity {
    level: String,
    rank: u32,
}

/// A `section` record from `rpt sections`: a section's key (what findings' `sections` are
/// keyed by) and the `##` heading this report gives it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Section {
    pub key: String,
    pub heading: String,
    /// One of the three fixed sections (`description`, `remediation`, `poc`).
    pub fixed: bool,
    /// Every finding must have it, non-empty.
    pub required: bool,
}

/// A `problem` record from `rpt validate`: one broken rule, at a file and maybe a line.
#[derive(Debug, Clone, Deserialize)]
pub struct Problem {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub line: Option<u32>,
    pub rule: String,
    /// `error` or `warning`.
    pub level: String,
    pub message: String,
}

impl Problem {
    pub fn is_error(&self) -> bool {
        self.level == "error"
    }
}

#[derive(Debug)]
pub enum Error {
    /// rpt could not be started.
    Spawn(std::io::Error),
    /// rpt exited with a status that is not a result (2 usage, 126 plugin, or a signal).
    Failed { verb: String, code: Option<i32>, stderr: String },
    /// rpt's stdout was not the records expected.
    BadOutput { verb: String, reason: String },
    /// rpt ran longer than it was given, and was stopped.
    TimedOut { verb: String },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Spawn(e) => write!(f, "could not run rpt: {e}"),
            Error::Failed { verb, code, stderr } => {
                let code = code.map_or("a signal".to_string(), |c| c.to_string());
                write!(f, "rpt {verb} exited {code}: {}", stderr.trim())
            }
            Error::BadOutput { verb, reason } => write!(f, "rpt {verb}: unexpected output: {reason}"),
            Error::TimedOut { verb } => write!(f, "rpt {verb} took too long and was stopped"),
        }
    }
}

impl std::error::Error for Error {}

/// A `plugin` record from `rpt plugins`: the verb, and what its `--describe` said, if anything.
#[derive(Debug, Clone, Deserialize)]
pub struct Plugin {
    pub name: String,
    pub path: PathBuf,
    pub summary: Option<String>,
    pub usage: Option<String>,
    pub input: Option<String>,
    pub output: Option<String>,
}

/// A `verb` record from `rpt verbs`: one of rpt's built-ins, described the way a plugin's
/// `--describe` describes a plugin.
#[derive(Debug, Clone, Deserialize)]
pub struct Verb {
    pub name: String,
    pub summary: String,
    pub usage: String,
}

/// Runs rpt verbs in one report.
#[derive(Debug, Clone)]
pub struct Client {
    bin: PathBuf,
    report: PathBuf,
    /// `PATH` for rpt and for runs: this rpt's folder first, then the base `PATH`.
    path: OsString,
}

impl Client {
    /// A client for `bin` in `report`, with the process's `PATH` as the base.
    pub fn new(bin: impl Into<PathBuf>, report: impl Into<PathBuf>) -> Self {
        let base = std::env::var_os("PATH").unwrap_or_default();
        Client { bin: bin.into(), report: report.into(), path: OsString::new() }.with_path(&base)
    }

    /// Replaces the base `PATH` (where rpt looks for plugins); this rpt's folder stays first.
    pub fn with_path(mut self, base: &OsStr) -> Self {
        let dir = self.bin.parent().map(Path::to_path_buf).unwrap_or_default();
        let dirs = std::iter::once(dir).chain(std::env::split_paths(base));
        self.path = std::env::join_paths(dirs).unwrap_or_else(|_| base.to_os_string());
        self
    }

    /// The `PATH` rpt and runs get, so `rpt` in a run is this rpt.
    pub fn path(&self) -> &OsStr {
        &self.path
    }

    pub fn report(&self) -> &Path {
        &self.report
    }

    /// `rpt plugins`: every plugin on `PATH`, in rpt's order.
    pub fn plugins(&self) -> Result<Vec<Plugin>, Error> {
        self.records("plugins", "plugin")
    }

    pub fn version(&self) -> Result<Version, Error> {
        let out = self.run(&["version", "--json"])?;
        serde_json::from_str(out.trim()).map_err(|e| Error::BadOutput { verb: "version".into(), reason: e.to_string() })
    }

    pub fn findings(&self) -> Result<Vec<Finding>, Error> {
        self.records("findings", "finding")
    }

    /// `rpt validate`: the report's problems (rpt exits 1 when there are errors; that is a result).
    pub fn problems(&self) -> Result<Vec<Problem>, Error> {
        self.records("validate", "problem")
    }

    /// `rpt severities`: the report's levels, most severe first.
    pub fn severities(&self) -> Result<Vec<String>, Error> {
        let mut levels: Vec<Severity> = self.records("severities", "severity")?;
        levels.sort_by_key(|s| s.rank);
        Ok(levels.into_iter().map(|s| s.level).collect())
    }

    /// `rpt sections`: the fixed sections with this report's headings, then the custom ones it
    /// requires, in rpt's order.
    pub fn sections(&self) -> Result<Vec<Section>, Error> {
        self.records("sections", "section")
    }

    /// `rpt verbs`: a record for each of rpt's built-in verbs, in name order.
    pub fn verbs(&self) -> Result<Vec<Verb>, Error> {
        self.records("verbs", "verb")
    }

    /// Runs `rpt <verb> --json` and parses each line whose `kind` is `kind`; other kinds are skipped.
    fn records<T: serde::de::DeserializeOwned>(&self, verb: &str, kind: &str) -> Result<Vec<T>, Error> {
        let out = self.run(&[verb, "--json"])?;
        parse_records(&out, verb, kind)
    }

    /// Runs `rpt <args>` in the report and returns stdout. Exit 0 and 1 are results
    /// (1 means problems in the report); anything else is an error.
    fn run(&self, args: &[&str]) -> Result<String, Error> {
        let out = Command::new(&self.bin)
            .args(args)
            .current_dir(&self.report)
            .env("RPT_REPORT_DIR", &self.report)
            .env("PATH", &self.path)
            .output()
            .map_err(Error::Spawn)?;
        match out.status.code() {
            Some(0 | 1) => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
            code => Err(Error::Failed {
                verb: args.first().unwrap_or(&"").to_string(),
                code,
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            }),
        }
    }
}

/// Parses each JSON Lines record of `verb`'s output whose `kind` is `kind`; other kinds are skipped.
fn parse_records<T: serde::de::DeserializeOwned>(out: &str, verb: &str, kind: &str) -> Result<Vec<T>, Error> {
    let bad = |reason: String| Error::BadOutput { verb: verb.into(), reason };
    let mut records = vec![];
    for (n, line) in out.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
        let value: serde_json::Value = serde_json::from_str(line).map_err(|e| bad(format!("line {}: {e}", n + 1)))?;
        if value.get("kind").and_then(|k| k.as_str()) == Some(kind) {
            records.push(serde_json::from_value(value).map_err(|e| bad(format!("line {}: {e}", n + 1)))?);
        }
    }
    Ok(records)
}
