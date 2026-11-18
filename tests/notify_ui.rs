//! Telling run ends in the app: toasts, "new" dots, desktop notifications, the bell and the
//! user's command.
mod common;

use common::AppScreen;

/// Chains `ok` (A) and `bad` (B, failing at step 2), each taking half a second.
const CHAINS: &str =
    "chains:\n  ok:\n    key: A\n    steps: ['sleep 0.5']\n  bad:\n    key: B\n    steps: ['sleep 0.5', 'exit 1']\n";

fn screen(more: &str) -> AppScreen {
    AppScreen::new(&format!("{CHAINS}{more}"), tempfile::tempdir().unwrap())
}

/// Starts chain `key` and looks away: keys back, then Findings.
fn start_unviewed(s: &mut AppScreen, key: &str) {
    s.typ(key);
    s.draw();
    s.ctrl('g');
    s.typ("1");
}

/// Draws until no run is live.
fn until_ended(s: &mut AppScreen) -> String {
    let t0 = std::time::Instant::now();
    loop {
        s.draw();
        if !s.app.any_live() || t0.elapsed() > std::time::Duration::from_secs(8) {
            return s.draw();
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn a_run_ending_unviewed_toasts_for_four_seconds() {
    let mut s = screen("");

    start_unviewed(&mut s, "A");
    let out = until_ended(&mut s);
    assert!(out.contains("✔ ok done in 0s"), "{out}");

    s.typ("j");
    s.advance(3);
    assert!(s.draw().contains("✔ ok done"), "keys don't clear it");
    s.advance(2);
    assert!(!s.draw().contains("✔ ok done"));
}

#[test]
fn a_failure_toasts_for_ten_seconds_naming_the_step() {
    let mut s = screen("");

    start_unviewed(&mut s, "B");
    let out = until_ended(&mut s);
    assert!(out.contains("✘ bad failed at step 2 (exit 1)"), "{out}");

    s.advance(9);
    assert!(s.draw().contains("✘ bad failed"));
    s.advance(2);
    assert!(!s.draw().contains("✘ bad failed"));
}

#[test]
fn toasts_stack_three_at_most_newest_last() {
    let mut s = screen("");
    for _ in 0..2 {
        start_unviewed(&mut s, "A");
        until_ended(&mut s);
        start_unviewed(&mut s, "B");
        until_ended(&mut s);
    }

    let out = s.draw();
    let toasts: Vec<usize> = ["✔ ok done", "✘ bad failed"]
        .iter()
        .flat_map(|t| out.lines().enumerate().filter(move |(_, l)| l.contains(t)).map(|(i, _)| i))
        .collect();
    assert_eq!(toasts.len(), 3, "{out}");
    let last = out.lines().collect::<Vec<_>>().iter().rposition(|l| l.contains("✘ bad failed")).unwrap();
    assert_eq!(Some(&last), toasts.iter().max(), "the newest is lowest:\n{out}");
}

#[test]
fn a_run_ending_in_view_raises_no_toast_and_no_dot() {
    let mut s = screen("");

    s.typ("A");
    let out = until_ended(&mut s);

    assert!(!out.contains("✔ ok done"), "{out}");
    assert!(!out.contains("new"), "{out}");
}

#[test]
fn unviewed_ends_get_new_dots_counted_in_the_title_until_viewed() {
    let mut s = screen("");

    start_unviewed(&mut s, "A");
    until_ended(&mut s);
    start_unviewed(&mut s, "B");
    let out = until_ended(&mut s);
    assert!(out.contains("3 Runs · 2 new · 1 ✘"), "{out}");
    let dots = |out: &str| out.matches("● ✔").count() + out.matches("● ✘").count();
    assert_eq!(dots(&out), 2, "{out}");

    s.typ("3");
    s.draw();
    let out = s.draw();
    assert!(out.contains("3 Runs · 1 new ─"), "the selected run is seen:\n{out}");
    s.typ("j");
    s.draw();
    let out = s.draw();
    assert!(!out.contains(" new") && dots(&out) == 0, "{out}");
}

#[test]
fn a_long_unwatched_run_notifies_the_desktop_and_rings_when_asked() {
    let mut s = screen("notify: { after_secs: 0, bell: true }\n");

    start_unviewed(&mut s, "A");
    until_ended(&mut s);

    assert_eq!(s.terminal_output(), b"\x1b]9;\xe2\x9c\x94 ok done in 0s\x1b\\\x07");
}

#[test]
fn a_run_in_view_notifies_only_while_the_terminal_is_unfocused() {
    let mut s = screen("notify: { after_secs: 0 }\n");

    s.typ("A");
    until_ended(&mut s);
    assert!(s.terminal_output().is_empty(), "watched");

    s.focus(false);
    s.typ("A");
    until_ended(&mut s);
    assert!(!s.terminal_output().is_empty(), "in view, but the terminal is elsewhere");
}

#[test]
fn short_runs_and_desktop_off_stay_quiet() {
    let mut s = screen("");
    start_unviewed(&mut s, "A");
    until_ended(&mut s);
    assert!(s.terminal_output().is_empty(), "shorter than after_secs");

    let mut s = screen("notify: { after_secs: 0, desktop: false }\n");
    start_unviewed(&mut s, "A");
    until_ended(&mut s);
    assert!(s.terminal_output().is_empty());
}

#[test]
fn a_chains_notify_overrides_the_rule() {
    let config = "notify: { after_secs: 0 }\nchains:\n  hush:\n    key: H\n    notify: false\n    steps: ['sleep 0.5']\n  shout:\n    key: S\n    notify: true\n    steps: ['true']\n";
    let mut s = AppScreen::new(config, tempfile::tempdir().unwrap());

    start_unviewed(&mut s, "H");
    until_ended(&mut s);
    assert!(s.terminal_output().is_empty());
    s.typ("S");
    until_ended(&mut s);
    assert!(!s.terminal_output().is_empty(), "watched, and quick, but it asks");
}

#[test]
fn the_notify_command_replaces_the_desktop_notification_and_its_failure_toasts() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("told");
    let command = format!("echo \"$RUN_NAME $RUN_STATUS $RUN_EXIT\" > {}; exit 3", out.display());
    let mut s = screen(&format!("notify: {{ after_secs: 0, command: '{command}' }}\n"));

    start_unviewed(&mut s, "B");
    until_ended(&mut s);
    let screen = s.until("notify.command: exited 3");

    assert!(screen.contains("notify.command: exited 3"), "{screen}");
    let at = |t: &str| screen.lines().position(|l| l.contains(t)).unwrap();
    assert!(at("notify.command") > at("✘ bad failed"), "the newest is lowest:\n{screen}");
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "bad failed 1\n");
    assert!(s.terminal_output().is_empty(), "no OSC");
}

#[test]
fn during_a_raw_takeover_notifications_and_toast_time_wait_for_its_end() {
    let config = "notify: { after_secs: 0 }\nchains:\n  ok:\n    key: A\n    steps: ['sleep 0.5']\n  big:\n    key: R\n    display: takeover\n    takeover: raw\n    steps: ['sleep 30']\n";
    let mut s = AppScreen::new(config, tempfile::tempdir().unwrap());
    start_unviewed(&mut s, "A");
    s.typ("R");

    std::thread::sleep(std::time::Duration::from_millis(800));
    s.draw();
    s.draw();
    assert!(s.terminal_output().is_empty(), "nothing while the run owns the terminal");
    s.advance(5);
    s.ctrl('t');
    s.ctrl('g');

    s.draw();
    let out = s.draw();
    assert!(out.contains("✔ ok done"), "its time starts now:\n{out}");
    assert!(!s.terminal_output().is_empty());
}
