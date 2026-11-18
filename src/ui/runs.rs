//! Runs in the UI: launching, the Runs panel's state, giving keys to a run, and takeover.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use ratatui::crossterm::event::{KeyEvent, MouseEvent};
use ratatui::layout::Rect;
use ratatui::style::Color;

use super::{App, Focus};
use crate::history::{Ended, Invocation, Record, Saved};
use crate::keys::Action;
use crate::run::chain::Scratch;
use crate::run::{Colors, KILL_GRACE, Launch, Run, Size, Status};

/// A live run silent for this long is shown as quiet.
pub const QUIET_AFTER: Duration = Duration::from_secs(3);

pub use crate::config::TakeoverStyle;

/// Where a run opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    Pane,
    Takeover(TakeoverStyle),
}

pub struct RunEntry {
    /// Unique among this session's runs; indices shift as runs are removed.
    pub id: u64,
    pub label: String,
    /// The process; none for a run read back from history.
    pub run: Option<Run>,
    /// The style used when this run is taken over.
    pub style: TakeoverStyle,
    /// Removed from Runs when it ends, leaving no trace.
    pub ephemeral: bool,
    /// The run's private folder (`RUN_TMP`), deleted when it ends.
    pub scratch: Option<Scratch>,
    /// The step a chain stopped at, once it has failed.
    pub failed_step: Option<usize>,
    /// When it started, shown in Runs so repeated listings can be told apart.
    pub started: std::time::SystemTime,
    /// The panel focused when it was launched; an ephemeral run gives focus back to it.
    pub came_from: Focus,
    /// What was run, for history and `rerun`; none for a run that is no command (the file
    /// picker).
    pub invocation: Option<Invocation>,
    /// It was launched taken over.
    pub takeover: bool,
    /// How it ended and how long it ran, once it has.
    pub ended: Option<(Ended, Duration)>,
    /// Its folder in history, once saved.
    pub saved: Option<String>,
    /// Read back from an earlier session's history: dimmed.
    pub earlier: bool,
    /// It ended unviewed and hasn't been viewed since (the "new" dot): `Some(true)` if it
    /// failed.
    pub unseen: Option<bool>,
    /// Its chain's `notify:`.
    pub notify: Option<bool>,
}

impl RunEntry {
    /// A run read back from history.
    pub(super) fn earlier(id: u64, saved: Saved) -> RunEntry {
        let r = saved.record;
        let failed_step = match r.ended {
            Ended::Exited { failed_step, .. } => failed_step,
            _ => None,
        };
        RunEntry {
            id,
            label: r.label,
            run: None,
            style: TakeoverStyle::Emulated,
            ephemeral: false,
            scratch: None,
            failed_step,
            started: std::time::UNIX_EPOCH + Duration::from_millis(r.started_ms),
            came_from: Focus::Findings,
            invocation: Some(r.invocation),
            takeover: r.takeover,
            ended: Some((r.ended, Duration::from_millis(r.duration_ms))),
            saved: Some(saved.key),
            earlier: true,
            unseen: None,
            notify: None,
        }
    }

    /// Its status: the process's, else how history says it ended.
    pub fn status(&self) -> Status {
        match (&self.run, &self.ended) {
            (Some(run), _) => run.status(),
            (None, Some((ended, _))) => ended.into(),
            (None, None) => Status::Killed,
        }
    }

    /// Whether it is still running.
    pub fn is_live(&self) -> bool {
        self.status() == Status::Live
    }

    /// The process, while it lives.
    pub(super) fn live_run(&mut self) -> Option<&mut Run> {
        self.run.as_mut().filter(|r| r.status() == Status::Live)
    }

    /// What history keeps of it, once it has ended.
    pub(super) fn record(&self) -> Option<Record> {
        let (ended, took) = self.ended.clone()?;
        Some(Record {
            label: self.label.clone(),
            invocation: self.invocation.clone()?,
            started_ms: crate::history::epoch_ms(self.started),
            duration_ms: took.as_millis() as u64,
            ended,
            takeover: self.takeover,
        })
    }
}

