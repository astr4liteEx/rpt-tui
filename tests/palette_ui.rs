//! The command palette in the app: plugins loading in the background, the argument line,
//! launching, recents and refreshing the report after a run.
mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
};
use rpt_tui::rpt::Client;
use rpt_tui::ui::App;

/// A stand-in plugin answering `--describe` with `describe`, otherwise running `body`.
fn plugin(dir: &Path, name: &str, describe: &str, body: &str) {
    common::stand_in_rpt(
        dir,
        &format!("rpt-{name}"),
        &format!("if [ \"$1\" = --describe ]; then {describe}; exit 0; fi\n{body}\n"),
    );
}

struct Fixture {
    app: App,
    term: Terminal<TestBackend>,
    report: tempfile::TempDir,
    _plugins: tempfile::TempDir,
}

/// The fixture report, with `greet` and `odd` plugins and nothing else on rpt's PATH.
fn fixture() -> Fixture {
    let plugins = tempfile::tempdir().unwrap();
    plugin(
        plugins.path(),
        "greet",
        r#"echo '{"summary":"Say hello","usage":"rpt greet <name>","input":"text","output":"records"}'"#,
        r#"echo "hello $*""#,
    );
    plugin(plugins.path(), "odd", r#"echo '{"summary":"Odd usage","usage":"odd-tool FILE"}'"#, "true");
    with_plugins(plugins)
}

fn with_plugins(plugins: tempfile::TempDir) -> Fixture {
    let report = common::fixture();
    // The plugins, then just enough system for bash: no plugins the machine happens to have.
    let path = format!("{}:/usr/bin:/bin", plugins.path().display());
    let client = Client::new(common::rpt_bin(), report.path()).with_path(path.as_ref());
    with_app(App::new(client), report, plugins)
}

fn with_app(app: App, report: tempfile::TempDir, plugins: tempfile::TempDir) -> Fixture {
    Fixture { app, term: Terminal::new(TestBackend::new(100, 24)).unwrap(), report, _plugins: plugins }
}

impl Fixture {
    fn draw(&mut self) -> String {
        self.term.draw(|f| self.app.draw(f)).unwrap();
        self.app.tick();
        let buf = self.term.backend().buffer();
        let text: Vec<String> = (0..buf.area.height).map(|y| common::row_text(buf, y)).collect();
        text.join("\n").replace(&self.report.path().display().to_string(), "<report>")
    }

    fn until(&mut self, text: &str) -> String {
        let t0 = Instant::now();
        loop {
            let s = self.draw();
            if s.contains(text) || t0.elapsed() > Duration::from_secs(8) {
                return s;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn key(&mut self, code: KeyCode) {
        self.app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn ctrl(&mut self, c: char) {
        self.app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
    }

    fn typ(&mut self, text: &str) {
        for c in text.chars() {
            self.key(KeyCode::Char(c));
        }
    }
}

#[test]
fn colon_opens_the_palette_with_each_group_and_the_plugins_once_listed() {
    let mut fx = fixture();
    fx.draw();

    fx.typ(":");
    let s = fx.until("Say hello");

    insta::assert_snapshot!(s);
}

#[test]
fn the_plugins_group_shows_loading_until_rpt_plugins_returns() {
    let plugins = tempfile::tempdir().unwrap();
    plugin(plugins.path(), "slowpoke", r#"sleep 1; echo '{"summary":"Takes its time"}'"#, "true");
    let mut fx = with_plugins(plugins);

    fx.ctrl('p');
    let s = fx.draw();
    assert!(s.contains("loading plugins…"), "{s}");

    let s = fx.until("Takes its time");
    assert!(!s.contains("loading plugins…"), "{s}");
}

#[test]
fn choosing_a_plugin_opens_its_argument_line_prefilled_from_usage() {
    let mut fx = fixture();
    fx.typ(":");
    fx.until("Say hello");

    fx.typ("greet");
    fx.key(KeyCode::Enter);
    let s = fx.draw();

    insta::assert_snapshot!(s);
}

#[test]
fn a_usage_not_starting_with_its_verb_is_a_hint_and_the_line_starts_empty() {
    let mut fx = fixture();
    fx.typ(":");
    fx.until("Odd usage");

    fx.typ("odd");
    fx.key(KeyCode::Enter);
    let s = fx.draw();

    assert!(s.contains("usage: odd-tool FILE"), "{s}");
    let line = s.lines().find(|l| l.contains("rpt odd ")).expect("the argument line");
    assert!(!line.contains("odd-tool"), "{line}");
}

#[test]
fn enter_on_the_argument_line_runs_the_plugin_in_the_pane_through_the_shell() {
    let mut fx = fixture();
    fx.typ(":");
    fx.until("Say hello");
    fx.typ("greet");
    fx.key(KeyCode::Enter);

    fx.ctrl('u');
    fx.typ("'big world' $RPT_REPORT_DIR");
    fx.key(KeyCode::Enter);
    let s = fx.until("hello big world");

    assert!(s.contains("hello big world <report>"), "{s}");
    assert!(s.contains("rpt greet 'big world' $RPT_REPORT_DIR"), "the run is labelled with its command: {s}");
}

#[test]
fn esc_on_the_argument_line_goes_back_to_the_list_with_the_query_kept_and_esc_again_closes() {
    let mut fx = fixture();
    fx.typ(":");
    fx.until("Say hello");
    fx.typ("greet");
    fx.key(KeyCode::Enter);

    fx.key(KeyCode::Esc);
    let s = fx.draw();
    assert!(s.contains("> greet"), "{s}");
    assert!(s.contains("Say hello"), "{s}");

    fx.key(KeyCode::Esc);
    let s = fx.draw();
    assert!(!s.contains("Say hello"), "{s}");
}

#[test]
fn set_from_the_palette_changes_the_report_and_the_findings_refresh() {
    let mut fx = fixture();
    let s = fx.draw();
    assert!(s.find("F-003 ●").unwrap() < s.find("F-002 ●").unwrap(), "{s}");

    fx.typ(":set");
    fx.key(KeyCode::Enter);
    fx.ctrl('u');
    fx.typ("F-002 severity critical");
    fx.key(KeyCode::Enter);
    let s = fx.until("● critical");

    assert!(s.find("F-002 ●").unwrap() < s.find("F-003 ●").unwrap(), "re-sorted after the run: {s}");
}

#[test]
fn a_tui_action_runs_from_the_palette() {
    let mut fx = fixture();
    fx.typ(":focus_runs");
    fx.key(KeyCode::Enter);

    let s = fx.draw();

    assert!(s.contains("No runs yet."), "{s}");
    assert!(s.lines().last().unwrap().contains("f takeover"), "focus moved to Runs: {s}");
}

#[test]
fn a_command_used_this_session_is_listed_first_under_recent() {
    let mut fx = fixture();
    fx.typ(":version");
    fx.key(KeyCode::Enter);
    fx.until("✔ 0");
    fx.ctrl('g');

    fx.typ(":");
    let s = fx.until("Say hello");

    let recent = s.find("recent").expect("a recent header");
    assert!(recent < s.find("version").unwrap() && s.find("version").unwrap() < s.find("chains").unwrap(), "{s}");
}

#[test]
fn v_runs_validate_in_the_pane() {
    let mut fx = fixture();

    fx.typ("v");
    let s = fx.until("S-99");

    assert!(s.contains("rpt validate"), "{s}");
}

#[test]
fn rpt_rows_come_from_rpt_verbs_with_rpt_s_own_summary() {
    let mut fx = fixture();

    fx.typ(":sections");
    let s = fx.draw();

    assert!(s.contains("rpt    sections           List the report's sections, headings"), "{s}");
}

#[test]
fn add_new_plugins_and_verbs_are_not_offered_as_rpt_rows() {
    let mut fx = fixture();
    fx.until("Say hello"); // plugins listed, so the palette is complete
    for (verb, summary) in [
        ("add", "Add finding records as new finding files"),
        ("new", "Create an empty report"),
        ("plugins", "List the plugins on PATH"),
        ("verbs", "List rpt's built-in verbs"),
    ] {
        fx.typ(&format!(":{verb}"));
        let s = fx.draw();
        assert!(!s.contains(summary), "{verb} is offered: {s}");
        fx.key(KeyCode::Esc);
    }
}

#[test]
fn a_verb_whose_usage_takes_no_placeholder_runs_at_once() {
    let mut fx = fixture();

    fx.typ(":sections");
    fx.key(KeyCode::Enter);
    let s = fx.until("HEADING");

    assert!(s.contains("HEADING"), "rpt sections ran in a pane: {s}");
}

#[test]
fn when_rpt_verbs_fails_a_toast_says_why_and_there_are_no_rpt_rows_or_verb_checks() {
    let report = common::fixture();
    // A stand-in rpt whose verbs verb fails; every other verb goes to the real rpt.
    let fake = common::stand_in_rpt(
        report.path(),
        "rpt-no-verbs",
        &format!(
            "if [ \"$1\" = verbs ]; then echo 'rpt: verbs broke' >&2; exit 2; fi\nexec {} \"$@\"\n",
            common::rpt_bin().display()
        ),
    );
    let config = rpt_tui::config::load("chains:\n  new:\n    description: My own new\n    steps: [true]\n");
    let app = App::with_config(Client::new(&fake, report.path()), config);
    let mut fx = with_app(app, report, tempfile::tempdir().unwrap());

    let home = fx.draw();
    fx.typ(":validate");
    let validate = fx.draw();
    fx.key(KeyCode::Esc);
    fx.typ(":new");
    let new = fx.draw();

    assert!(home.contains("verbs broke"), "{home}");
    assert!(!validate.contains("rpt    validate"), "{validate}");
    assert!(new.contains("My own new"), "the chain isn't checked against verbs: {new}");
}
