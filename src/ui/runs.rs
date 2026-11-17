//! Runs in the UI: launching, the Runs panel's state, giving keys to a run, and takeover.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};
use ratatui::layout::Rect;
use ratatui::style::Color;

use super::{App, Focus};
use crate::run::{Colors, KILL_GRACE, Launch, Run, Size, Status};

/// A live run silent for this long is shown as quiet.
pub const QUIET_AFTER: Duration = Duration::from_secs(3);

/// How a run is taken over: emulated draws its grid full-screen; raw hands it the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TakeoverStyle {
    Emulated,
    Raw,
}

/// Where a run opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    Pane,
    Takeover(TakeoverStyle),
}

pub struct RunEntry {
    pub label: String,
    pub run: Run,
    /// The style used when this run is taken over.
    pub style: TakeoverStyle,
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
    pub fn of(run: &Run) -> Mark {
        match run.status() {
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

    fn selected_live(&self) -> Option<usize> {
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
    /// Starts `argv` in the report as a new run, selected, with the keys.
    pub fn launch(&mut self, label: &str, argv: Vec<String>, display: Display) {
        let mut launch = Launch::new(argv, &self.view.root);
        launch.env.push(("RPT_REPORT_DIR".into(), self.view.root.display().to_string()));
        let defaults = Colors::default();
        launch.colors = Colors { fg: rgb(self.theme.text, defaults.fg), bg: rgb(self.theme.bg, defaults.bg) };
        let style = match display {
            Display::Takeover(style) => style,
            Display::Pane => TakeoverStyle::Emulated,
        };
        match Run::spawn(launch, self.runs.pane_size(), self.runs.dirty.clone()) {
            Ok(run) => {
                self.runs.entries.push(RunEntry { label: label.to_string(), run, style });
                let i = self.runs.entries.len() - 1;
                self.runs.sel = i;
                self.runs.keys = true;
                self.focus = Focus::Runs;
                if matches!(display, Display::Takeover(_)) {
                    self.runs.enter_takeover(i);
                }
            }
            Err(e) => self.toast = Some(format!("could not start {label}: {e}")),
        }
    }

    /// Updates run statuses and sizes; call after each draw. True when a status changed.
    pub fn tick(&mut self) -> bool {
        let pane = self.runs.pane_size();
        let takeover = self.runs.takeover;
        let mut changed = false;
        for (i, e) in self.runs.entries.iter_mut().enumerate() {
            let before = e.run.status();
            let now = e.run.poll();
            changed |= now != before;
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
        if self.runs.keys && self.runs.selected_live().is_none() {
            self.runs.keys = false;
        }
        changed
    }

    /// Keys while a run has them: `ctrl-g` takes them back, `ctrl-t` switches pane and takeover,
    /// anything else goes to the run. Returns false when no run has the keys.
    pub(super) fn run_key(&mut self, key: KeyEvent) -> bool {
        let Some(i) = self.runs.key_target() else { return false };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('g') if ctrl => {
                self.runs.leave_takeover();
                self.runs.keys = false;
            }
            KeyCode::Char('t') if ctrl => self.toggle_takeover(i),
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

    /// Keys for the Runs panel itself (focus on Runs, no run holding the keys).
    pub(super) fn runs_panel_key(&mut self, key: KeyEvent) -> bool {
        let len = self.runs.entries.len();
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.runs.sel = (self.runs.sel + 1).min(len.saturating_sub(1)),
            KeyCode::Char('k') | KeyCode::Up => self.runs.sel = self.runs.sel.saturating_sub(1),
            KeyCode::Char('g') => self.runs.sel = 0,
            KeyCode::Char('G') => self.runs.sel = len.saturating_sub(1),
            KeyCode::Enter => self.runs.keys = self.runs.selected_live().is_some(),
            KeyCode::Char('f') => self.runs.enter_takeover(self.runs.sel),
            KeyCode::Char('x') => {
                if let Some(e) = self.runs.entries.get_mut(self.runs.sel) {
                    e.run.kill();
                }
            }
            _ => return false,
        }
        true
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
