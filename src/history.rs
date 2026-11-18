//! Run history: finished runs per report, kept under `history.dir` (never in the report), so
//! later sessions list them. Each run is a folder holding `meta.json` and `transcript.ansi`.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::run::Status;

/// How much history is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Runs kept per report, newest first.
    pub max_runs: usize,
    /// Days a run is kept.
    pub max_days: u64,
    /// Bytes of transcript kept per run: the end of it.
    pub max_transcript: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { max_runs: 200, max_days: 30, max_transcript: crate::run::TRANSCRIPT_CAP }
    }
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "as", rename_all = "lowercase")]
pub enum Ended {
    Exited { code: u32, failed_step: Option<usize> },
    Signalled { signal: String },
    Killed,
}

impl Ended {
    /// How a run with `status` ended; none while it lives.
    pub fn of(status: &Status, failed_step: Option<usize>) -> Option<Ended> {
        match status {
            Status::Live => None,
            Status::Exited(code) => Some(Ended::Exited { code: *code, failed_step }),
            Status::Signalled(signal) => Some(Ended::Signalled { signal: signal.clone() }),
            Status::Killed => Some(Ended::Killed),
        }
    }
}

impl From<&Ended> for Status {
    fn from(e: &Ended) -> Status {
        match e {
            Ended::Exited { code, .. } => Status::Exited(*code),
            Ended::Signalled { signal } => Status::Signalled(signal.clone()),
            Ended::Killed => Status::Killed,
        }
    }
}

/// Milliseconds since the epoch, as history keeps times.
pub fn epoch_ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// What was run, enough to run it again.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invocation {
    /// The palette's id for the command: `chain import`, `rpt scope`, `plugin greet`.
    pub command: String,
    /// A chain's parameter values, in its order.
    pub params: Vec<(String, String)>,
    /// A plugin's or `set`/`delete`'s argument line, as typed.
    pub args: Option<String>,
}

/// What is kept of a finished run besides its transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// The label it had in Runs.
    pub label: String,
    #[serde(flatten)]
    pub invocation: Invocation,
    /// Milliseconds since the epoch.
    pub started_ms: u64,
    /// How long it ran.
    pub duration_ms: u64,
    /// How it ended.
    pub ended: Ended,
    /// It ran taken over rather than in the pane.
    pub takeover: bool,
}

/// A run read back from history; `key` names its folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    /// Its folder's name.
    pub key: String,
    /// What was kept of it.
    pub record: Record,
}

/// What is on disk: the runs read, newest first, and how many folders could not be read.
#[derive(Debug, Default)]
pub struct Listing {
    /// The runs read, newest first.
    pub runs: Vec<Saved>,
    /// Folders whose `meta.json` could not be read.
    pub unreadable: usize,
}

/// One report's history.
#[derive(Debug, Clone)]
pub struct Store {
    /// `<history.dir>/reports/<id>`.
    root: PathBuf,
    limits: Limits,
}

const META: &str = "meta.json";
const TRANSCRIPT: &str = "transcript.ansi";

impl Store {
    /// The history of the report at `report` (an absolute path), under `dir`.
    pub fn open(dir: &Path, report: &Path, limits: Limits) -> io::Result<Store> {
        let root = dir.join("reports").join(report_id(report));
        std::fs::create_dir_all(root.join("runs"))?;
        std::fs::write(root.join("report.txt"), format!("{}\n", report.display()))?;
        Ok(Store { root, limits })
    }

    /// The folder holding one folder per run.
    fn runs(&self) -> PathBuf {
        self.root.join("runs")
    }

    /// Saves a finished run; returns its key. The folder appears whole or not at all.
    pub fn save(&self, record: &Record, transcript: &str) -> io::Result<String> {
        static N: AtomicU64 = AtomicU64::new(0);
        let key = format!("{}-{}-{}", record.started_ms, std::process::id(), N.fetch_add(1, Ordering::Relaxed));
        let tmp = self.runs().join(format!(".{key}"));
        std::fs::create_dir_all(&tmp)?;
        std::fs::write(tmp.join(TRANSCRIPT), crate::run::tail(transcript, self.limits.max_transcript))?;
        std::fs::write(tmp.join(META), serde_json::to_string_pretty(record).map_err(io::Error::other)?)?;
        std::fs::rename(&tmp, self.runs().join(&key))?;
        Ok(key)
    }

