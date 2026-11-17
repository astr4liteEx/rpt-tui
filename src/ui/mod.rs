//! The terminal UI: app state, keys and the main loop. Drawing is in `home`.

mod home;
pub mod theme;

use std::path::{Path, PathBuf};

use ratatui::{
    Frame,
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
};

use crate::rpt::{self, Client, Finding, Problem, Version};
use theme::Theme;

/// What the TUI knows about the report, read through rpt.
pub struct ReportView {
    pub root: PathBuf,
    pub findings: Vec<Finding>,
    pub problems: Vec<Problem>,
    pub version: Option<Version>,
    /// Severity levels from `rpt severities`, most severe first.
    pub severities: Vec<String>,
    /// Messages for things that could not be read, shown as a toast.
    pub errors: Vec<String>,
}

impl ReportView {
    pub fn load(client: &Client, root: &Path) -> ReportView {
        let mut errors = vec![];
        let version = keep(client.version(), &mut errors);
        let findings = keep(client.findings(), &mut errors).unwrap_or_default();
        let problems = keep(client.problems(), &mut errors).unwrap_or_default();
        let severities = client.severities().unwrap_or_else(|e| {
            errors.push(format!("{e} (rpt older than the severities verb? update rpt); using the default levels"));
            rpt::default_severities()
        });
        let mut view = ReportView { root: root.to_path_buf(), findings, problems, version, severities, errors };
        view.sort_findings();
        view
    }

    /// Most severe first, by the report's levels; levels it doesn't list go last. Ties by ID.
    fn sort_findings(&mut self) {
        let mut findings = std::mem::take(&mut self.findings);
        findings.sort_by(|a, b| {
            self.severity_position(&a.severity).cmp(&self.severity_position(&b.severity)).then_with(|| a.id.cmp(&b.id))
        });
        self.findings = findings;
    }

    /// Levels that have findings, with counts: in the report's order, then any unknown levels.
    pub fn severity_counts(&self) -> Vec<(&str, usize)> {
        let count = |level: &str| self.findings.iter().filter(|f| f.severity == level).count();
        let mut counts: Vec<(&str, usize)> = self.severities.iter().map(|s| (s.as_str(), count(s))).collect();
        for f in &self.findings {
            if !counts.iter().any(|(s, _)| *s == f.severity) {
                counts.push((&f.severity, count(&f.severity)));
            }
        }
        counts.retain(|(_, n)| *n > 0);
        counts
    }

    /// A level's position in the report's severities; unknown levels sort after them.
    pub fn severity_position(&self, level: &str) -> usize {
        self.severities.iter().position(|s| s == level).unwrap_or(self.severities.len())
    }

    /// A warning when rpt writes a record schema this TUI doesn't know.
    pub fn banner(&self) -> Option<String> {
        let v = self.version.as_ref()?;
        (v.schema_version != rpt::SCHEMA_VERSION).then(|| {
            format!(
                "rpt {} writes record schema {}; rpt-tui knows {}: some data may not show",
                v.version,
                v.schema_version,
                rpt::SCHEMA_VERSION
            )
        })
    }
}

/// The value, or `None` with the error's message pushed onto `errors`.
fn keep<T>(r: Result<T, rpt::Error>, errors: &mut Vec<String>) -> Option<T> {
    r.map_err(|e| errors.push(e.to_string())).ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Findings,
    Problems,
    Runs,
}

pub struct App {
    pub view: ReportView,
    pub focus: Focus,
    findings_sel: usize,
    problems_sel: usize,
    toast: Option<String>,
    theme: Theme,
    quit: bool,
}

impl App {
    pub fn new(view: ReportView) -> App {
        let toast = (!view.errors.is_empty()).then(|| view.errors.join("; "));
        App { view, focus: Focus::Findings, findings_sel: 0, problems_sel: 0, toast, theme: Theme::CHARM, quit: false }
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn selected_finding(&self) -> Option<&Finding> {
        self.view.findings.get(self.findings_sel)
    }

    pub fn selected_problem(&self) -> Option<&Problem> {
        self.view.problems.get(self.problems_sel)
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        // Any key dismisses the toast; real toast timing comes with notifications.
        self.toast = None;
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('c') if ctrl => self.quit = true,
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Findings => Focus::Problems,
                    Focus::Problems => Focus::Runs,
                    Focus::Runs => Focus::Findings,
                }
            }
            KeyCode::Char('1') => self.focus = Focus::Findings,
            KeyCode::Char('2') => self.focus = Focus::Problems,
            KeyCode::Char('3') => self.focus = Focus::Runs,
            KeyCode::Char('j') | KeyCode::Down => self.move_by(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_by(-1),
            KeyCode::Char('g') => self.move_to(0),
            KeyCode::Char('G') => self.move_to(usize::MAX),
            _ => {}
        }
    }

    fn list(&mut self) -> Option<(&mut usize, usize)> {
        match self.focus {
            Focus::Findings => Some((&mut self.findings_sel, self.view.findings.len())),
            Focus::Problems => Some((&mut self.problems_sel, self.view.problems.len())),
            Focus::Runs => None,
        }
    }

    fn move_by(&mut self, d: isize) {
        if let Some((sel, len)) = self.list() {
            *sel = sel.saturating_add_signed(d).min(len.saturating_sub(1));
        }
    }

    fn move_to(&mut self, i: usize) {
        if let Some((sel, len)) = self.list() {
            *sel = i.min(len.saturating_sub(1));
        }
    }

    pub fn draw(&mut self, f: &mut Frame) {
        home::draw(f, self);
    }
}

/// Runs the TUI until the user quits; returns the process exit code.
pub fn run(report: &Path, rpt_bin: &Path) -> i32 {
    let client = Client::new(rpt_bin, report);
    let mut app = App::new(ReportView::load(&client, report));
    let mut terminal = ratatui::init();
    let result = (|| -> std::io::Result<()> {
        while !app.should_quit() {
            terminal.draw(|f| app.draw(f))?;
            if let Event::Key(k) = event::read()?
                && k.kind == KeyEventKind::Press
            {
                app.handle_key(k);
            }
        }
        Ok(())
    })();
    ratatui::restore();
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("rpt-tui: {e}");
            1
        }
    }
}
