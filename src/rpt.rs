//! Client for the rpt binary: runs its `--json` verbs in the report and parses the records.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fmt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// The record schema version this rpt-tui understands.
pub const SCHEMA_VERSION: u32 = 1;

/// Finds the `rpt` executable in a `PATH` value, skipping empty and relative entries
/// as rpt's plugin lookup does.
pub fn find_on_path(path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("rpt"))
        .find(|bin| is_executable(bin))
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
        }
    }
}

impl std::error::Error for Error {}

/// Runs rpt verbs in one report.
pub struct Client {
    bin: PathBuf,
    report: PathBuf,
}

impl Client {
    pub fn new(bin: impl Into<PathBuf>, report: impl Into<PathBuf>) -> Self {
        Client { bin: bin.into(), report: report.into() }
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

    /// Runs `rpt <verb> --json` and parses each line whose `kind` is `kind`; other kinds are skipped.
    fn records<T: serde::de::DeserializeOwned>(&self, verb: &str, kind: &str) -> Result<Vec<T>, Error> {
        let out = self.run(&[verb, "--json"])?;
        let bad = |reason: String| Error::BadOutput { verb: verb.into(), reason };
        let mut records = vec![];
        for (n, line) in out.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
            let value: serde_json::Value =
                serde_json::from_str(line).map_err(|e| bad(format!("line {}: {e}", n + 1)))?;
            if value.get("kind").and_then(|k| k.as_str()) == Some(kind) {
                records.push(serde_json::from_value(value).map_err(|e| bad(format!("line {}: {e}", n + 1)))?);
            }
        }
        Ok(records)
    }

    /// Runs `rpt <args>` in the report and returns stdout. Exit 0 and 1 are results
    /// (1 means problems in the report); anything else is an error.
    fn run(&self, args: &[&str]) -> Result<String, Error> {
        let out = Command::new(&self.bin)
            .args(args)
            .current_dir(&self.report)
            .env("RPT_REPORT_DIR", &self.report)
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
