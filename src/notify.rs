//! Telling a run's end when nobody watched it: a desktop notification (OSC 9 or 777), the
//! bell, or the user's `notify.command`. Toasts and "new" dots are the `ui`'s.

use std::path::Path;
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::config::NotifySettings;
use crate::history::Ended;

/// A run that just ended.
pub struct RunEnd<'a> {
    /// Its label in Runs.
    pub name: &'a str,
    /// How it ended.
    pub ended: &'a Ended,
    /// How long it ran.
    pub duration: Duration,
    /// Its screen was on display, in a focused terminal, when it ended.
    pub watched: bool,
    /// Its chain's `notify:`, which overrides the rule.
    pub chain_notify: Option<bool>,
}

impl RunEnd<'_> {
    /// It ended well.
    pub fn ok(&self) -> bool {
        matches!(self.ended, Ended::Exited { code: 0, .. })
    }

    /// The user killed it: nothing to tell.
    pub fn killed(&self) -> bool {
        matches!(self.ended, Ended::Killed)
    }

    /// Whether it notifies: long enough and unwatched, unless its chain says.
    pub fn notifies(&self, s: &NotifySettings) -> bool {
        !self.killed()
            && self.chain_notify.unwrap_or(!self.watched && self.duration >= Duration::from_secs(s.after_secs))
    }

    /// The toast's text, which a desktop notification shows too.
    pub fn message(&self) -> String {
        match self.ended {
            Ended::Exited { code: 0, .. } => {
                format!("✔ {} done in {}", self.name, crate::time::took(self.duration))
            }
            Ended::Exited { code, failed_step: Some(step) } => {
                format!("✘ {} failed at step {step} (exit {code})", self.name)
            }
            Ended::Exited { code, failed_step: None } => format!("✘ {} failed (exit {code})", self.name),
            Ended::Signalled { signal } => format!("✘ {} failed ({signal})", self.name),
            Ended::Killed => format!("✘ {} killed", self.name),
        }
    }

    /// `notify.command`'s environment.
    pub fn command_env(&self, report_dir: &Path) -> Vec<(&'static str, String)> {
        let (status, exit) = match self.ended {
            Ended::Exited { code, .. } => (if *code == 0 { "ok" } else { "failed" }, code.to_string()),
            // A signal the user didn't send: a crash, or a kill from outside.
            Ended::Signalled { signal } => {
                ("failed", signal_number(signal).map_or(String::new(), |n| (128 + n).to_string()))
            }
            Ended::Killed => ("killed", (128 + libc::SIGKILL).to_string()),
        };
        vec![
            ("RUN_NAME", self.name.to_string()),
            ("RUN_STATUS", status.into()),
            ("RUN_EXIT", exit),
            ("RUN_DURATION", self.duration.as_secs().to_string()),
            ("RPT_REPORT_DIR", report_dir.display().to_string()),
        ]
    }
}

/// A signal's number from its description (`Terminated`), as the exit status gives it.
fn signal_number(described: &str) -> Option<i32> {
    (1..65).find(|&n| {
        // SAFETY: strsignal returns a static or thread-local C string, or null.
        let p = unsafe { libc::strsignal(n) };
        // SAFETY: a non-null pointer from strsignal is a valid C string.
        !p.is_null() && unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy() == described
    })
}

/// What the terminal is, for choosing the notification's escape.
pub struct Terminal {
    /// `$TERM`.
    pub term: String,
    /// `$VTE_VERSION` is set: a VTE terminal (GNOME Terminal and the like).
    pub vte: bool,
    /// Inside tmux (`$TMUX`).
    pub tmux: bool,
}

impl Terminal {
    /// A plain terminal: OSC 9, no tmux.
    pub fn plain() -> Terminal {
        Terminal { term: "xterm".into(), vte: false, tmux: false }
    }

    /// The terminal this process runs in.
    pub fn from_env() -> Terminal {
        let set = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
        Terminal { term: std::env::var("TERM").unwrap_or_default(), vte: set("VTE_VERSION"), tmux: set("TMUX") }
    }
}

/// A desktop notification saying `body`: OSC 777 where it's known to work, else OSC 9;
/// wrapped for tmux's passthrough inside tmux.
pub fn desktop(body: &str, t: &Terminal) -> Vec<u8> {
    let body: String = body.chars().filter(|c| !c.is_control()).collect();
    let osc777 = t.vte || t.term.starts_with("foot") || t.term.starts_with("rxvt");
    let seq = if osc777 { format!("\x1b]777;notify;rpt-tui;{body}\x1b\\") } else { format!("\x1b]9;{body}\x1b\\") };
    if t.tmux { format!("\x1bPtmux;{}\x1b\\", seq.replace('\x1b', "\x1b\x1b")) } else { seq }.into_bytes()
}

/// The terminal bell.
pub const BELL: &[u8] = b"\x07";

/// Runs `command` through `bash -c` in its own session, with `env` added and nothing for
/// input or output. The handle gives why it failed, if it did, for a toast.
pub fn run_command(command: &str, env: &[(&str, String)]) -> JoinHandle<Option<String>> {
    let mut cmd = Command::new("bash");
    cmd.arg("-c").arg(command).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    cmd.envs(env.iter().map(|(k, v)| (k, v)));
    // SAFETY: setsid is async-signal-safe, the only kind of call allowed before exec.
    unsafe {
        std::os::unix::process::CommandExt::pre_exec(&mut cmd, || {
            libc::setsid();
            Ok(())
        });
    }
    let child = cmd.spawn();
    std::thread::spawn(move || match child.and_then(|mut c| c.wait()) {
        Ok(st) if st.success() => None,
        Ok(st) => Some(match st.code() {
            Some(code) => format!("notify.command: exited {code}"),
            None => "notify.command: killed by a signal".into(),
        }),
        Err(e) => Some(format!("notify.command: could not start ({e})")),
    })
}
