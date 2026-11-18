//! Feedback when runs end (spec §10): toasts that go by time, "new" dots, and the desktop
//! notification, bell or `notify.command` for runs nobody watched.

use std::time::{Duration, Instant};

use super::{App, Focus};
use crate::notify::{self, RunEnd};

/// How long a run's toast shows when it went well.
const OK_TOAST_FOR: Duration = Duration::from_secs(4);
/// How long a run's toast shows when it failed.
const FAILED_TOAST_FOR: Duration = Duration::from_secs(10);

/// At most this many toasts show at once, the newest kept.
pub const MAX_TOASTS: usize = 3;

/// A toast for a run that ended.
pub struct RunToast {
    /// What it says.
    pub text: String,
    /// The run went well (drawn green, not red).
    pub ok: bool,
    /// When it came up, to stack it among the others.
    at: Instant,
    /// When it goes.
    shows_for: Duration,
    /// When it goes; none until the TUI draws again after a raw takeover.
    until: Option<Instant>,
}

impl App {
    /// Now, by the app's clock.
    pub(super) fn now(&self) -> Instant {
        Instant::now() + self.clock_ahead
    }

    /// Moves the app's clock on by `d`, as if that much time had passed. For tests: toasts
    /// go by this clock.
    pub fn pass_time(&mut self, d: Duration) {
        self.clock_ahead += d;
    }

    /// The terminal gained or lost focus.
    pub fn handle_focus(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// The bytes for the terminal itself since last asked; the main loop writes them out.
    /// None during a raw takeover, where they could land inside the run's own output.
    pub fn take_terminal_output(&mut self) -> Vec<u8> {
        if self.runs.in_raw_takeover() {
            return vec![];
        }
        std::mem::take(&mut self.to_terminal)
    }

    /// Whether run `i`'s screen is on display: selected with Runs focused, taken over, or
    /// with the keys.
    pub(super) fn viewed(&self, i: usize) -> bool {
        self.runs.takeover == Some(i) || self.runs.sel == i && (self.focus == Focus::Runs || self.runs.keys)
    }

    /// Tells that run `id` ended, as spec §10 says: a toast and a dot unless it was viewed,
    /// and a notification if it ran long unwatched (or its chain asks).
    pub(super) fn announce(&mut self, id: u64) {
        let Some(i) = self.runs.entries.iter().position(|e| e.id == id) else { return };
        let viewed = self.viewed(i);
        let e = &self.runs.entries[i];
        let Some((ended, took)) = &e.ended else { return };
        let end =
            RunEnd { name: &e.label, ended, duration: *took, watched: viewed && self.focused, chain_notify: e.notify };
        if end.killed() {
            return;
        }
        let (ok, text, ephemeral) = (end.ok(), end.message(), e.ephemeral);
        let notifies = end.notifies(&self.notify);
        let env = end.command_env(&self.view.root);
        // An ephemeral run leaves no trace, so its failure toasts even in view.
        if (!viewed || ephemeral) && !(ephemeral && ok) {
            let shows_for = if ok { OK_TOAST_FOR } else { FAILED_TOAST_FOR };
            let until = (!self.runs.in_raw_takeover()).then(|| self.now() + shows_for);
            let at = self.now();
            self.run_toasts.push(RunToast { text: text.clone(), ok, at, shows_for, until });
        }
        if !viewed && !ephemeral {
            self.runs.entries[i].unseen = Some(!ok);
        }
        if notifies {
            match &self.notify.command {
                Some(cmd) => self.notify_jobs.push(notify::run_command(cmd, &env)),
                None if self.notify.desktop => {
                    self.to_terminal.extend(notify::desktop(&text, &self.terminal));
                }
                None => {}
            }
            if self.notify.bell {
                self.to_terminal.extend(notify::BELL);
            }
        }
    }

    /// Drops toasts whose time is up, clears the dot of the run in view, and toasts commands
    /// that failed. True when something to draw changed.
    pub(super) fn tick_feedback(&mut self) -> bool {
        let now = self.now();
        if self.toast.as_ref() != self.toast_since.as_ref().map(|(t, _)| t) {
            self.toast_since = self.toast.clone().map(|t| (t, now));
        }
        let before = self.run_toasts.len();
        // A raw takeover owns the terminal: toasts' time starts when it ends.
        if !self.runs.in_raw_takeover() {
            for t in self.run_toasts.iter_mut().filter(|t| t.until.is_none()) {
                t.until = Some(now + t.shows_for);
            }
        }
        self.run_toasts.retain(|t| t.until.is_none_or(|u| u > now));
        let mut changed = self.run_toasts.len() != before;
        let sel = self.runs.sel;
        if self.viewed(sel)
            && let Some(e) = self.runs.entries.get_mut(sel)
            && e.unseen.take().is_some()
        {
            changed = true;
        }
        let (done, running): (Vec<_>, Vec<_>) = self.notify_jobs.drain(..).partition(|j| j.is_finished());
        self.notify_jobs = running;
        for why in done.into_iter().filter_map(|j| j.join().ok().flatten()) {
            self.add_toast(why);
            changed = true;
        }
        changed
    }

    /// The toasts to show, oldest first: the startup or error toast, then runs'; the newest
    /// [`MAX_TOASTS`].
    pub(super) fn toasts(&self) -> Vec<(&str, Option<bool>)> {
        let now = self.now();
        // The error toast came up when first seen in a tick (or now, if not seen yet).
        let since = |t: &String| self.toast_since.as_ref().filter(|(s, _)| s == t).map_or(now, |(_, at)| *at);
        let mut all: Vec<(Instant, &str, Option<bool>)> = self
            .toast
            .iter()
            .map(|t| (since(t), t.as_str(), None))
            .chain(
                self.run_toasts
                    .iter()
                    .filter(|t| t.until.is_some_and(|u| u > now))
                    .map(|t| (t.at, t.text.as_str(), Some(t.ok))),
            )
            .collect();
        all.sort_by_key(|(at, ..)| *at);
        all[all.len().saturating_sub(MAX_TOASTS)..].iter().map(|(_, t, ok)| (*t, *ok)).collect()
    }

    /// The Runs title: `3 Runs`, with `· N new` and `· N ✘` when there are any.
    pub(super) fn runs_title(&self) -> String {
        let unseen: Vec<bool> = self.runs.entries.iter().filter_map(|e| e.unseen).collect();
        let failed = unseen.iter().filter(|f| **f).count();
        let mut title = "3 Runs".to_string();
        if !unseen.is_empty() {
            title += &format!(" · {} new", unseen.len());
        }
        if failed > 0 {
            title += &format!(" · {failed} ✘");
        }
        title
    }
}
