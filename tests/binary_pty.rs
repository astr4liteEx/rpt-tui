//! rpt-tui itself in a PTY, hosted by our own run host: what a real terminal would see.
mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use rpt_tui::run::{Launch, Run, Size, Status};

/// Starts rpt-tui in a copy of the fixture with `args`, in a 24×100 terminal.
fn rpt_tui(args: &[&str]) -> (Run, tempfile::TempDir) {
    let report = common::fixture();
    let mut argv = vec![env!("CARGO_BIN_EXE_rpt-tui").to_string()];
    argv.extend(args.iter().map(|s| s.to_string()));
    let mut spec = Launch::new(argv, report.path());
    let path = format!("{}:{}", common::rpt_dir().display(), std::env::var("PATH").unwrap_or_default());
    spec.env.push(("PATH".into(), path));
    let run = Run::spawn(spec, Size { rows: 24, cols: 100 }, Arc::new(AtomicBool::new(false))).unwrap();
    (run, report)
}

fn until(run: &mut Run, text: &str) -> String {
    let t0 = Instant::now();
    loop {
        run.poll();
        let s = run.contents();
        if s.contains(text) || t0.elapsed() > Duration::from_secs(8) {
            return s;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn wait_end(run: &mut Run) -> Status {
    let t0 = Instant::now();
    while run.poll() == Status::Live && t0.elapsed() < Duration::from_secs(8) {
        std::thread::sleep(Duration::from_millis(30));
    }
    run.poll()
}

const CTRL_G: &[u8] = b"\x07";
const CTRL_T: &[u8] = b"\x14";

#[test]
fn a_dev_run_opens_in_the_pane_and_quitting_restores_the_terminal() {
    let (mut tui, _report) = rpt_tui(&["--dev-run", "echo pane-hello; sleep 30"]);

    let s = until(&mut tui, "pane-hello");
    assert!(s.contains("1 Findings") && s.contains("● live"), "{s}");
    assert!(tui.with_screen(|s| s.alternate_screen()));

    tui.send(CTRL_G);
    std::thread::sleep(Duration::from_millis(100));
    tui.send(b"q");

    assert_eq!(wait_end(&mut tui), Status::Exited(0));
    assert!(!tui.with_screen(|s| s.alternate_screen()), "left the alternate screen");
}

#[test]
fn raw_takeover_tees_the_child_passes_stdin_and_cleans_up_after_it() {
    let script =
        r#"printf '\033[?1049h\033[?1h'; echo raw-hello; printf '\033[?1049l'; read x; echo "got:$x"; sleep 0.3"#;
    let (mut tui, _report) = rpt_tui(&["--dev-run-raw", script]);

    let s = until(&mut tui, "raw-hello");
    assert!(!s.contains("1 Findings"), "the child has the whole terminal: {s}");
    std::thread::sleep(Duration::from_millis(100));
    assert!(tui.with_screen(|s| s.alternate_screen()), "the child's ?1049l was filtered out");
    assert!(tui.with_screen(|s| s.application_cursor()), "the child's own modes reach the terminal");

    tui.send(b"abc\r");
    assert!(until(&mut tui, "got:abc").contains("got:abc"));

    // The run ends: back to the pane, the child's modes reset.
    let s = until(&mut tui, "✔ 0");
    assert!(s.contains("1 Findings") && s.contains("✔ 0"), "{s}");
    assert!(!tui.with_screen(|s| s.application_cursor()), "application cursor mode was reset");

    tui.send(b"q");
    assert_eq!(wait_end(&mut tui), Status::Exited(0));
}

#[test]
fn ctrl_t_leaves_raw_takeover_while_the_run_lives() {
    let (mut tui, _report) = rpt_tui(&["--dev-run-raw", "echo raw-two; sleep 30"]);
    until(&mut tui, "raw-two");

    tui.send(CTRL_T);

    let s = until(&mut tui, "1 Findings");
    assert!(s.contains("1 Findings") && s.contains("● live") && s.contains("raw-two"), "{s}");
    tui.send(CTRL_G);
    std::thread::sleep(Duration::from_millis(100));
    tui.send(b"q");
    assert_eq!(wait_end(&mut tui), Status::Exited(0));
}

#[test]
fn leaving_raw_takeover_needs_no_reply_from_the_terminal() {
    let (mut tui, _report) = rpt_tui(&["--dev-run-raw", "echo raw-three; read x"]);
    until(&mut tui, "raw-three");
    // From here the outer terminal answers no queries (teeing turns our responder off).
    tui.start_tee(Box::new(std::io::sink()));

    tui.send(b"\r");

    let s = until(&mut tui, "✔ 0");
    assert_eq!(tui.poll(), Status::Live, "rpt-tui is still running: {s}");
    assert!(s.contains("1 Findings") && s.contains("✔ 0"), "{s}");
    tui.send(b"q");
    assert_eq!(wait_end(&mut tui), Status::Exited(0));
}

#[test]
fn an_idle_screen_still_shows_a_run_going_quiet() {
    let (mut tui, _report) = rpt_tui(&["--dev-run", "echo waiting-here; sleep 30"]);
    until(&mut tui, "waiting-here");

    // No input at all: the redraw has to come from the TUI itself.
    let s = until(&mut tui, "◆ quiet 3s");

    assert!(s.contains("◆ quiet 3s"), "{s}");
    tui.send(CTRL_G);
    std::thread::sleep(Duration::from_millis(100));
    tui.send(b"q");
    wait_end(&mut tui);
}

fn alive(pid: i32) -> bool {
    std::process::Command::new("kill").args(["-0", &pid.to_string()]).output().is_ok_and(|o| o.status.success())
}

#[test]
fn quitting_kills_runs_that_ignore_sighup() {
    let (mut tui, report) =
        rpt_tui(&["--dev-run", "trap '' HUP; sleep 600 & echo $! > stubborn.pid; echo stubborn; wait"]);
    until(&mut tui, "stubborn");
    let pid: i32 = std::fs::read_to_string(report.path().join("stubborn.pid")).unwrap().trim().parse().unwrap();

    tui.send(CTRL_G);
    std::thread::sleep(Duration::from_millis(100));
    tui.send(b"q");
    assert_eq!(wait_end(&mut tui), Status::Exited(0));

    std::thread::sleep(Duration::from_millis(200));
    assert!(!alive(pid), "sleep {pid} survived quit");
}

#[test]
fn bytes_after_ctrl_t_in_the_same_read_still_reach_the_run() {
    let (mut tui, _report) = rpt_tui(&["--dev-run-raw", r#"echo raw-four; read x; echo "got:$x"; sleep 30"#]);
    until(&mut tui, "raw-four");

    tui.send(b"\x14typed\r");

    let s = until(&mut tui, "got:typed");
    assert!(s.contains("1 Findings") && s.contains("got:typed"), "{s}");
    tui.send(CTRL_G);
    std::thread::sleep(Duration::from_millis(100));
    tui.send(b"q");
    wait_end(&mut tui);
}
