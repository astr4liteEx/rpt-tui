//! A run's transcript: rows evicted from scrollback, then the scrollback, then the screen,
//! with colours, capped to its end.
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use rpt_tui::run::{Launch, Run, Size, Status, plain};

fn finished(cwd: &Path, script: &str, cap: Option<usize>) -> Run {
    let mut launch = Launch::new(vec!["sh".into(), "-c".into(), script.into()], cwd);
    if let Some(cap) = cap {
        launch.transcript_cap = cap;
    }
    let mut run = Run::spawn(launch, Size { rows: 5, cols: 40 }, Arc::new(AtomicBool::new(false))).unwrap();
    let t0 = Instant::now();
    while run.poll() == Status::Live && t0.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(20));
    }
    run
}

fn lines(transcript: &str) -> Vec<String> {
    plain(transcript).lines().map(str::to_string).filter(|l| !l.is_empty()).collect()
}

#[test]
fn every_line_is_in_the_transcript_once_and_in_order() {
    let dir = tempfile::tempdir().unwrap();
    // More lines than the scrollback keeps, so the first ones were evicted.
    let run = finished(dir.path(), "seq 1 6000", None);

    let got = lines(&run.transcript());

    let want: Vec<String> = (1..=6000).map(|n| n.to_string()).collect();
    assert_eq!(got.len(), want.len(), "first {:?} last {:?}", got.first(), got.last());
    assert_eq!(got, want);
}

#[test]
fn colours_are_kept_and_plain_text_drops_them() {
    let dir = tempfile::tempdir().unwrap();
    let run = finished(dir.path(), r"printf '\033[31mred\033[0m plain\n'", None);

    let t = run.transcript();

    assert!(t.contains("\x1b[31m") && t.contains("red"), "{t:?}");
    assert_eq!(lines(&t), ["red plain"]);
}

#[test]
fn the_transcript_keeps_only_its_last_bytes_from_a_line_start() {
    let dir = tempfile::tempdir().unwrap();
    let run = finished(dir.path(), "seq 1 20000", Some(1000));

    let t = run.transcript();

    assert!(t.len() <= 1000, "{}", t.len());
    let got = lines(&t);
    assert_eq!(got.last().map(String::as_str), Some("20000"));
    let first: usize = got[0].parse().unwrap();
    let want: Vec<String> = (first..=20000).map(|n| n.to_string()).collect();
    assert_eq!(got, want, "whole lines, none missing");
}

#[test]
fn full_screen_programs_leave_only_what_they_print_after() {
    let dir = tempfile::tempdir().unwrap();
    let run =
        finished(dir.path(), r"echo before; printf '\033[?1049h'; seq 101 140; printf '\033[?1049l'; echo after", None);

    assert_eq!(lines(&run.transcript()), ["before", "after"]);
}
