mod common;

use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
};
use rpt_tui::rpt::Client;
use rpt_tui::ui::{App, ReportView};

/// Loads the fixture report through the real rpt, the way the TUI does at start-up.
fn fixture_app() -> (App, tempfile::TempDir) {
    let report = common::fixture();
    let client = Client::new(common::rpt_bin(), report.path());
    let view = ReportView::load(&client, report.path());
    (App::new(view), report)
}

/// Renders a frame and returns it as text, one line per row, with the temp report path masked.
fn screen(app: &mut App, report: &std::path::Path) -> String {
    let mut term = Terminal::new(TestBackend::new(100, 24)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let buf = term.backend().buffer();
    let text: Vec<String> = (0..buf.area.height).map(|y| common::row_text(buf, y)).collect();
    let report = report.display().to_string();
    text.join("\n").replace(&report, "<report>")
}

fn press(app: &mut App, c: char) {
    app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
}

#[test]
fn home_shows_findings_problems_runs_and_the_selected_preview() {
    let (mut app, report) = fixture_app();

    insta::assert_snapshot!(screen(&mut app, report.path()));
}

#[test]
fn j_moves_down_and_the_preview_follows() {
    let (mut app, report) = fixture_app();

    press(&mut app, 'j');

    let s = screen(&mut app, report.path());
    assert!(s.contains("F-001  Reflected XSS in search"), "{s}");
    assert!(s.contains("The `q` parameter is reflected unescaped."), "{s}");
}

#[test]
fn g_and_shift_g_jump_to_the_ends_and_k_stops_at_the_top() {
    let (mut app, report) = fixture_app();

    press(&mut app, 'G');
    assert!(screen(&mut app, report.path()).contains("F-002  Weak TLS configuration"));
    press(&mut app, 'g');
    press(&mut app, 'k');
    assert!(screen(&mut app, report.path()).contains("F-003  SQL injection in login"));
}

#[test]
fn focusing_problems_shows_the_selected_problem_on_the_right() {
    let (mut app, report) = fixture_app();

    press(&mut app, '2');

    insta::assert_snapshot!(screen(&mut app, report.path()));
}

#[test]
fn tab_cycles_findings_problems_runs() {
    let (mut app, report) = fixture_app();

    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(screen(&mut app, report.path()).contains("scopeIDsExist"));
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(screen(&mut app, report.path()).contains("No run selected."));
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(screen(&mut app, report.path()).contains("Proof of concept"));
}

#[test]
fn an_unknown_schema_version_shows_a_banner() {
    let report = common::fixture();
    // A stand-in rpt that reports schema 2 and passes every other verb to the real rpt.
    let fake = common::stand_in_rpt(
        report.path(),
        "rpt-schema2",
        &format!(
            "if [ \"$1\" = version ]; then echo '{{\"kind\":\"version\",\"version\":\"v9.0.0\",\"schema_version\":2,\"format\":1}}'; exit 0; fi\nexec {} \"$@\"\n",
            common::rpt_bin().display()
        ),
    );
    let client = Client::new(&fake, report.path());
    let mut app = App::new(ReportView::load(&client, report.path()));

    let s = screen(&mut app, report.path());

    assert!(s.lines().next().unwrap().contains("record schema 2"), "{s}");
}

#[test]
fn a_broken_rpt_yaml_is_a_toast_not_a_crash() {
    let report = common::fixture();
    std::fs::write(report.path().join("rpt.yaml"), "format: 1\nseverities: [high, {oops\n").unwrap();
    let client = Client::new(common::rpt_bin(), report.path());
    let mut app = App::new(ReportView::load(&client, report.path()));

    let s = screen(&mut app, report.path());

    assert!(s.contains("rpt.yaml"), "{s}");
}

#[test]
fn a_problem_row_shows_level_message_and_file_line() {
    let (mut app, report) = fixture_app();

    let s = screen(&mut app, report.path());

    let row = s.lines().find(|l| l.contains("✘")).expect("a problem row");
    assert!(row.contains("✘ error"), "{row}");
    assert!(row.contains("findings/tls-weak.md:5"), "{row}");
    assert!(row.contains("S-99"), "{row}");
}

#[test]
fn the_footer_hints_follow_focus() {
    let (mut app, report) = fixture_app();

    assert!(screen(&mut app, report.path()).lines().last().unwrap().contains("j/k move"));
    press(&mut app, '3');
    assert!(screen(&mut app, report.path()).lines().last().unwrap().contains("f takeover"));
}

#[test]
fn findings_are_listed_most_severe_first() {
    let (mut app, report) = fixture_app();

    let s = screen(&mut app, report.path());

    let order: Vec<&str> =
        ["F-003", "F-001", "F-002"].into_iter().filter_map(|id| s.find(&format!("{id} ●")).map(|_| id)).collect();
    let pos = |id: &str| s.find(&format!("{id} ●")).unwrap();
    assert_eq!(order.len(), 3, "{s}");
    assert!(pos("F-003") < pos("F-001") && pos("F-001") < pos("F-002"), "{s}");
}

#[test]
fn an_rpt_without_the_severities_verb_falls_back_to_the_defaults_with_a_toast() {
    let report = common::fixture();
    let fake = common::stand_in_rpt(
        report.path(),
        "rpt-old",
        &format!(
            "if [ \"$1\" = severities ]; then echo 'rpt: unknown verb \"severities\"' >&2; exit 2; fi\nexec {} \"$@\"\n",
            common::rpt_bin().display()
        ),
    );
    let client = Client::new(&fake, report.path());
    let mut app = App::new(ReportView::load(&client, report.path()));

    let s = screen(&mut app, report.path());

    assert!(s.contains("severities"), "a toast names the missing verb: {s}");
    assert!(s.contains("1 high  1 medium  1 info"), "{s}");
}
