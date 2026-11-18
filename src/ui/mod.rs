//! The terminal UI: app state, keys and the main loop. Drawing is in `home`.

mod chains;
mod commands;
pub mod edit;
pub mod form;
mod guide;
mod history;
mod home;
pub mod palette;
pub mod picker;
mod runs;
mod term;
pub mod theme;
mod time;

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
            self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event,
            KeyEvent, KeyEventKind,
        },
        execute,
    },
};

use crate::config::{Chain, Config, KeySetting, Needs};
use crate::keys::Bindings;
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

/// The focused panel; keys can be scoped to panels too.
pub use crate::keys::Panel as Focus;

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
    /// `keys:` from the config, as written.
    key_settings: Vec<KeySetting>,
    /// The live bindings, from `keys:` and the chains' hotkeys.
    bindings: Bindings,
    /// The selected finding or problem is shown full width (`open`).
    full: bool,
    /// How far the full view is scrolled, and how far it can go (set as it is drawn).
    scroll: u16,
    scroll_max: std::cell::Cell<u16>,
    /// Help is open, scrolled this far; and how far it can go (set as it is drawn).
    help: Option<u16>,
    help_max: std::cell::Cell<u16>,
    /// A y/n question waiting for its answer.
    confirm: Option<Confirm>,
    /// The key guide, when open.
    guide: Option<guide::Guide>,
    /// The config file, which the guide's fixes are saved to.
    config_path: Option<PathBuf>,
    /// The config file was backed up before the guide's first save.
    backed_up: bool,
    /// This report's run history; none when `history.dir` is unknown or can't be made.
    history: Option<crate::history::Store>,
    /// How much history keeps; the transcript cap applies even with history off.
    history_limits: crate::history::Limits,
    /// The last transcript replayed for drawing.
    replay: std::cell::RefCell<Option<history::Replay>>,
}

/// `word`, with an `s` unless `n` is one.
fn plural(n: usize, word: &str) -> String {
    if n == 1 { word.to_string() } else { format!("{word}s") }
}

/// What a y/n question asks to do.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Confirm {
    /// Quit, killing the live runs.
    Quit,
    /// Delete the finished run with this id.
    Delete(u64),
    /// Empty this report's history.
    Clear,
    /// Write a run's transcript over a file.
    Overwrite { run: u64, path: PathBuf },
}

/// The environment variables a run gets for the selection.
const SELECTION_VARS: [&str; 4] = ["RPT_FINDING_ID", "RPT_FINDING_FILE", "RPT_PROBLEM_FILE", "RPT_PROBLEM_LINE"];