/// The Runs panel's state.
pub struct Runs {
    pub entries: Vec<RunEntry>,
    pub sel: usize,
    /// Keys go to the selected run (in its pane or taken over).
    pub keys: bool,
    /// The run taken over, if any.
    pub takeover: Option<usize>,
    /// The pane's inside, as last drawn; live runs are kept this size.
    pub pane: Rect,
    /// The whole terminal, as last drawn.
    pub full: Size,
    /// Set by run reader threads when a screen changes.
    pub dirty: Arc<AtomicBool>,
    /// The next run's id.
    pub(super) next_id: u64,
}

impl Default for Runs {
    fn default() -> Runs {
        Runs {
            entries: vec![],
            sel: 0,
            keys: false,
            takeover: None,
            pane: Rect::new(0, 0, 60, 20),
            full: Size { rows: 24, cols: 80 },
            dirty: Arc::new(AtomicBool::new(true)),
            next_id: 0,
        }
    }
}

/// How a run's status reads in the Runs panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mark {
    Live,
    Quiet(u64),
    Ok,
    Failed(String),
    Killed,
}

impl Mark {
    pub fn of(entry: &RunEntry) -> Mark {
        let quiet = entry.run.as_ref().map_or(Duration::ZERO, Run::quiet_for);
        match entry.status() {
            Status::Exited(code) if let Some(step) = entry.failed_step => Mark::Failed(format!("step {step}: {code}")),
            Status::Live if quiet >= QUIET_AFTER => Mark::Quiet(quiet.as_secs()),
            Status::Live => Mark::Live,
            Status::Exited(0) => Mark::Ok,
            Status::Exited(code) => Mark::Failed(code.to_string()),
            Status::Signalled(sig) => Mark::Failed(sig),
            Status::Killed => Mark::Killed,
        }
    }

    pub fn colour(&self, t: &super::theme::Theme) -> Color {
        match self {
            Mark::Live => t.accent,
            Mark::Quiet(_) => t.warn,
            Mark::Ok => t.ok,
            Mark::Failed(_) => t.err,
            Mark::Killed => t.muted,
        }
    }

    pub fn text(&self) -> String {
        match self {
            Mark::Live => "● live".into(),
            Mark::Quiet(s) => format!("◆ quiet {s}s"),
            Mark::Ok => "✔ 0".into(),
            Mark::Failed(why) => format!("✘ {why}"),
            Mark::Killed => "killed".into(),
        }
    }
}

impl Runs {
    pub fn pane_size(&self) -> Size {
        Size { rows: self.pane.height, cols: self.pane.width }
    }

    /// The size a taken-over run gets: emulated keeps one row for the status line.
    pub(super) fn takeover_size(&self, style: TakeoverStyle) -> Size {
        match style {
            TakeoverStyle::Emulated => Size { rows: self.full.rows.saturating_sub(1).max(1), cols: self.full.cols },
            TakeoverStyle::Raw => self.full,
        }
    }

    pub fn selected(&self) -> Option<&RunEntry> {
        self.entries.get(self.sel)
    }

    pub(super) fn selected_live(&self) -> Option<usize> {
        self.selected().filter(|e| e.is_live()).map(|_| self.sel)
    }

