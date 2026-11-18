//! The terminal UI: app state, keys and the main loop. Drawing is in `home`.

mod chains;
mod commands;
pub mod edit;
pub mod form;
mod home;
pub mod palette;
pub mod picker;
mod runs;
mod term;
pub mod theme;

use runs::Runs;
pub use runs::{Display, Mark, QUIET_AFTER, TakeoverStyle};

use std::path::{Path, PathBuf};

use std::io::Write;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use ratatui::{
    DefaultTerminal, Frame,
    crossterm::{
        self,
        event::{
            self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event, KeyCode,
            KeyEvent, KeyEventKind, KeyModifiers,
        },
        execute,
    },
};

use crate::config::{Chain, Config};
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
    /// Findings or Problems, whichever was focused last: where the selection is.
    last_list: Focus,
    findings_sel: usize,
    problems_sel: usize,
    toast: Option<String>,
    theme: Theme,
    runs: Runs,
    quit: bool,
    client: Client,
    plugins: commands::Plugins,
    palette: Option<palette::State>,
    /// Commands used this session, most recent first (`plugin greet`).
    recent: Vec<String>,
    /// `$VISUAL` and `$EDITOR`, as rpt-tui was started with.
    editor_env: (Option<String>, Option<String>),
    /// Chains from the config, less any named like a plugin.
    chains: Vec<Chain>,
    /// `file_picker:` from the config.
    file_picker: Option<String>,
    /// A file picker run in progress.
    picking: Option<picker::Picking>,
    /// The built-in file list, when open.
    list_picker: Option<picker::ListPicker>,
}

impl App {
    /// Reads the report through `client` and starts listing plugins in the background.
    pub fn new(client: Client) -> App {
        App::with_config(client, Config::default())
    }

    /// As [`App::new`], with chains and settings from `config`; its errors join the startup toast.
    pub fn with_config(client: Client, config: Config) -> App {
        let view = ReportView::load(&client, client.report());
        let errors: Vec<String> = view.errors.iter().chain(&config.errors).cloned().collect();
        let toast = (!errors.is_empty()).then(|| errors.join("; "));
        App {
            view,
            focus: Focus::Findings,
            last_list: Focus::Findings,
            findings_sel: 0,
            problems_sel: 0,
            toast,
            theme: Theme::CHARM,
            runs: Runs::default(),
            quit: false,
            plugins: commands::Plugins::load(&client),
            client,
            palette: None,
            recent: vec![],
            editor_env: (std::env::var("VISUAL").ok(), std::env::var("EDITOR").ok()),
            chains: config.chains,
            file_picker: config.file_picker,
            picking: None,
            list_picker: None,
        }
    }

    /// Re-reads findings and problems (after a run), keeping the selection on the same items.
    fn refresh(&mut self) {
        let finding = self.selected_finding().map(|f| f.id.clone());
        let problem = self.selected_problem().map(|p| (p.path.clone(), p.line));
        self.view = ReportView::load(&self.client, &self.view.root);
        if !self.view.errors.is_empty() {
            self.toast = Some(self.view.errors.join("; "));
        }
        let clamp = |i: usize, len: usize| i.min(len.saturating_sub(1));
        self.findings_sel = finding
            .and_then(|id| self.view.findings.iter().position(|f| f.id == id))
            .unwrap_or(clamp(self.findings_sel, self.view.findings.len()));
        self.problems_sel = problem
            .and_then(|(path, line)| self.view.problems.iter().position(|p| p.path == path && p.line == line))
            .unwrap_or(clamp(self.problems_sel, self.view.problems.len()));
    }

    fn set_focus(&mut self, focus: Focus) {
        self.focus = focus;
        if focus != Focus::Runs {
            self.last_list = focus;
        }
    }

    fn focus_next(&mut self) {
        self.set_focus(match self.focus {
            Focus::Findings => Focus::Problems,
            Focus::Problems => Focus::Runs,
            Focus::Runs => Focus::Findings,
        })
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
        if self.list_picker.is_some() {
            self.list_picker_key(key);
            return;
        }
        // While a picker run has the keys, the palette waits behind it.
        if self.palette.is_some() && self.picking.is_none() {
            self.palette_key(key);
            return;
        }
        if self.run_key(key) {
            return;
        }
        if self.focus == Focus::Runs && self.runs_panel_key(key) {
            return;
        }
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('c') if ctrl => self.quit = true,
            KeyCode::Char(':') => self.open_palette(),
            KeyCode::Char('p') if ctrl => self.open_palette(),
            KeyCode::Char('e') if self.focus != Focus::Runs => self.edit(),
            KeyCode::Char('v') => {
                self.run_line(palette::Entry::new(palette::Kind::Rpt, "validate", "").id(), "rpt validate")
            }
            KeyCode::Tab => self.focus_next(),
            KeyCode::Char('1') => self.set_focus(Focus::Findings),
            KeyCode::Char('2') => self.set_focus(Focus::Problems),
            KeyCode::Char('3') => self.set_focus(Focus::Runs),
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
        let area = f.area();
        self.runs.full = crate::run::Size { rows: area.height, cols: area.width };
        if let Some(i) = self.runs.takeover {
            home::draw_takeover(f, self, i);
        } else if let Some(pane) = home::draw(f, self) {
            self.runs.pane = pane;
        }
    }
}