/// The selection: the highlighted row of whichever of Findings and Problems was focused last.
pub enum Selected<'a> {
    Finding(&'a Finding),
    Problem(&'a Problem),
}

impl App {
    /// Reads the report through `client` and starts listing plugins in the background.
    pub fn new(client: Client) -> App {
        App::with_config(client, Config::default())
    }

    /// As [`App::with_config`], from the config file at `path` (missing means no config); the
    /// key guide saves its fixes there. Run history is kept only where `history.dir` says.
    pub fn from_config_file(client: Client, path: &Path) -> App {
        let mut app = App::with_config(client, crate::config::load_file(path));
        app.config_path = Some(path.to_path_buf());
        app
    }

    /// As [`App::new`], with chains and settings from `config`; its errors join the startup
    /// toast, and key troubles open the guide.
    pub fn with_config(client: Client, config: Config) -> App {
        let view = ReportView::load(&client, client.report());
        let bindings = Bindings::new(&config.keys, &config.chains);
        let unfixable = bindings.troubles.iter().filter(|t| t.side().is_none()).map(|t| t.to_string());
        let errors: Vec<String> = view.errors.iter().chain(&config.errors).cloned().chain(unfixable).collect();
        let mut errors = errors;
        let root = std::path::absolute(client.report()).unwrap_or_else(|_| client.report().to_path_buf());
        let limits = config.history.limits;
        let history = config.history.dir.as_deref().and_then(|dir| {
            crate::history::Store::open(dir, &root, limits)
                .map_err(|e| errors.push(format!("run history is off: can't use {}: {e}", dir.display())))
                .ok()
        });
        let toast = (!errors.is_empty()).then(|| errors.join("; "));
        let recent = history.as_ref().map(|h| h.recent()).unwrap_or_default();
        let mut app = App {
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
            recent,
            editor_env: (std::env::var("VISUAL").ok(), std::env::var("EDITOR").ok()),
            bindings,
            key_settings: config.keys,
            chains: config.chains,
            file_picker: config.file_picker,
            picking: None,
            list_picker: None,
            full: false,
            scroll: 0,
            scroll_max: std::cell::Cell::new(0),
            help: None,
            help_max: std::cell::Cell::new(0),
            confirm: None,
            guide: None,
            config_path: None,
            backed_up: false,
            history,
            history_limits: limits,
            replay: Default::default(),
        };
        app.load_history();
        app.open_guide();
        app
    }

    /// Builds the bindings again from `keys:` and the chains.
    fn rebind(&mut self) {
        self.bindings = Bindings::new(&self.key_settings, &self.chains);
    }

    pub fn selection(&self) -> Option<Selected<'_>> {
        match self.last_list {
            Focus::Problems => self.selected_problem().map(Selected::Problem),
            _ => self.selected_finding().map(Selected::Finding),
        }
    }

    /// The selection as environment variables for a run; only the selected kind's are set.
    fn selection_env(&self) -> Vec<(String, String)> {
        let abs = |path: &str| self.view.root.join(path).display().to_string();
        match self.selection() {
            None => vec![],
            Some(Selected::Finding(f)) => {
                vec![("RPT_FINDING_ID".into(), f.id.clone()), ("RPT_FINDING_FILE".into(), abs(&f.path))]
            }
            Some(Selected::Problem(p)) => {
                let mut env = vec![];
                if !p.path.is_empty() {
                    env.push(("RPT_PROBLEM_FILE".into(), abs(&p.path)));
                    if let Some(line) = p.line {
                        env.push(("RPT_PROBLEM_LINE".into(), line.to_string()));
                    }
                }
                env
            }
        }
    }

    /// Why a chain with these `needs` can't run now, if it can't: `needs a finding`.
    fn unmet(&self, needs: Option<Needs>) -> Option<&'static str> {
        let sel = self.selection();
        match needs? {
            Needs::Finding if !matches!(sel, Some(Selected::Finding(_))) => Some("needs a finding"),
            Needs::Problem if !matches!(sel, Some(Selected::Problem(_))) => Some("needs a problem"),
            Needs::Any if sel.is_none() => Some("needs a finding or problem"),
            _ => None,
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
        self.full = false;
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

    /// `quit`: at once, or after a y/n when runs are live.
    fn request_quit(&mut self) {
        if self.any_live() {
            self.confirm = Some(Confirm::Quit);
        } else {
            self.quit = true;
        }
    }

    /// Adds `msg` to the toast.
    fn add_toast(&mut self, msg: String) {
        self.toast = Some(match self.toast.take() {
            Some(t) => format!("{t}; {msg}"),
            None => msg,
        });
    }

    /// The question `c` asks, as shown.
    fn question(&self, c: &Confirm) -> String {
        match c {
            Confirm::Quit => {
                let n = self.runs.entries.iter().filter(|e| e.is_live()).count();
                format!("kill {n} live {} and quit? y/n", plural(n, "run"))
            }
            Confirm::Delete(id) => {
                let label = self.runs.entries.iter().find(|e| e.id == *id).map_or("", |e| e.label.as_str());
                format!("delete run {label}? y/n")
            }
            Confirm::Clear => {
                let n = self.runs.entries.iter().filter(|e| e.saved.is_some()).count();
                format!("clear {n} finished {} from this report's history? y/n", plural(n, "run"))
            }
            Confirm::Overwrite { path, .. } => format!("overwrite {}? y/n", path.display()),
        }
    }

    /// `y` to a question.
    fn confirmed(&mut self, c: Confirm) {
        match c {
            Confirm::Quit => self.quit = true,
            Confirm::Delete(id) => self.delete_run(id),
            Confirm::Clear => self.clear_history(),
            Confirm::Overwrite { run, path } => self.write_transcript(run, &path),
        }
    }

    /// Keys while help is open: scroll, or close with esc or `help`'s key.
    fn help_key(&mut self, key: KeyEvent) {
        use ratatui::crossterm::event::KeyCode;
        let max = self.help_max.get();
        let Some(at) = self.help.as_mut() else { return };
        let closes = self.bindings.keys(crate::keys::Action::Help).contains(&crate::keys::Key::of(&key));
        match key.code {
            KeyCode::Esc => self.help = None,
            _ if closes => self.help = None,
            KeyCode::Down | KeyCode::Char('j') => *at = (*at + 1).min(max),
            KeyCode::Up | KeyCode::Char('k') => *at = at.saturating_sub(1),
            _ => {}
        }
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
        // Any key dismisses the toast; real toast timing comes with notifications.
        self.toast = None;
        if self.guide.is_some() {
            self.guide_key(key);
            return;
        }
        if let Some(c) = self.confirm.take() {
            if key.code == ratatui::crossterm::event::KeyCode::Char('y') {
                self.confirmed(c);
            }
            return;
        }
        if self.help.is_some() {
            self.help_key(key);
            return;
        }
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
        if let Some(action) = self.bindings.action(&key, self.focus) {
            self.act(action);
        } else if let Some(name) = self.bindings.chain(&key).map(String::from) {
            self.hotkey(&name);
        }
    }

    fn list(&mut self) -> Option<(&mut usize, usize)> {
        match self.focus {
            Focus::Findings => Some((&mut self.findings_sel, self.view.findings.len())),
            Focus::Problems => Some((&mut self.problems_sel, self.view.problems.len())),
            Focus::Runs => Some((&mut self.runs.sel, self.runs.entries.len())),
        }
    }

    fn move_by(&mut self, d: isize) {
        if self.full {
            self.scroll = self.scroll.saturating_add_signed(d as i16).min(self.scroll_max.get());
        } else if self.focus == Focus::Runs {
            self.runs.step(d);
        } else if let Some((sel, len)) = self.list() {
            *sel = sel.saturating_add_signed(d).min(len.saturating_sub(1));
        }
    }

    fn move_to(&mut self, i: usize) {
        if self.full {
            self.scroll = if i == 0 { 0 } else { self.scroll_max.get() };
        } else if self.focus == Focus::Runs {
            self.runs.select_end(i != 0);
        } else if let Some((sel, len)) = self.list() {
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

/// While runs are live the screen is redrawn at least this often, so marks like
/// `◆ quiet 12s` move with no input.
const LIVE_REDRAW: Duration = Duration::from_millis(500);

/// Redraws at most this often (about 60 fps).
const FRAME: Duration = Duration::from_millis(16);

/// Runs the TUI until the user quits; returns the process exit code.
pub fn run(report: &Path, rpt_bin: &Path) -> i32 {
    let config_path =
        crate::config::path(std::env::var_os("XDG_CONFIG_HOME").as_deref(), std::env::var_os("HOME").as_deref());
    let client = Client::new(rpt_bin, report);
    let mut config = config_path.as_deref().map(crate::config::load_file).unwrap_or_default();
    config.history.dir = crate::config::history_dir(
        config.history.dir.as_deref(),
        std::env::var_os("XDG_STATE_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    );
    if config.history.dir.is_none() {
        config.errors.push("run history is off: neither HOME nor XDG_STATE_HOME is set".into());
    }
    let mut app = App::with_config(client, config);
    app.config_path = config_path;
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
/// except `toggle_takeover`, which leaves takeover.
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
    let toggle = app.bindings.toggle_bytes();
    let Some(run) = app.runs.raw_takeover() else { return Ok(()) };
    // Every byte goes to the run except `toggle_takeover`'s, which leave takeover (the run
    // keeps the keys).
    let toggled = data.iter().any(|b| toggle.contains(b));
    run.send(data.iter().copied().filter(|b| !toggle.contains(b)).collect::<Vec<u8>>());
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
