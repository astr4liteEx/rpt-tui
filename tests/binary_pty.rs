//! rpt-tui itself in a PTY, hosted by our own run host: what a real terminal would see.
mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use rpt_tui::run::{Launch, Run, Size, Status};

/// What rpt-tui is started with: a test plugin `rpt-t-pane` running `pane`, and `$VISUAL` a
/// stand-in editor named `vi` (so it gets `+LINE`) running `editor`.
#[derive(Default)]
struct Setup<'a> {
    pane: &'a str,
    editor: &'a str,
}

struct Tui {
    run: Run,
    report: tempfile::TempDir,
    _bin: tempfile::TempDir,
}

impl std::ops::Deref for Tui {
    type Target = Run;
    fn deref(&self) -> &Run {
        &self.run
    }
}

impl std::ops::DerefMut for Tui {
    fn deref_mut(&mut self) -> &mut Run {
        &mut self.run
    }
}

/// Starts rpt-tui in a copy of the fixture, in a 24×100 terminal.
fn rpt_tui(setup: Setup) -> Tui {
    let report = common::fixture();
    let bin = tempfile::tempdir().unwrap();
    let describe = r#"if [ "$1" = --describe ]; then echo '{"summary":"the test plugin"}'; exit 0; fi"#;
    common::stand_in_rpt(bin.path(), "rpt-t-pane", &format!("{describe}\n{}\n", setup.pane));
    let editor = common::stand_in_rpt(bin.path(), "vi", &format!("{}\n", setup.editor));
    let argv = vec![env!("CARGO_BIN_EXE_rpt-tui").to_string()];
    let mut spec = Launch::new(argv, report.path());
    let path = format!(
        "{}:{}:{}",
        bin.path().display(),
        common::rpt_dir().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    spec.env.push(("PATH".into(), path));
    spec.env.push(("VISUAL".into(), editor.display().to_string()));
    let run = Run::spawn(spec, Size { rows: 24, cols: 100 }, Arc::new(AtomicBool::new(false))).unwrap();
    Tui { run, report, _bin: bin }
}

/// Starts the test plugin from the palette, once plugins are listed.
fn start_pane_run(tui: &mut Run) {
    until(tui, "1 Findings");
    tui.send(b":");
    until(tui, "the test plugin");
    tui.send(b"t-pane\r");
    until(tui, "rpt t-pane");
    tui.send(b"\r");
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
fn a_plugin_from_the_palette_opens_in_the_pane_and_quitting_restores_the_terminal() {
    let mut tui = rpt_tui(Setup { pane: "echo pane-hello; sleep 30", ..Setup::default() });
    start_pane_run(&mut tui);

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
fn e_edits_the_selected_finding_in_raw_takeover_which_tees_the_child_and_cleans_up_after_it() {
    // The pause lets the tee start first, as an editor's start-up would; output made before
    // then is only replayed as the run's screen, which ?1049l has already blanked.
    let editor = r#"sleep 0.3; printf '\033[?1049h\033[?1h'; echo "raw-hello $*"; printf '\033[?1049l'; read x; echo "got:$x"; sleep 0.3"#;
    let mut tui = rpt_tui(Setup { editor, ..Setup::default() });
    until(&mut tui, "1 Findings");
    tui.send(b"e");

    let s = until(&mut tui, "raw-hello");
    assert!(!s.contains("1 Findings"), "the child has the whole terminal: {s}");
    let file = tui.report.path().join("findings/sqli-login.md");
    assert!(s.contains(&format!("raw-hello {}", file.display())), "the selected finding's file, no line: {s}");
    std::thread::sleep(Duration::from_millis(100));
    assert!(tui.with_screen(|s| s.alternate_screen()), "the child's ?1049l was filtered out");
    assert!(tui.with_screen(|s| s.application_cursor()), "the child's own modes reach the terminal");

    tui.send(b"abc\r");
    assert!(until(&mut tui, "got:abc").contains("got:abc"));

    // The run ends: back home, the child's modes reset, and the ephemeral run gone.
    let s = until(&mut tui, "1 Findings");
    assert!(s.contains("No runs yet."), "{s}");
    assert!(!tui.with_screen(|s| s.application_cursor()), "application cursor mode was reset");

    tui.send(b"q");
    assert_eq!(wait_end(&mut tui), Status::Exited(0));
}

#[test]
fn ctrl_t_leaves_raw_takeover_while_the_run_lives() {
    let mut tui = rpt_tui(Setup { editor: "echo raw-two; sleep 30", ..Setup::default() });
    until(&mut tui, "1 Findings");
    tui.send(b"e");
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
    let mut tui = rpt_tui(Setup { editor: "echo raw-three; read x", ..Setup::default() });
    until(&mut tui, "1 Findings");
    tui.send(b"e");
    until(&mut tui, "raw-three");
    // From here the outer terminal answers no queries (teeing turns our responder off).
    tui.start_tee(Box::new(std::io::sink()));

    tui.send(b"\r");

    let s = until(&mut tui, "1 Findings");
    assert_eq!(tui.poll(), Status::Live, "rpt-tui is still running: {s}");
    assert!(s.contains("1 Findings"), "{s}");
    tui.send(b"q");
    assert_eq!(wait_end(&mut tui), Status::Exited(0));
}

#[test]
fn an_idle_screen_still_shows_a_run_going_quiet() {
    let mut tui = rpt_tui(Setup { pane: "echo waiting-here; sleep 30", ..Setup::default() });
    start_pane_run(&mut tui);
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
    let mut tui = rpt_tui(Setup {
        pane: "trap '' HUP; sleep 600 & echo $! > stubborn.pid; echo stubborn; wait",
        ..Setup::default()
    });
    start_pane_run(&mut tui);
    until(&mut tui, "stubborn");
    let pid: i32 = std::fs::read_to_string(tui.report.path().join("stubborn.pid")).unwrap().trim().parse().unwrap();

    tui.send(CTRL_G);
    std::thread::sleep(Duration::from_millis(100));
    tui.send(b"q");
    assert_eq!(wait_end(&mut tui), Status::Exited(0));

    std::thread::sleep(Duration::from_millis(200));
    assert!(!alive(pid), "sleep {pid} survived quit");
}

#[test]
fn bytes_after_ctrl_t_in_the_same_read_still_reach_the_run() {
    let mut tui = rpt_tui(Setup { editor: r#"echo raw-four; read x; echo "got:$x"; sleep 30"#, ..Setup::default() });
    until(&mut tui, "1 Findings");
    tui.send(b"e");
    until(&mut tui, "raw-four");

    tui.send(b"\x14typed\r");

    let s = until(&mut tui, "got:typed");
    assert!(s.contains("1 Findings") && s.contains("got:typed"), "{s}");
    tui.send(CTRL_G);
    std::thread::sleep(Duration::from_millis(100));
    tui.send(b"q");
    wait_end(&mut tui);
}

#[test]
fn e_on_a_problem_opens_its_file_at_its_line() {
    let mut tui = rpt_tui(Setup { editor: r#"echo "args: $*"; sleep 30"#, ..Setup::default() });
    until(&mut tui, "1 Findings");

    tui.send(b"2e");

    let file = tui.report.path().join("findings/tls-weak.md");
    let s = until(&mut tui, "args:");
    assert!(s.contains(&format!("args: +5 {}", file.display())), "{s}");
    tui.send(CTRL_G);
    std::thread::sleep(Duration::from_millis(100));
    tui.send(b"q");
    wait_end(&mut tui);
}

#[test]
fn an_editor_failing_leaves_a_toast_with_its_code_and_no_run() {
    let mut tui = rpt_tui(Setup { editor: "exit 3", ..Setup::default() });
    until(&mut tui, "1 Findings");

    tui.send(b"e");

    let s = until(&mut tui, "failed: 3");
    assert!(s.contains("edit findings/sqli-login.md failed: 3"), "{s}");
    assert!(s.contains("No runs yet."), "{s}");
    tui.send(b"q");
    assert_eq!(wait_end(&mut tui), Status::Exited(0));
}
