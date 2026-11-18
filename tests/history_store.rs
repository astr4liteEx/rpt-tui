//! Run history on disk: one store per report under `history.dir`, read back by later sessions.
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rpt_tui::history::{Ended, Invocation, Limits, Record, Store};

const DAY: u64 = 24 * 60 * 60 * 1000;

/// A finished run started at `ms` (milliseconds since the epoch).
fn record(label: &str, ms: u64) -> Record {
    Record {
        label: label.into(),
        invocation: Invocation {
            command: format!("chain {label}"),
            params: vec![("SCAN".into(), "a.nessus".into())],
            args: None,
        },
        started_ms: ms,
        duration_ms: 2000,
        ended: Ended::Exited { code: 0, failed_step: None },
        takeover: false,
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
}

fn store(dir: &Path, report: &Path) -> Store {
    Store::open(dir, report, Limits::default()).unwrap()
}

#[test]
fn a_saved_run_is_read_back_by_a_later_session() {
    let (dir, report) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let rec = record("import", now_ms());
    let key = store(dir.path(), report.path()).save(&rec, "\x1b[31mred\x1b[m line\nsecond").unwrap();

    let later = store(dir.path(), report.path());
    let listing = later.list();

    assert_eq!(listing.unreadable, 0);
    assert_eq!(listing.runs.len(), 1);
    assert_eq!(listing.runs[0].key, key);
    assert_eq!(listing.runs[0].record, rec);
    assert_eq!(later.transcript(&key).unwrap(), "\x1b[31mred\x1b[m line\nsecond");
}

#[test]
fn runs_are_listed_newest_first() {
    let (dir, report) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let s = store(dir.path(), report.path());
    let t = now_ms();
    s.save(&record("middle", t - 2000), "").unwrap();
    s.save(&record("newest", t), "").unwrap();
    s.save(&record("oldest", t - 4000), "").unwrap();

    let labels: Vec<String> = s.list().runs.into_iter().map(|r| r.record.label).collect();

    assert_eq!(labels, ["newest", "middle", "oldest"]);
}

#[test]
fn each_report_has_its_own_history_named_after_its_folder() {
    let dir = tempfile::tempdir().unwrap();
    let reports = tempfile::tempdir().unwrap();
    let (a, b) = (reports.path().join("acme"), reports.path().join("other").join("acme"));
    store(dir.path(), &a).save(&record("in-a", now_ms()), "").unwrap();

    assert!(store(dir.path(), &b).list().runs.is_empty(), "same folder name, different report");
    let id = rpt_tui::history::report_id(&a);
    assert!(id.starts_with("acme-") && id.len() == "acme-".len() + 16, "{id}");
    assert_ne!(id, rpt_tui::history::report_id(&b));
    let beside = std::fs::read_to_string(dir.path().join("reports").join(&id).join("report.txt")).unwrap();
    assert_eq!(beside.trim(), a.display().to_string());
}

#[test]
fn pruning_keeps_the_newest_max_runs_younger_than_max_days() {
    let (dir, report) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let s = Store::open(dir.path(), report.path(), Limits { max_runs: 2, max_days: 3, ..Limits::default() }).unwrap();
    let t = now_ms();
    for (label, at) in [("old", t - 4 * DAY), ("a", t - 3000), ("b", t - 2000), ("c", t - 1000)] {
        s.save(&record(label, at), "").unwrap();
    }

    s.prune(SystemTime::now());

    let labels: Vec<String> = s.list().runs.into_iter().map(|r| r.record.label).collect();
    assert_eq!(labels, ["c", "b"]);
    let s = Store::open(dir.path(), report.path(), Limits { max_runs: 10, max_days: 3, ..Limits::default() }).unwrap();
    s.save(&record("old", t - 4 * DAY), "").unwrap();
    s.prune(SystemTime::now());
    assert_eq!(s.list().runs.len(), 2, "older than max_days goes even under max_runs");
}

#[test]
fn a_transcript_over_the_cap_keeps_its_end_from_a_line_start() {
    let (dir, report) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let s = Store::open(dir.path(), report.path(), Limits { max_transcript: 20, ..Limits::default() }).unwrap();

    let key = s.save(&record("long", now_ms()), "first line\nsecond line\nthird").unwrap();

    assert_eq!(s.transcript(&key).unwrap(), "second line\nthird");
}

#[test]
fn a_run_that_cannot_be_read_is_skipped_counted_and_kept() {
    let (dir, report) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let s = store(dir.path(), report.path());
    let key = s.save(&record("good", now_ms()), "").unwrap();
    let runs = dir.path().join("reports").join(rpt_tui::history::report_id(report.path())).join("runs");
    std::fs::create_dir(runs.join("123-1-0")).unwrap();
    std::fs::write(runs.join("123-1-0").join("meta.json"), "{ not json").unwrap();

    let listing = s.list();
    s.prune(SystemTime::now() + Duration::from_secs(365 * 24 * 3600));

    assert_eq!((listing.runs.len(), listing.unreadable), (1, 1));
    assert_eq!(listing.runs[0].key, key);
    assert!(runs.join("123-1-0").exists(), "nothing unreadable is deleted");
}

#[test]
fn a_run_can_be_deleted() {
    let (dir, report) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let s = store(dir.path(), report.path());
    let t = now_ms();
    let a = s.save(&record("a", t), "").unwrap();
    s.save(&record("b", t - 1000), "").unwrap();
    s.save(&record("c", t - 2000), "").unwrap();

    s.delete(&a).unwrap();
    assert_eq!(s.list().runs.len(), 2);
    assert!(s.transcript(&a).is_err());
}

#[test]
fn recent_commands_are_newest_first_once_each_up_to_fifty() {
    let (dir, report) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let s = store(dir.path(), report.path());
    for i in 0..60 {
        s.note_used(&format!("chain c{i}"));
    }
    s.note_used("chain c55");

    let recent = store(dir.path(), report.path()).recent();

    assert_eq!(recent.len(), 50);
    assert_eq!(&recent[..3], ["chain c55", "chain c59", "chain c58"]);
}

#[test]
fn remembered_values_are_kept_per_chain_and_parameter() {
    let (dir, report) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let s = store(dir.path(), report.path());
    s.remember("import", &[("SCAN".into(), "a.nessus".into())]);
    s.remember("other", &[("SCAN".into(), "b.nessus".into())]);
    s.remember("import", &[("STRICT".into(), "yes".into())]);

    let later = store(dir.path(), report.path());

    assert_eq!(later.remembered("import", "SCAN").as_deref(), Some("a.nessus"));
    assert_eq!(later.remembered("import", "STRICT").as_deref(), Some("yes"));
    assert_eq!(later.remembered("other", "SCAN").as_deref(), Some("b.nessus"));
    assert_eq!(later.remembered("nope", "SCAN"), None);
}

#[test]
fn a_cut_transcript_keeps_the_colour_in_force_where_it_was_cut() {
    let (dir, report) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let s = Store::open(dir.path(), report.path(), Limits { max_transcript: 30, ..Limits::default() }).unwrap();

    let key = s.save(&record("red", now_ms()), "\x1b[31mfirst red line\nsecond red line\nthird").unwrap();

    let t = s.transcript(&key).unwrap();
    assert!(t.starts_with("\x1b[31m") && t.ends_with("second red line\nthird"), "{t:?}");
}
