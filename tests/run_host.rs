//! The run host: real children in a PTY, observed through `run::Run`'s screen and status.
mod common;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use rpt_tui::run::{Launch, Run, Size, Status};

fn spec(cwd: &Path, argv: &[&str]) -> Launch {
    Launch::new(argv.iter().map(|s| s.to_string()).collect(), cwd)
}

fn sh(cwd: &Path, script: &str) -> Launch {
    spec(cwd, &["sh", "-c", script])
}

fn start(spec: Launch, rows: u16, cols: u16) -> Run {
    Run::spawn(spec, Size { rows, cols }, Arc::new(AtomicBool::new(false))).unwrap()
}

/// Polls until the run ends or `secs` pass; returns the final status.
fn wait_end(run: &mut Run, secs: u64) -> Status {
    let t0 = Instant::now();
    while run.poll() == Status::Live && t0.elapsed() < Duration::from_secs(secs) {
        std::thread::sleep(Duration::from_millis(20));
    }
    run.poll()
}

/// Polls until the screen contains `text` or `secs` pass.
fn wait_for(run: &mut Run, text: &str, secs: u64) -> String {
    let t0 = Instant::now();
    loop {
        let s = run.contents();
        if s.contains(text) || t0.elapsed() > Duration::from_secs(secs) {
            return s;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_child_s_output_lands_on_the_screen_and_its_exit_code_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let mut run = start(sh(dir.path(), "echo hello from the pty; exit 3"), 10, 60);

    assert_eq!(wait_end(&mut run, 5), Status::Exited(3));
    assert!(run.contents().contains("hello from the pty"), "{}", run.contents());
}

fn has(program: &str) -> bool {
    std::process::Command::new("sh")
        .args(["-c", &format!("command -v {program}")])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn fake_plugin() -> String {
    format!("{}/tests/fixtures/rpt-fake-checklist", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn terminal_queries_are_answered() {
    if !has("python3") {
        eprintln!("skipped: python3 not found");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut s = spec(dir.path(), &["python3", &fake_plugin(), "--probe"]);
    s.colors = rpt_tui::run::Colors { fg: (0xdd, 0xdd, 0xdd), bg: (0x1a, 0x1a, 0x24) };
    let mut run = start(s, 12, 100);

    wait_end(&mut run, 10);

    let screen = run.contents();
    assert!(!screen.contains("NO REPLY"), "{screen}");
    assert!(screen.contains(r"DSR 5n  → b'\x1b[0n'"), "{screen}");
    assert!(screen.contains(r"CPR 6n  → b'\x1b[2;1R'"), "{screen}");
    assert!(screen.contains(r"DA1     → b'\x1b[?62;22c'"), "{screen}");
    assert!(screen.contains(r"DA2     → b'\x1b[>0;10;1c'"), "{screen}");
    assert!(screen.contains(r"OSC 11  → b'\x1b]11;rgb:1a1a/1a1a/2424\x1b\\'"), "{screen}");
}

#[test]
fn the_child_gets_a_colour_terminal_its_folder_and_extra_environment() {
    let dir = tempfile::tempdir().unwrap();
    let mut s =
        sh(dir.path(), r#"echo "term=$TERM colorterm=$COLORTERM report=$RPT_REPORT_DIR"; echo "cwd=$(pwd -P)""#);
    s.env.push(("RPT_REPORT_DIR".into(), "/some/report".into()));
    let mut run = start(s, 10, 120);

    wait_end(&mut run, 5);

    let screen = run.contents();
    assert!(screen.contains("term=xterm-256color colorterm=truecolor report=/some/report"), "{screen}");
    assert!(screen.contains(&format!("cwd={}", dir.path().canonicalize().unwrap().display())), "{screen}");
}

#[test]
fn an_interactive_plugin_is_driven_by_encoded_keys() {
    if !has("python3") {
        eprintln!("skipped: python3 not found");
        return;
    }
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let report = common::fixture();
    let mut run = start(spec(report.path(), &["python3", &fake_plugin()]), 30, 100);
    let press = |run: &Run, code: KeyCode| {
        run.send_key(&KeyEvent::new(code, KeyModifiers::NONE));
        std::thread::sleep(Duration::from_millis(80));
    };

    wait_for(&mut run, "Web-app checklist", 5);
    // Tick the first and third items, confirm; then filter the picker to "sqli" and pick.
    for code in [KeyCode::Char(' '), KeyCode::Down, KeyCode::Down, KeyCode::Char(' '), KeyCode::Enter] {
        press(&run, code);
    }
    wait_for(&mut run, "Pick the scan file", 5);
    for c in "sqli".chars() {
        press(&run, KeyCode::Char(c));
    }
    press(&run, KeyCode::Enter);

    assert_eq!(wait_end(&mut run, 15), Status::Exited(0));
    let screen = run.contents();
    assert!(screen.contains("done: 120 records, 2 checklist items, input findings/sqli-login.md"), "{screen}");
}

fn alive(pid: i32) -> bool {
    std::process::Command::new("kill").args(["-0", &pid.to_string()]).output().is_ok_and(|o| o.status.success())
}

#[test]
fn kill_reaches_grandchildren_and_marks_the_run_killed() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("grandchild.pid");
    let mut run = start(sh(dir.path(), "sleep 600 & echo $! > grandchild.pid; echo started; wait"), 10, 60);
    wait_for(&mut run, "started", 5);
    let grandchild: i32 = std::fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
    assert!(alive(grandchild));

    run.kill();

    assert_eq!(wait_end(&mut run, 5), Status::Killed);
    let t0 = Instant::now();
    while alive(grandchild) && t0.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!alive(grandchild), "grandchild {grandchild} survived the kill");
}

/// Sends an ex command to nvim: ESC on its own first, or nvim reads `ESC :` as alt-`:`.
fn nvim_command(run: &Run, cmd: &str) {
    run.send(b"\x1b".to_vec());
    std::thread::sleep(Duration::from_millis(150));
    run.send(format!(":{cmd}\r").into_bytes());
}

#[test]
fn nvim_starts_without_e1568_redraws_on_resize_and_quits() {
    if !has("nvim") {
        eprintln!("skipped: nvim not found");
        return;
    }
    let report = common::fixture();
    let mut run = start(spec(report.path(), &["nvim", "--clean", "findings/sqli-login.md"]), 20, 90);

    let screen = wait_for(&mut run, "SQL injection", 5);
    std::thread::sleep(Duration::from_millis(300));
    assert!(!run.contents().contains("E1568"), "{screen}");

    run.resize(Size { rows: 30, cols: 120 });
    assert_eq!(run.size(), Size { rows: 30, cols: 120 });
    // nvim draws its last line (the command line) on the new bottom row.
    nvim_command(&run, "echo 'resized-ok'");
    let screen = wait_for(&mut run, "resized-ok", 5);
    let bottom = screen.lines().nth(29).unwrap_or("");
    assert!(bottom.contains("resized-ok"), "row 30 is {bottom:?}:\n{screen}");

    nvim_command(&run, "qa!");
    assert_eq!(wait_end(&mut run, 5), Status::Exited(0));
}

#[test]
fn a_flood_of_output_never_starves_the_reader_of_the_screen() {
    let dir = tempfile::tempdir().unwrap();
    // Three seconds of coloured, wide lines: long enough for an unfair lock to starve readers.
    let mut run = start(
        sh(
            dir.path(),
            r"timeout 3 yes '[1;31mred[0m [38;2;10;200;90mtrue colour[0m ワイド文字 0123456789 0123456789 0123456789'; seq 1 200000",
        ),
        40,
        120,
    );

    let mut worst = Duration::ZERO;
    let t0 = Instant::now();
    while run.poll() == Status::Live && t0.elapsed() < Duration::from_secs(40) {
        let t = Instant::now();
        let _ = run.contents();
        worst = worst.max(t.elapsed());
        std::thread::sleep(Duration::from_millis(16));
    }

    assert_eq!(run.poll(), Status::Exited(0));
    assert!(run.contents().contains("200000"));
    eprintln!("flood: worst screen read {worst:?}");
    // The spike saw 650 ms before the fixes and 23 ms after, in dev.
    assert!(worst < Duration::from_millis(150), "worst screen read took {worst:?}");
}

#[test]
fn huge_insert_line_and_insert_char_counts_stay_fast() {
    let dir = tempfile::tempdir().unwrap();
    let mut run =
        start(sh(dir.path(), r"for i in $(seq 1 20); do printf '\033[30000L\033[30000@x'; done; echo done"), 40, 120);

    let t0 = Instant::now();
    wait_end(&mut run, 30);

    assert!(t0.elapsed() < Duration::from_secs(2), "took {:?}", t0.elapsed());
}

#[test]
fn quiet_counts_from_the_last_output() {
    let dir = tempfile::tempdir().unwrap();
    let mut run = start(sh(dir.path(), "echo first; sleep 0.8; echo second; sleep 5"), 10, 60);

    wait_for(&mut run, "first", 5);
    std::thread::sleep(Duration::from_millis(500));
    assert!(run.quiet_for() >= Duration::from_millis(400), "{:?}", run.quiet_for());
    wait_for(&mut run, "second", 5);
    assert!(run.quiet_for() < Duration::from_millis(300), "{:?}", run.quiet_for());
    run.kill();
}

/// A `Write` the test can read back.
#[derive(Clone, Default)]
struct Captured(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Captured {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

#[test]
fn the_raw_tee_repaints_then_passes_output_through_without_alternate_screen_switches() {
    let dir = tempfile::tempdir().unwrap();
    let mut run =
        start(sh(dir.path(), r"echo before; read go; printf '\033[?1049hafter\033[?1049l\n'; read stop"), 10, 60);
    wait_for(&mut run, "before", 5);
    let out = Captured::default();

    run.start_tee(Box::new(out.clone()));
    run.send(b"go\r".to_vec());
    wait_for(&mut run, "after", 5);
    std::thread::sleep(Duration::from_millis(100));
    run.stop_tee();
    run.send(b"x\r".to_vec());
    wait_end(&mut run, 5);

    let text = out.text();
    assert!(text.starts_with("\x1b[0m\x1b[H\x1b[2J"), "clears first: {text:?}");
    assert!(text.contains("before"), "repaints the screen so far: {text:?}");
    assert!(text.contains("after"), "{text:?}");
    assert!(!text.contains("\x1b[?1049"), "{text:?}");
    assert!(!text.contains("x\r"), "nothing after stop_tee: {text:?}");
}

#[test]
fn queries_go_unanswered_while_teeing_because_the_real_terminal_answers() {
    if !has("python3") {
        eprintln!("skipped: python3 not found");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let probe = r#"
import os, select, sys, tty
tty.setraw(0)
input_ready = lambda: select.select([0], [], [], 0.5)[0]
sys.stdin.buffer.raw.read(1)  # wait for the go byte
os.write(1, b"\x1b[5n")
os.write(1, b"answered\r\n" if input_ready() else b"unanswered\r\n")
"#;
    let mut run = start(spec(dir.path(), &["python3", "-c", probe]), 10, 60);
    std::thread::sleep(Duration::from_millis(300));

    run.start_tee(Box::new(Captured::default()));
    run.send(b"g".to_vec());

    assert_eq!(wait_end(&mut run, 5), Status::Exited(0));
    assert!(run.contents().contains("unanswered"), "{}", run.contents());
}
