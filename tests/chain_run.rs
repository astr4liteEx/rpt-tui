//! A chain's steps as one run in one PTY: headers, stopping on failure, parameters, `RUN_TMP`.
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use rpt_tui::config::Step;
use rpt_tui::run::chain::{Scratch, steps_launch};
use rpt_tui::run::{Run, Size, Status};

fn allowed(run: &str) -> Step {
    Step { run: run.into(), allow_failure: true }
}

/// Runs `steps` to the end; returns the screen, the status and the scratch folder.
fn run(steps: &[Step], env: &[(&str, &str)]) -> (String, Status, Scratch) {
    let scratch = Scratch::new().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let env = env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let launch = steps_launch(steps, env, cwd.path(), &scratch);
    let mut run = Run::spawn(launch, Size { rows: 20, cols: 100 }, Arc::new(AtomicBool::new(false))).unwrap();
    let t0 = Instant::now();
    while run.poll() == Status::Live && t0.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    (run.contents(), run.poll(), scratch)
}

#[test]
fn steps_run_in_order_in_one_run_with_a_header_before_each() {
    let (s, status, _) = run(&[Step::new("echo one"), Step::new("echo two")], &[]);

    assert_eq!(status, Status::Exited(0));
    let lines: Vec<&str> = s.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines, ["── step 1/2: echo one", "one", "── step 2/2: echo two", "two"]);
}

#[test]
fn a_single_step_has_no_header() {
    let (s, status, scratch) = run(&[Step::new("echo alone")], &[]);

    assert_eq!(status, Status::Exited(0));
    assert_eq!(s.trim(), "alone");
    assert_eq!(scratch.failed_step(), None);
}

#[test]
fn the_chain_stops_at_the_first_failing_step_and_names_it() {
    let (s, status, scratch) = run(&[Step::new("true"), Step::new("exit 3"), Step::new("echo never")], &[]);

    assert_eq!(status, Status::Exited(3));
    assert_eq!(scratch.failed_step(), Some(2));
    assert!(!s.contains("never") && !s.contains("step 3/3"), "{s}");
}

#[test]
fn an_allowed_failure_is_noted_and_the_chain_goes_on() {
    let (s, status, scratch) = run(&[allowed("exit 4"), Step::new("echo after")], &[]);

    assert_eq!(status, Status::Exited(0));
    assert_eq!(scratch.failed_step(), None);
    assert!(s.contains("── step 1/2 failed (4), continuing"), "{s}");
    assert!(s.contains("after"), "{s}");
}

#[test]
fn a_step_runs_with_pipefail() {
    let (_, status, scratch) = run(&[Step::new("false | true"), Step::new("echo never")], &[]);

    assert_eq!(status, Status::Exited(1));
    assert_eq!(scratch.failed_step(), Some(1));
}

#[test]
fn parameters_reach_the_steps_as_environment_variables() {
    let (s, _, _) = run(&[Step::new(r#"printf '[%s]\n' "$SCAN" "$EMPTY""#)], &[("SCAN", "a b.nessus"), ("EMPTY", "")]);

    assert_eq!(s.trim(), "[a b.nessus]\n[]");
}

#[test]
fn run_tmp_is_a_folder_for_the_run_deleted_with_its_scratch() {
    let (s, status, scratch) = run(
        &[Step::new(r#"echo staged > "$RUN_TMP/records.jsonl""#), Step::new(r#"cat "$RUN_TMP/records.jsonl""#)],
        &[],
    );
    assert_eq!(status, Status::Exited(0));
    assert!(s.contains("staged"), "{s}");
    let tmp = scratch.run_tmp();
    assert!(tmp.is_dir());

    drop(scratch);

    assert!(!tmp.exists(), "{} survived", tmp.display());
}

#[test]
fn a_step_line_is_shown_and_run_exactly_as_written() {
    let line = r#"printf '%s\n' "it's" '$HOME' "a  b""#;

    let (s, _, _) = run(&[Step::new(line), Step::new("true")], &[]);

    assert!(s.contains(&format!("── step 1/2: {line}")), "{s}");
    assert!(s.contains("it's\n$HOME\na  b"), "{s}");
}

#[test]
fn a_one_step_failure_is_not_named_by_step() {
    let (_, status, scratch) = run(&[Step::new("exit 5")], &[]);

    assert_eq!(status, Status::Exited(5));
    assert_eq!(scratch.failed_step(), None, "marked `✘ 5`, not `✘ step 1: 5`");
}
