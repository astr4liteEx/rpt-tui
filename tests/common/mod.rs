//! Shared test helpers: the real rpt from ../rpt/dist and a writable copy of rpt's fixture report.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

const RPT_SOURCE: &str = "../rpt";
const FIXTURE: &str = "../rpt/testdata/fixture";

/// Folder holding an `rpt` built from ../rpt's source (once per test run): the dist
/// archive predates `rpt severities`.
pub fn rpt_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("rpt-src");
        std::fs::create_dir_all(&dir).unwrap();
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join(RPT_SOURCE);
        let ok = Command::new("go")
            .args(["build", "-trimpath", "-o"])
            .arg(dir.join("rpt"))
            .arg("./cmd/rpt")
            .current_dir(&src)
            .env("CGO_ENABLED", "0")
            .status()
            .expect("tests need Go to build rpt from ../rpt");
        assert!(ok.success(), "could not build rpt from {}", src.display());
        dir
    })
}

pub fn rpt_bin() -> PathBuf {
    rpt_dir().join("rpt")
}

/// A fresh, writable copy of rpt's fixture report.
pub fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE), tmp.path());
    tmp
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dst = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dst);
        } else {
            std::fs::copy(entry.path(), dst).unwrap();
        }
    }
}

/// Writes an executable shell script standing in for rpt into `dir` and returns its path.
pub fn stand_in_rpt(dir: &Path, name: &str, script: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{script}")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Clock times (`14:03:22`) written as `hh:mm:ss`, so screens with runs compare the same.
pub fn mask_clock(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let d = |i: usize| c.get(i).is_some_and(|x| x.is_ascii_digit());
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if d(i)
            && d(i + 1)
            && c.get(i + 2) == Some(&':')
            && d(i + 3)
            && d(i + 4)
            && c.get(i + 5) == Some(&':')
            && d(i + 6)
            && d(i + 7)
        {
            out.push_str("hh:mm:ss");
            i += 8;
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}

/// One row of a ratatui buffer as text, the way a terminal shows it: the cell after a
/// wide character is hidden behind it, so it is skipped.
pub fn row_text(buf: &ratatui::buffer::Buffer, y: u16) -> String {
    let mut out = String::new();
    let mut x = 0;
    while x < buf.area.width {
        let symbol = buf[(x, y)].symbol();
        out.push_str(symbol);
        x += unicode_width::UnicodeWidthStr::width(symbol).max(1) as u16;
    }
    out.trim_end().to_string()
}

/// The app in a 100×24 `TestBackend`, driven by keys and read back as text.
pub struct AppScreen {
    pub app: rpt_tui::ui::App,
    pub term: ratatui::Terminal<ratatui::backend::TestBackend>,
    pub report: tempfile::TempDir,
    pub plugins: tempfile::TempDir,
    /// The folder holding `config.yaml`, when the app was started from a file.
    pub config_dir: Option<tempfile::TempDir>,
    /// The config text, when the app was started from text.
    config_text: Option<String>,
}

/// A stand-in plugin in `dir` answering `--describe` with `describe`, otherwise running `body`.
pub fn plugin(dir: &Path, name: &str, describe: &str, body: &str) {
    stand_in_rpt(
        dir,
        &format!("rpt-{name}"),
        &format!("if [ \"$1\" = --describe ]; then {describe}; exit 0; fi\n{body}\n"),
    );
}

impl AppScreen {
    /// The fixture report with `config` (YAML) and the plugins in `plugins` alone on rpt's PATH
    /// (plus enough system for bash).
    pub fn new(config: &str, plugins: tempfile::TempDir) -> AppScreen {
        let mut s = AppScreen::start(plugins, None, |client| {
            rpt_tui::ui::App::with_config(client, rpt_tui::config::load(config))
        });
        s.config_text = Some(config.to_string());
        s
    }

    /// The client for this screen's report, with its plugins alone on rpt's PATH.
    fn client(report: &Path, plugins: &Path) -> rpt_tui::rpt::Client {
        let path = format!("{}:/usr/bin:/bin", plugins.display());
        rpt_tui::rpt::Client::new(rpt_bin(), report).with_path(path.as_ref())
    }

    /// Quits (killing live runs) and starts a new session on the same report and config.
    pub fn restart(&mut self) {
        self.app.kill_all();
        let config = rpt_tui::config::load(self.config_text.as_deref().expect("started from text"));
        self.app = rpt_tui::ui::App::with_config(AppScreen::client(self.report.path(), self.plugins.path()), config);
    }

    /// The fixture report and `plugins` on rpt's PATH, with the app `make` builds from a client.
    fn start(
        plugins: tempfile::TempDir,
        config_dir: Option<tempfile::TempDir>,
        make: impl FnOnce(rpt_tui::rpt::Client) -> rpt_tui::ui::App,
    ) -> AppScreen {
        let report = fixture();
        let app = make(AppScreen::client(report.path(), plugins.path()));
        let term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();
        AppScreen { app, term, report, plugins, config_dir, config_text: None }
    }

    /// As [`AppScreen::new`], started from `config` written to a `config.yaml` it may edit.
    pub fn from_file(config: &str) -> AppScreen {
        AppScreen::from_files(config, &[])
    }

    /// As [`AppScreen::from_file`], with more files (`themes/night.yaml`) beside it.
    pub fn from_files(config: &str, files: &[(&str, &str)]) -> AppScreen {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.yaml");
        std::fs::write(&file, config).unwrap();
        for (name, text) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        AppScreen::start(tempfile::tempdir().unwrap(), Some(dir), |client| {
            rpt_tui::ui::App::from_config_file(client, &file)
        })
    }

    /// The config file as it now is.
    pub fn config(&self) -> String {
        std::fs::read_to_string(self.config_dir.as_ref().unwrap().path().join("config.yaml")).unwrap()
    }

    /// The backup of the config file, if one was made.
    pub fn backup(&self) -> Option<String> {
        std::fs::read_to_string(self.config_dir.as_ref().unwrap().path().join("config.yaml.bak")).ok()
    }

    pub fn draw(&mut self) -> String {
        self.term.draw(|f| self.app.draw(f)).unwrap();
        self.app.tick();
        let buf = self.term.backend().buffer();
        let text: Vec<String> = (0..buf.area.height).map(|y| row_text(buf, y)).collect();
        mask_clock(&text.join("\n").replace(&self.report.path().display().to_string(), "<report>"))
    }

    /// Draws until the screen contains `text`, for up to eight seconds.
    pub fn until(&mut self, text: &str) -> String {
        let t0 = std::time::Instant::now();
        loop {
            let s = self.draw();
            if s.contains(text) || t0.elapsed() > std::time::Duration::from_secs(8) {
                return s;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    pub fn key(&mut self, code: ratatui::crossterm::event::KeyCode) {
        use ratatui::crossterm::event::{KeyEvent, KeyModifiers};
        self.app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    pub fn ctrl(&mut self, c: char) {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        self.app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
    }

    pub fn typ(&mut self, text: &str) {
        for c in text.chars() {
            self.key(ratatui::crossterm::event::KeyCode::Char(c));
        }
    }

    /// Opens the palette, waits for plugins, and chooses the entry `query` ranks first.
    pub fn choose(&mut self, query: &str) {
        self.typ(":");
        self.until_plugins();
        self.typ(query);
        self.key(ratatui::crossterm::event::KeyCode::Enter);
    }

    fn until_plugins(&mut self) {
        let t0 = std::time::Instant::now();
        while self.draw().contains("loading plugins…") && t0.elapsed() < std::time::Duration::from_secs(8) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}
