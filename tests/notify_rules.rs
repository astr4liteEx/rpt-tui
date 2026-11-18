//! Telling a run's end: when it notifies, what it says, the OSC bytes and the command's env.
use std::path::Path;
use std::time::Duration;

use rpt_tui::config::NotifySettings;
use rpt_tui::history::Ended;
use rpt_tui::notify::{self, RunEnd, Terminal};

fn ok() -> Ended {
    Ended::Exited { code: 0, failed_step: None }
}

fn end(ended: &Ended, secs: u64, watched: bool, chain_notify: Option<bool>) -> RunEnd<'_> {
    RunEnd { name: "import", ended, duration: Duration::from_secs(secs), watched, chain_notify }
}

#[test]
fn only_long_unwatched_runs_notify() {
    let s = NotifySettings::default();
    let ok = ok();

    assert!(end(&ok, 30, false, None).notifies(&s));
    assert!(!end(&ok, 29, false, None).notifies(&s), "too short");
    assert!(!end(&ok, 300, true, None).notifies(&s), "watched");
    assert!(!end(&Ended::Killed, 300, false, None).notifies(&s), "the user killed it");
}

#[test]
fn a_chains_notify_overrides_the_rule() {
    let s = NotifySettings::default();
    let ok = ok();

    assert!(end(&ok, 1, true, Some(true)).notifies(&s));
    assert!(!end(&ok, 300, false, Some(false)).notifies(&s));
    assert!(!end(&Ended::Killed, 300, false, Some(true)).notifies(&s), "never for a run the user killed");
}

#[test]
fn the_message_says_how_it_went() {
    let failed = Ended::Exited { code: 1, failed_step: Some(2) };
    let plain = Ended::Exited { code: 3, failed_step: None };
    let signalled = Ended::Signalled { signal: "Segmentation fault".into() };

    assert_eq!(end(&ok(), 123, false, None).message(), "✔ import done in 2m03s");
    assert_eq!(end(&failed, 5, false, None).message(), "✘ import failed at step 2 (exit 1)");
    assert_eq!(end(&plain, 5, false, None).message(), "✘ import failed (exit 3)");
    assert_eq!(end(&signalled, 5, false, None).message(), "✘ import failed (Segmentation fault)");
}

#[test]
fn desktop_notifications_use_osc_777_where_it_is_known_and_9_elsewhere() {
    let t = |term: &str, vte: bool, tmux: bool| Terminal { term: term.into(), vte, tmux };

    assert_eq!(notify::desktop("✔ done", &t("xterm-256color", false, false)), b"\x1b]9;\xe2\x9c\x94 done\x1b\\");
    let n777 = b"\x1b]777;notify;rpt-tui;hi\x1b\\".to_vec();
    assert_eq!(notify::desktop("hi", &t("foot", false, false)), n777);
    assert_eq!(notify::desktop("hi", &t("rxvt-unicode-256color", false, false)), n777);
    assert_eq!(notify::desktop("hi", &t("xterm-256color", true, false)), n777, "VTE");
    assert_eq!(
        notify::desktop("hi", &t("tmux-256color", false, true)),
        b"\x1bPtmux;\x1b\x1b]9;hi\x1b\x1b\\\x1b\\",
        "tmux passthrough doubles the escapes inside"
    );
    assert_eq!(notify::desktop("a\x07b\x1bc", &t("xterm", false, false)), b"\x1b]9;abc\x1b\\", "no controls");
}

#[test]
fn the_command_is_told_about_the_run() {
    let failed = Ended::Exited { code: 4, failed_step: Some(1) };
    let killed = Ended::Signalled { signal: "Terminated".into() };
    let env = |e: &RunEnd| e.command_env(Path::new("/r/acme"));

    assert_eq!(
        env(&end(&failed, 61, false, None)),
        [
            ("RUN_NAME", "import".to_string()),
            ("RUN_STATUS", "failed".into()),
            ("RUN_EXIT", "4".into()),
            ("RUN_DURATION", "61".into()),
            ("RPT_REPORT_DIR", "/r/acme".into()),
        ]
    );
    let k = env(&end(&killed, 2, false, None));
    assert_eq!((&k[1].1[..], &k[2].1[..]), ("failed", "143"), "a signal it wasn't sent by the user: 128 + SIGTERM");
    assert_eq!(&env(&end(&ok(), 2, false, None))[1].1, "ok");
}

#[test]
fn the_command_runs_detached_and_a_failure_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    let cmd = format!("echo \"$RUN_NAME $RUN_STATUS\" > {}; exit 3", out.display());

    let done = notify::run_command(&cmd, &[("RUN_NAME", "import".into()), ("RUN_STATUS", "ok".into())]);

    assert_eq!(done.join().unwrap(), Some("notify.command: exited 3".to_string()));
    assert_eq!(std::fs::read_to_string(out).unwrap(), "import ok\n");
    assert_eq!(notify::run_command("true", &[]).join().unwrap(), None);
}