    /// The runs on disk, newest first.
    pub fn list(&self) -> Listing {
        let mut listing = Listing::default();
        let Ok(dir) = std::fs::read_dir(self.runs()) else { return listing };
        for entry in dir.flatten() {
            let key = entry.file_name().to_string_lossy().into_owned();
            if key.starts_with('.') {
                continue;
            }
            let meta = std::fs::read_to_string(entry.path().join(META));
            match meta.ok().and_then(|m| serde_json::from_str::<Record>(&m).ok()) {
                Some(record) => listing.runs.push(Saved { key, record }),
                None => listing.unreadable += 1,
            }
        }
        listing.runs.sort_by(|a, b| b.record.started_ms.cmp(&a.record.started_ms).then(b.key.cmp(&a.key)));
        listing
    }

    /// A saved run's coloured transcript.
    pub fn transcript(&self, key: &str) -> io::Result<String> {
        std::fs::read_to_string(self.runs().join(key).join(TRANSCRIPT))
    }

    /// Removes a saved run.
    pub fn delete(&self, key: &str) -> io::Result<()> {
        std::fs::remove_dir_all(self.runs().join(key))
    }

    /// Keeps the newest `max_runs` runs younger than `max_days`; runs that can't be read stay.
    /// Returns the keys of those removed.
    pub fn prune(&self, now: SystemTime) -> Vec<String> {
        let now_ms = epoch_ms(now);
        let oldest = now_ms.saturating_sub(self.limits.max_days.saturating_mul(DAY_MS));
        let mut gone = vec![];
        for (i, r) in self.list().runs.into_iter().enumerate() {
            if (i >= self.limits.max_runs || r.record.started_ms < oldest) && self.delete(&r.key).is_ok() {
                gone.push(r.key);
            }
        }
        gone
    }

    /// Commands used in this report, newest first.
    pub fn recent(&self) -> Vec<String> {
        read_json(&self.root.join(RECENT)).unwrap_or_default()
    }

    /// Notes command `id` as just used.
    pub fn note_used(&self, id: &str) {
        let mut recent = self.recent();
        recent.retain(|r| r != id);
        recent.insert(0, id.to_string());
        recent.truncate(MAX_RECENT);
        let _ = write_json(&self.root.join(RECENT), &recent);
    }

    /// The value last used for a `remember: true` parameter of `chain`.
    pub fn remembered(&self, chain: &str, param: &str) -> Option<String> {
        let all: Remembered = read_json(&self.root.join(REMEMBERED)).unwrap_or_default();
        all.get(chain)?.get(param).cloned()
    }

    /// Keeps `values` as the last used for `chain`'s parameters.
    pub fn remember(&self, chain: &str, values: &[(String, String)]) {
        let path = self.root.join(REMEMBERED);
        let mut all: Remembered = read_json(&path).unwrap_or_default();
        let mine = all.entry(chain.to_string()).or_default();
        for (k, v) in values {
            mine.insert(k.clone(), v.clone());
        }
        let _ = write_json(&path, &all);
    }
}

/// Parameter values by chain, then parameter.
type Remembered = BTreeMap<String, BTreeMap<String, String>>;

const RECENT: &str = "recent.json";
const REMEMBERED: &str = "remembered.json";
const MAX_RECENT: usize = 50;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// A JSON file's value; none when it is missing or can't be read.
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Written beside and renamed over, so a reader never sees half a file.
fn write_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_string(value).map_err(io::Error::other)?)?;
    std::fs::rename(tmp, path)
}

/// The report's folder name and a hash of its path: `report-3f9a1c2b7d4e5f60`.
pub fn report_id(report: &Path) -> String {
    // FNV-1a: stable across Rust versions, unlike std's hasher.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in report.as_os_str().as_encoded_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    let name = report.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    format!("{name}-{h:016x}")
}