    /// The order runs are listed in: live ones first, then finished ones; newest first in each.
    pub fn order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.entries.len()).collect();
        order.sort_by_key(|&i| {
            let e = &self.entries[i];
            (!e.is_live(), std::cmp::Reverse(e.started), std::cmp::Reverse(e.id))
        });
        order
    }

    /// Moves the selection `d` rows down the list as it is shown (up when negative).
    pub(super) fn step(&mut self, d: isize) {
        let order = self.order();
        if let Some(at) = order.iter().position(|&i| i == self.sel) {
            self.sel = order[at.saturating_add_signed(d).min(order.len() - 1)];
        }
    }

    /// Selects the first row shown, or the last.
    pub(super) fn select_end(&mut self, last: bool) {
        let order = self.order();
        let pick = if last { order.last() } else { order.first() };
        if let Some(&i) = pick {
            self.sel = i;
        }
    }

    /// The run that keys go to, if any.
    pub fn key_target(&self) -> Option<usize> {
        self.takeover.or(if self.keys { self.selected_live() } else { None })
    }

    /// The run taken over in raw style, which the main loop hands the terminal.
    /// A run is taken over raw: it owns the real terminal.
    pub fn in_raw_takeover(&self) -> bool {
        self.takeover.and_then(|i| self.entries.get(i)).is_some_and(|e| e.style == TakeoverStyle::Raw)
    }

    pub fn raw_takeover(&mut self) -> Option<&mut Run> {
        let i = self.takeover?;
        let e = self.entries.get_mut(i)?;
        if e.style == TakeoverStyle::Raw { e.run.as_mut() } else { None }
    }

    pub fn enter_takeover(&mut self, i: usize) {
        let Some(style) = self.entries.get(i).filter(|e| e.is_live()).map(|e| e.style) else {
            return;
        };
        let size = self.takeover_size(style);
        if let Some(run) = self.entries[i].run.as_mut() {
            run.resize(size);
        }
        self.sel = i;
        self.takeover = Some(i);
        self.keys = true;
    }

    /// Removes run `i`; the selection and takeover stay on the same runs. Removing the
    /// taken-over run ends takeover; removing the selected one selects the run before it.
    pub fn remove(&mut self, i: usize) -> RunEntry {
        let e = self.entries.remove(i);
        let shift = |j: usize| if j > i { j - 1 } else { j };
        self.takeover = self.takeover.filter(|&t| t != i).map(shift);
        self.sel = if self.sel == i { i.saturating_sub(1) } else { shift(self.sel) };
        self.sel = self.sel.min(self.entries.len().saturating_sub(1));
        e
    }

    /// Back to the pane; keys stay with the run while it lives.
    pub fn leave_takeover(&mut self) {
        let Some(i) = self.takeover.take() else { return };
        let pane = self.pane_size();
        if let Some(run) = self.entries.get_mut(i).and_then(|e| e.run.as_mut()) {
            run.stop_tee();
            run.resize(pane);
        }
    }
}

impl App {
    /// Starts `argv` in the report as a new run, selected, with the keys; returns its index.
    pub fn launch(&mut self, label: &str, argv: Vec<String>, display: Display) -> Option<usize> {
        let launch = Launch::new(argv, &self.view.root);
        self.start(label, launch, display, None, None)
    }

    /// Starts `launch` as a new run, selected, with the keys; returns its index. Every run gets
    /// `RPT_REPORT_DIR`, and `rpt` on its `PATH` is the rpt this TUI uses.
    pub(super) fn start(
        &mut self,
        label: &str,
        mut launch: Launch,
        display: Display,
        scratch: Option<Scratch>,
        invocation: Option<Invocation>,
    ) -> Option<usize> {
        // Only the current selection's variables: none inherited from rpt-tui's own environment.
        launch.unset.extend(super::SELECTION_VARS.iter().map(|v| v.to_string()));
        launch.env.extend(self.selection_env());
        launch.env.push(("RPT_REPORT_DIR".into(), self.view.root.display().to_string()));
        launch.env.push(("PATH".into(), self.client.path().to_string_lossy().into_owned()));
        let (fg, bg) = self.theme.query_colours();
        launch.colors = Colors { fg, bg };
        launch.transcript_cap = self.history_limits.max_transcript;
        let style = match display {
            Display::Takeover(style) => style,
            Display::Pane => TakeoverStyle::Emulated,
        };
        match Run::spawn(launch, self.runs.pane_size(), self.runs.dirty.clone()) {
            Ok(run) => {
                self.runs.next_id += 1;
                self.runs.entries.push(RunEntry {
                    id: self.runs.next_id,
                    label: label.to_string(),
                    run: Some(run),
                    style,
                    ephemeral: false,
                    scratch,
                    failed_step: None,
                    started: std::time::SystemTime::now(),
                    came_from: self.focus,
                    invocation,
                    takeover: matches!(display, Display::Takeover(_)),
                    ended: None,
                    saved: None,
                    earlier: false,
                    unseen: None,
                    notify: None,
                });
                let i = self.runs.entries.len() - 1;
                self.runs.sel = i;
                self.runs.keys = true;
                self.set_focus(Focus::Runs);
                if matches!(display, Display::Takeover(_)) {
                    self.runs.enter_takeover(i);
                }
                Some(i)
            }
            Err(e) => {
                self.toast = Some(format!("could not start {label}: {e}"));
                None
            }
        }
    }