/// Modes a raw-takeover child may have left on the real terminal: mouse, bracketed paste,
/// application cursor and keypad, cursor shape and visibility, SGR, focus events, kitty
/// keyboard and synchronized output.
const RESET_CHILD_MODES: &[u8] = b"\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l\x1b[?1015l\x1b[?2004l\
\x1b[?1l\x1b>\x1b[?25h\x1b[0m\x1b[?1004l\x1b[<u\x1b[?2026l\x1b[0 q";

/// The `toggle_takeover` key (ctrl-t) as a raw byte.
const CTRL_T: u8 = 0x14;

/// While runs are live the screen is redrawn at least this often, so marks like
/// `◆ quiet 12s` move with no input.
const LIVE_REDRAW: Duration = Duration::from_millis(500);

/// Redraws at most this often (about 60 fps).
const FRAME: Duration = Duration::from_millis(16);

/// Runs the TUI until the user quits; returns the process exit code.
pub fn run(report: &Path, rpt_bin: &Path) -> i32 {
    let config =
        crate::config::path(std::env::var_os("XDG_CONFIG_HOME").as_deref(), std::env::var_os("HOME").as_deref())
            .map(|p| crate::config::load_file(&p))
            .unwrap_or_default();
    let mut app = App::with_config(Client::new(rpt_bin, report), config);
    let mut terminal = ratatui::init();
    let result = main_loop(&mut terminal, &mut app);
    app.kill_all();
    let _ = execute!(std::io::stdout(), DisableMouseCapture, DisableBracketedPaste);
    ratatui::restore();
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("rpt-tui: {e}");
            1
        }
    }
}

fn main_loop(terminal: &mut DefaultTerminal, app: &mut App) -> std::io::Result<()> {
    execute!(std::io::stdout(), EnableMouseCapture, EnableBracketedPaste)?;
    terminal.draw(|f| app.draw(f))?;
    let mut last_draw = Instant::now();
    let mut was_raw = false;
    while !app.should_quit() {
        if app.tick() || (app.any_live() && last_draw.elapsed() >= LIVE_REDRAW) {
            app.runs.dirty.store(true, Ordering::Release);
        }
        if app.runs.raw_takeover().is_some() {
            if !was_raw {
                enter_raw(app)?;
                was_raw = true;
            }
            raw_step(app)?;
            continue;
        }
        if was_raw {
            leave_raw(terminal)?;
            was_raw = false;
            app.runs.dirty.store(true, Ordering::Release);
        }
        if app.runs.dirty.load(Ordering::Acquire) && last_draw.elapsed() >= FRAME {
            app.runs.dirty.store(false, Ordering::Release);
            terminal.draw(|f| app.draw(f))?;
            last_draw = Instant::now();
        }
        if event::poll(FRAME)? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => app.handle_key(k),
                Event::Mouse(m) => app.handle_mouse(m),
                Event::Paste(text) => app.handle_paste(&text),
                _ => {}
            }
            app.runs.dirty.store(true, Ordering::Release);
        }
    }
    if was_raw {
        leave_raw(terminal)?;
    }
    Ok(())
}

/// Hands the real terminal to the taken-over run: our mouse and paste modes off, then the
/// run's screen and modes repainted, then its output tee'd straight through.
fn enter_raw(app: &mut App) -> std::io::Result<()> {
    execute!(std::io::stdout(), DisableMouseCapture, DisableBracketedPaste)?;
    let full = app.runs.full;
    if let Some(run) = app.runs.raw_takeover() {
        run.resize(full);
        run.start_tee(Box::new(std::io::stdout()));
    }
    Ok(())
}

/// One step of raw takeover: crossterm isn't polled; stdin bytes go straight to the run,
/// except ctrl-t, which leaves takeover.
fn raw_step(app: &mut App) -> std::io::Result<()> {
    let (cols, rows) = crossterm::terminal::size()?;
    let size = crate::run::Size { rows, cols };
    if size != app.runs.full {
        app.runs.full = size;
        if let Some(run) = app.runs.raw_takeover() {
            run.resize(size);
        }
    }
    let mut fds = libc::pollfd { fd: libc::STDIN_FILENO, events: libc::POLLIN, revents: 0 };
    // SAFETY: one valid pollfd for stdin.
    if unsafe { libc::poll(&mut fds, 1, 30) } <= 0 {
        return Ok(());
    }
    let mut buf = [0u8; 4096];
    // SAFETY: reads at most buf.len() bytes into buf.
    let n = unsafe { libc::read(libc::STDIN_FILENO, buf.as_mut_ptr().cast(), buf.len()) };
    if n <= 0 {
        return Ok(());
    }
    let data = &buf[..n as usize];
    let Some(run) = app.runs.raw_takeover() else { return Ok(()) };
    // Every byte goes to the run except ctrl-t, which leaves takeover (the run keeps the keys).
    let toggled = data.contains(&CTRL_T);
    run.send(data.iter().copied().filter(|&b| b != CTRL_T).collect::<Vec<u8>>());
    if toggled {
        app.runs.leave_takeover();
    }
    Ok(())
}

/// Takes the real terminal back: the child's modes reset, ours on again, a full redraw.
fn leave_raw(terminal: &mut DefaultTerminal) -> std::io::Result<()> {
    let mut out = std::io::stdout();
    out.write_all(RESET_CHILD_MODES)?;
    out.flush()?;
    execute!(out, EnableMouseCapture, EnableBracketedPaste)?;
    // Not `terminal.clear()`: it asks the terminal for the cursor position, and fails when
    // nothing answers. A full-screen resize clears and forces a full redraw without asking.
    let (cols, rows) = crossterm::terminal::size()?;
    terminal.resize(ratatui::layout::Rect::new(0, 0, cols, rows))
}
