//! The UI with runs: real short-lived children drawn into ratatui's TestBackend.
mod common;

use std::time::{Duration, Instant};

use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
};
use rpt_tui::rpt::Client;
use rpt_tui::ui::{App, Display, ReportView};

fn fixture_app() -> (App, tempfile::TempDir) {
    let report = common::fixture();
    let client = Client::new(common::rpt_bin(), report.path());
    let view = ReportView::load(&client, report.path());
    (App::new(view), report)
}

struct Screen {
    term: Terminal<TestBackend>,
    report: String,
}

impl Screen {
    fn new(report: &std::path::Path) -> Screen {
        Screen { term: Terminal::new(TestBackend::new(100, 24)).unwrap(), report: report.display().to_string() }
    }

    /// Draws a frame (which also tells the app the pane size), then lets runs update.
    fn draw(&mut self, app: &mut App) -> String {
        self.term.draw(|f| app.draw(f)).unwrap();
        app.tick();
        let buf = self.term.backend().buffer();
        let text: Vec<String> = (0..buf.area.height).map(|y| common::row_text(buf, y)).collect();
        text.join("\n").replace(&self.report, "<report>")
    }

    /// Draws until the screen contains `text`, for up to five seconds.
    fn until(&mut self, app: &mut App, text: &str) -> String {
        let t0 = Instant::now();
        loop {
            let s = self.draw(app);
            if s.contains(text) || t0.elapsed() > Duration::from_secs(5) {
                return s;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn sh(script: &str) -> Vec<String> {
    vec!["sh".into(), "-c".into(), script.into()]
}

fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn ctrl(app: &mut App, c: char) {
    app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
}

#[test]
fn a_launched_run_is_listed_and_drawn_in_the_pane() {
    let (mut app, report) = fixture_app();
    let mut screen = Screen::new(report.path());
    screen.draw(&mut app);

    app.launch("greeter", sh("printf 'hello from a run 日本語\\n'; sleep 5"), Display::Pane);

    let s = screen.until(&mut app, "hello from a run");
    insta::assert_snapshot!(s);
}

#[test]
fn a_finished_run_shows_its_exit_code() {
    let (mut app, report) = fixture_app();
    let mut screen = Screen::new(report.path());
    screen.draw(&mut app);

    app.launch("fails", sh("exit 3"), Display::Pane);
    app.launch("works", sh("true"), Display::Pane);

    let s = screen.until(&mut app, "✔ 0");
    let s = if s.contains("✘ 3") { s } else { screen.until(&mut app, "✘ 3") };
    assert!(s.contains("✘ 3         fails"), "{s}");
    assert!(s.contains("✔ 0         works"), "{s}");
}

#[test]
fn keys_go_to_the_run_until_ctrl_g_gives_them_back() {
    let (mut app, report) = fixture_app();
    let mut screen = Screen::new(report.path());
    screen.draw(&mut app);
    app.launch("reader", sh("read x; echo \"got:$x\"; sleep 5"), Display::Pane);
    screen.until(&mut app, "● live");

    // `q` would quit the TUI; with a run holding the keys it is typed into the run.
    for c in ['q', 'u', 'i', 't'] {
        key(&mut app, KeyCode::Char(c));
    }
    key(&mut app, KeyCode::Enter);
    let s = screen.until(&mut app, "got:quit");
    assert!(s.contains("got:quit"), "{s}");
    assert!(!app.should_quit());

    ctrl(&mut app, 'g');
    assert!(screen.draw(&mut app).lines().last().unwrap().contains("f takeover"));
    key(&mut app, KeyCode::Char('q'));
    assert!(app.should_quit());
}

#[test]
fn enter_gives_the_keys_back_to_a_live_run() {
    let (mut app, report) = fixture_app();
    let mut screen = Screen::new(report.path());
    screen.draw(&mut app);
    app.launch("reader", sh("read x; echo \"got:$x\"; sleep 5"), Display::Pane);
    screen.until(&mut app, "● live");
    ctrl(&mut app, 'g');

    key(&mut app, KeyCode::Enter);
    for c in "hi".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    key(&mut app, KeyCode::Enter);

    assert!(screen.until(&mut app, "got:hi").contains("got:hi"));
}

#[test]
fn ctrl_t_takes_the_run_over_full_screen_and_back() {
    let (mut app, report) = fixture_app();
    let mut screen = Screen::new(report.path());
    screen.draw(&mut app);
    app.launch(
        "sizes",
        sh(r#"echo "a:$(stty size)"; read x; echo "b:$(stty size)"; read y; echo "c:$(stty size)"; sleep 5"#),
        Display::Pane,
    );
    screen.until(&mut app, "a:20 56");

    ctrl(&mut app, 't');
    screen.draw(&mut app);
    key(&mut app, KeyCode::Enter);
    let s = screen.until(&mut app, "b:23 100");
    insta::assert_snapshot!("takeover", s);

    ctrl(&mut app, 't');
    screen.draw(&mut app);
    key(&mut app, KeyCode::Enter);
    let s = screen.until(&mut app, "c:20 56");
    assert!(s.contains("1 Findings") && s.contains("c:20 56"), "back in the pane at its size: {s}");
}

#[test]
fn f_takes_over_and_the_end_of_the_run_returns_to_the_pane() {
    let (mut app, report) = fixture_app();
    let mut screen = Screen::new(report.path());
    screen.draw(&mut app);
    app.launch("short", sh("read x; echo bye"), Display::Pane);
    screen.until(&mut app, "● live");
    ctrl(&mut app, 'g');

    key(&mut app, KeyCode::Char('f'));
    assert!(!screen.draw(&mut app).contains("1 Findings"), "taken over");
    key(&mut app, KeyCode::Enter);

    let s = screen.until(&mut app, "✔ 0");
    assert!(s.contains("1 Findings") && s.contains("✔ 0"), "{s}");
}

#[test]
fn x_kills_the_selected_run() {
    let (mut app, report) = fixture_app();
    let mut screen = Screen::new(report.path());
    screen.draw(&mut app);
    app.launch("sleeper", sh("sleep 60"), Display::Pane);
    screen.until(&mut app, "● live");
    ctrl(&mut app, 'g');

    key(&mut app, KeyCode::Char('x'));

    let s = screen.until(&mut app, "killed");
    assert!(s.contains("killed      sleeper"), "{s}");
}

#[test]
fn j_and_k_choose_which_run_the_pane_shows() {
    let (mut app, report) = fixture_app();
    let mut screen = Screen::new(report.path());
    screen.draw(&mut app);
    app.launch("first", sh("echo one-output; sleep 5"), Display::Pane);
    app.launch("second", sh("echo two-output; sleep 5"), Display::Pane);
    screen.until(&mut app, "two-output");
    ctrl(&mut app, 'g');

    key(&mut app, KeyCode::Char('k'));

    let s = screen.until(&mut app, "one-output");
    assert!(s.contains("╭ first"), "{s}");
}

#[test]
fn a_quiet_live_run_is_marked_quiet() {
    let (mut app, report) = fixture_app();
    let mut screen = Screen::new(report.path());
    screen.draw(&mut app);
    app.launch("waiting", sh("echo prompt; sleep 30"), Display::Pane);
    screen.until(&mut app, "prompt");

    std::thread::sleep(rpt_tui::ui::QUIET_AFTER + Duration::from_millis(200));

    assert!(screen.draw(&mut app).contains("◆ quiet 3s"));
    app.kill_all();
}

#[test]
fn an_emulated_takeover_follows_a_terminal_resize() {
    let (mut app, report) = fixture_app();
    let mut screen = Screen::new(report.path());
    screen.draw(&mut app);
    app.launch("sizes", sh(r#"read x; echo "now:$(stty size)"; sleep 5"#), Display::Pane);
    screen.until(&mut app, "● live");
    ctrl(&mut app, 't');
    screen.draw(&mut app);

    let mut bigger = Screen { term: Terminal::new(TestBackend::new(120, 30)).unwrap(), report: screen.report.clone() };
    bigger.draw(&mut app);
    key(&mut app, KeyCode::Enter);

    let s = bigger.until(&mut app, "now:29 120");
    assert!(s.contains("now:29 120"), "{s}");
}

#[test]
fn ctrl_t_in_the_runs_panel_does_nothing_without_a_run_holding_the_keys() {
    let (mut app, report) = fixture_app();
    let mut screen = Screen::new(report.path());
    screen.draw(&mut app);
    app.launch("sleeper", sh("sleep 5"), Display::Pane);
    screen.until(&mut app, "● live");
    ctrl(&mut app, 'g');

    ctrl(&mut app, 't');

    assert!(screen.draw(&mut app).contains("1 Findings"), "still the home screen");
    app.kill_all();
}