    /// Updates run statuses and sizes, takes in the plugin list, and re-reads the report
    /// when a run ends; call after each draw. True when something to draw changed.
    pub fn tick(&mut self) -> bool {
        let plugins = self.poll_plugins();
        let ended = self.tick_runs();
        self.finish_picker();
        if ended {
            self.refresh();
            self.reload_if_changed();
        }
        self.drop_ended_ephemeral();
        if self.runs.keys && self.runs.selected_live().is_none() {
            self.runs.keys = false;
        }
        let feedback = self.tick_feedback();
        plugins || ended || feedback
    }

    /// Polls and sizes the runs, and saves those that ended; true when one ended.
    fn tick_runs(&mut self) -> bool {
        let pane = self.runs.pane_size();
        let takeover = self.runs.takeover;
        let mut ended = vec![];
        for (i, e) in self.runs.entries.iter_mut().enumerate() {
            let Some(run) = e.run.as_mut() else { continue };
            let before = run.status();
            let now = run.poll();
            if now == Status::Live && Some(i) != takeover {
                run.resize(pane);
            }
            if now != Status::Live && before == Status::Live {
                ended.push(e.id);
            }
        }
        // A taken-over run follows the terminal's size.
        if let Some(i) = takeover {
            let size = self.runs.takeover_size(self.runs.entries[i].style);
            if let Some(run) = self.runs.entries[i].run.as_mut() {
                run.resize(size);
            }
        }
        if let Some(i) = takeover
            && !self.runs.entries[i].is_live()
        {
            self.runs.leave_takeover();
        }
        for &id in &ended {
            self.finish(id, false);
            self.announce(id);
        }
        !ended.is_empty()
    }

    /// Notes how run `id` ended (as killed when `killed`, whatever its last poll said), and
    /// saves it to history.
    fn finish(&mut self, id: u64, killed: bool) {
        let Some(e) = self.runs.entries.iter_mut().find(|e| e.id == id) else { return };
        if let Some(scratch) = e.scratch.take() {
            e.failed_step = scratch.failed_step();
        }
        let ended = match Ended::of(&e.status(), e.failed_step) {
            Some(ended) if !killed => ended,
            _ => Ended::Killed,
        };
        e.ended = Some((ended, e.started.elapsed().unwrap_or_default()));
        self.save_run(id);
    }

    /// Ended ephemeral runs leave Runs (a failure has toasted).
    fn drop_ended_ephemeral(&mut self) {
        while let Some(i) = self.runs.entries.iter().position(|e| e.ephemeral && !e.is_live()) {
            // Focus goes back to where it was launched from, unless it has moved on since.
            let watched = self.focus == Focus::Runs && self.runs.sel == i;
            let e = self.runs.remove(i);
            if watched {
                self.runs.keys = false;
                self.set_focus(e.came_from);
            }
        }
    }

    /// Keys while a run has them: `leave_run` takes them back, `toggle_takeover` switches pane
    /// and takeover, anything else goes to the run. Returns false when no run has the keys.
    pub(super) fn run_key(&mut self, key: KeyEvent) -> bool {
        let Some(i) = self.runs.key_target() else { return false };
        match self.bindings.run_action(&key) {
            Some(Action::LeaveRun) => {
                self.runs.leave_takeover();
                self.runs.keys = false;
            }
            Some(Action::ToggleTakeover) => self.toggle_takeover(i),
            _ => {
                if let Some(run) = &self.runs.entries[i].run {
                    run.send_key(&key)
                }
            }
        }
        true
    }

    fn toggle_takeover(&mut self, i: usize) {
        if self.runs.takeover.is_some() {
            self.runs.leave_takeover();
        } else {
            self.runs.enter_takeover(i);
        }
    }

    pub(super) fn take_over_selected(&mut self) {
        self.runs.enter_takeover(self.runs.sel);
    }

    /// `kill_or_delete`: kills a live run; a finished one is deleted after a y/n.
    pub(super) fn kill_or_delete(&mut self) {
        let Some(e) = self.runs.entries.get_mut(self.runs.sel) else { return };
        match e.live_run() {
            Some(run) => run.kill(),
            None => self.confirm = Some(super::Confirm::Delete(e.id)),
        }
    }

