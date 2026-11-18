//! Runs in the UI: launching, the Runs panel's state, giving keys to a run, and takeover.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use ratatui::crossterm::event::{KeyEvent, MouseEvent};
use ratatui::layout::Rect;
use ratatui::style::Color;

use super::{App, Focus};
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
    pub run: Run,
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
}

/// A start time as the local clock reads it: `14:03:22`.
pub fn clock(t: std::time::SystemTime) -> String {
    let secs = t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as libc::time_t;
    // SAFETY: localtime_r only writes the tm it is given.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&secs, &mut tm) }.is_null() {
        return "--:--:--".into();
    }
    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
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
    next_id: u64,
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
        let run = &entry.run;
        match run.status() {
            Status::Exited(code) if let Some(step) = entry.failed_step => Mark::Failed(format!("step {step}: {code}")),
            Status::Live if run.quiet_for() >= QUIET_AFTER => Mark::Quiet(run.quiet_for().as_secs()),
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

fn rgb(c: Color, fallback: (u8, u8, u8)) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => fallback,
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
        self.selected().filter(|e| e.run.status() == Status::Live).map(|_| self.sel)
    }

    /// The run that keys go to, if any.
    pub fn key_target(&self) -> Option<usize> {
        self.takeover.or(if self.keys { self.selected_live() } else { None })
    }

    /// The run taken over in raw style, which the main loop hands the terminal.
    pub fn raw_takeover(&mut self) -> Option<&mut Run> {
        let i = self.takeover?;
        let e = self.entries.get_mut(i)?;
        (e.style == TakeoverStyle::Raw).then_some(&mut e.run)
    }

    pub fn enter_takeover(&mut self, i: usize) {
        let Some(style) = self.entries.get(i).filter(|e| e.run.status() == Status::Live).map(|e| e.style) else {
            return;
        };
        let size = self.takeover_size(style);
        self.entries[i].run.resize(size);
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
        if let Some(e) = self.entries.get_mut(i) {
            e.run.stop_tee();
            e.run.resize(pane);
        }
    }
}

impl App {
    /// Starts `argv` in the report as a new run, selected, with the keys; returns its index.
    pub fn launch(&mut self, label: &str, argv: Vec<String>, display: Display) -> Option<usize> {
        let launch = Launch::new(argv, &self.view.root);
        self.start(label, launch, display, None)
    }

    /// Starts `launch` as a new run, selected, with the keys; returns its index. Every run gets
    /// `RPT_REPORT_DIR`, and `rpt` on its `PATH` is the rpt this TUI uses.
    pub(super) fn start(
        &mut self,
        label: &str,
        mut launch: Launch,
        display: Display,
        scratch: Option<Scratch>,
    ) -> Option<usize> {
        // Only the current selection's variables: none inherited from rpt-tui's own environment.
        launch.unset.extend(super::SELECTION_VARS.iter().map(|v| v.to_string()));
        launch.env.extend(self.selection_env());
        launch.env.push(("RPT_REPORT_DIR".into(), self.view.root.display().to_string()));
        launch.env.push(("PATH".into(), self.client.path().to_string_lossy().into_owned()));
        let defaults = Colors::default();
        launch.colors = Colors { fg: rgb(self.theme.text, defaults.fg), bg: rgb(self.theme.bg, defaults.bg) };
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
                    run,
                    style,
                    ephemeral: false,
                    scratch,
                    failed_step: None,
                    started: std::time::SystemTime::now(),
                    came_from: self.focus,
                });
                let i = self.runs.entries.len() - 1;
                self.runs.sel = i;
                self.runs.keys = true;
                self.focus = Focus::Runs;
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
        }
        // After the refresh, so an ephemeral run's failure toast isn't replaced by it.
        self.drop_ended_ephemeral();
        if self.runs.keys && self.runs.selected_live().is_none() {
            self.runs.keys = false;
        }
        plugins || ended
    }

    /// Polls and sizes the runs; true when one ended.
    fn tick_runs(&mut self) -> bool {
        let pane = self.runs.pane_size();
        let takeover = self.runs.takeover;
        let mut changed = false;
        for (i, e) in self.runs.entries.iter_mut().enumerate() {
            let before = e.run.status();
            let now = e.run.poll();
            changed |= now != before;
            if now != Status::Live
                && let Some(scratch) = e.scratch.take()
            {
                e.failed_step = scratch.failed_step();
            }
            if now == Status::Live && Some(i) != takeover {
                e.run.resize(pane);
            }
        }
        // A taken-over run follows the terminal's size.
        if let Some(i) = takeover {
            let size = self.runs.takeover_size(self.runs.entries[i].style);
            self.runs.entries[i].run.resize(size);
        }
        if let Some(i) = takeover
            && self.runs.entries[i].run.status() != Status::Live
        {
            self.runs.leave_takeover();
        }
        changed
    }

    /// Ended ephemeral runs leave Runs; a failure leaves a toast with its code.
    fn drop_ended_ephemeral(&mut self) {
        while let Some(i) = self.runs.entries.iter().position(|e| e.ephemeral && e.run.status() != Status::Live) {
            // Focus goes back to where it was launched from, unless it has moved on since.
            let watched = self.focus == Focus::Runs && self.runs.sel == i;
            let e = self.runs.remove(i);
            if watched {
                self.runs.keys = false;
                self.set_focus(e.came_from);
            }
            if let Mark::Failed(why) = Mark::of(&e) {
                self.toast = Some(format!("{} failed: {why}", e.label));
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
            _ => self.runs.entries[i].run.send_key(&key),
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

    pub(super) fn kill_selected(&mut self) {
        if let Some(e) = self.runs.entries.get_mut(self.runs.sel) {
            e.run.kill();
        }
    }

    /// A mouse event: forwarded to the run that has the keys, relative to where it is drawn.
    pub fn handle_mouse(&mut self, ev: MouseEvent) {
        let Some(i) = self.runs.key_target() else { return };
        let origin = if self.runs.takeover.is_some() { (0, 0) } else { (self.runs.pane.x, self.runs.pane.y) };
        self.runs.entries[i].run.send_mouse(&ev, origin);
    }

    /// Pasted text: to the run that has the keys.
    pub fn handle_paste(&mut self, text: &str) {
        if let Some(i) = self.runs.key_target() {
            self.runs.entries[i].run.send_paste(text);
        }
    }

    /// Kills every live run (on quit): SIGHUP, up to [`KILL_GRACE`] to go, then SIGKILL. This
    /// waits, because the process exits straight after and a timer thread would die with it.
    pub fn kill_all(&mut self) {
        let mut killed: Vec<usize> = vec![];
        for (i, e) in self.runs.entries.iter_mut().enumerate() {
            if e.run.poll() == Status::Live {
                e.run.kill();
                killed.push(i);
            }
        }
        let t0 = std::time::Instant::now();
        while t0.elapsed() < KILL_GRACE && killed.iter().any(|&i| self.runs.entries[i].run.poll() == Status::Live) {
            std::thread::sleep(Duration::from_millis(20));
        }
        // Grandchildren may outlive the child, so the groups get SIGKILL either way.
        for i in killed {
            self.runs.entries[i].run.kill_now();
        }
    }

    pub fn any_live(&self) -> bool {
        self.runs.entries.iter().any(|e| e.run.status() == Status::Live)
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
            run,
            style: TakeoverStyle::Emulated,
            ephemeral: false,
            scratch: None,
            failed_step: None,
            started: std::time::SystemTime::now(),
            came_from: Focus::Findings,
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