    /// A mouse event: forwarded to the run that has the keys, relative to where it is drawn.
    pub fn handle_mouse(&mut self, ev: MouseEvent) {
        let Some(i) = self.runs.key_target() else { return };
        let origin = if self.runs.takeover.is_some() { (0, 0) } else { (self.runs.pane.x, self.runs.pane.y) };
        if let Some(run) = &self.runs.entries[i].run {
            run.send_mouse(&ev, origin);
        }
    }

    /// Pasted text: to the run that has the keys.
    pub fn handle_paste(&mut self, text: &str) {
        if let Some(run) = self.runs.key_target().and_then(|i| self.runs.entries[i].run.as_ref()) {
            run.send_paste(text);
        }
    }

    /// Kills every live run (on quit): SIGHUP, up to [`KILL_GRACE`] to go, then SIGKILL; each
    /// is saved to history as killed. This waits, because the process exits straight after and a
    /// timer thread would die with it.
    pub fn kill_all(&mut self) {
        let mut killed: Vec<u64> = vec![];
        for e in &mut self.runs.entries {
            if let Some(run) = e.run.as_mut().filter(|r| r.status() == Status::Live) {
                run.kill();
                killed.push(e.id);
            }
        }
        let live = |app: &mut App| {
            app.runs
                .entries
                .iter_mut()
                .any(|e| killed.contains(&e.id) && e.run.as_mut().is_some_and(|r| r.poll() == Status::Live))
        };
        let t0 = std::time::Instant::now();
        while t0.elapsed() < KILL_GRACE && live(self) {
            std::thread::sleep(Duration::from_millis(20));
        }
        // Grandchildren may outlive the child, so the groups get SIGKILL either way.
        for e in self.runs.entries.iter_mut().filter(|e| killed.contains(&e.id)) {
            if let Some(run) = e.run.as_mut() {
                run.kill_now();
            }
        }
        // A moment for the last output to be read, so the transcript is whole.
        let t0 = std::time::Instant::now();
        while t0.elapsed() < Duration::from_millis(300) && live(self) {
            std::thread::sleep(Duration::from_millis(20));
        }
        for id in killed {
            self.finish(id, true);
        }
    }

    pub fn any_live(&self) -> bool {
        self.runs.entries.iter().any(RunEntry::is_live)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(label: &str) -> RunEntry {
        let launch = Launch::new(vec!["true".into()], &std::env::temp_dir());
        let run = Run::spawn(launch, Size { rows: 5, cols: 20 }, Arc::new(AtomicBool::new(false))).unwrap();
        RunEntry {
            id: 0,
            label: label.into(),
            run: Some(run),
            style: TakeoverStyle::Emulated,
            ephemeral: false,
            scratch: None,
            failed_step: None,
            started: std::time::SystemTime::now(),
            came_from: Focus::Findings,
            invocation: None,
            takeover: false,
            ended: None,
            saved: None,
            earlier: false,
            unseen: None,
            notify: None,
        }
    }

    fn runs(labels: &[&str]) -> Runs {
        Runs { entries: labels.iter().map(|l| entry(l)).collect(), ..Runs::default() }
    }

    fn label(runs: &Runs, i: Option<usize>) -> Option<&str> {
        i.and_then(|i| runs.entries.get(i)).map(|e| e.label.as_str())
    }

    #[test]
    fn removing_a_run_keeps_the_selection_and_takeover_on_the_same_runs() {
        let mut r = runs(&["a", "b", "c"]);
        r.sel = 2;
        r.takeover = Some(1);

        r.remove(0);

        assert_eq!(label(&r, Some(r.sel)), Some("c"));
        assert_eq!(label(&r, r.takeover), Some("b"));
    }

    #[test]
    fn removing_the_selected_or_taken_over_run_moves_the_selection_and_ends_takeover() {
        let mut r = runs(&["a", "b"]);
        r.sel = 1;
        r.takeover = Some(1);

        r.remove(1);

        assert_eq!(label(&r, Some(r.sel)), Some("a"));
        assert_eq!(r.takeover, None);
    }
}
