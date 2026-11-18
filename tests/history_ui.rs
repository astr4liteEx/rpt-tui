//! Run history in the app: runs survive a restart, and can be opened, re-run, deleted,
//! cleared and exported; remembered values and recent commands carry over too.
mod common;

use std::path::Path;

use common::AppScreen;
use ratatui::crossterm::event::KeyCode;
use ratatui::style::Modifier;

/// Chains for these tests, each on its own hotkey.
const CHAINS: &str = r#"
chains:
  listing:
    key: L
    steps: ['for i in $(seq 1 300); do echo line-$i; done']
  waits:
    key: W
    steps: ['echo started-waiting; sleep 30']
  later:
    key: T
    steps: ['echo later']
  slow:
    key: S
    steps: ['sleep 1.2']
  ask:
    key: a
    params: { WHO: { kind: text, remember: true } }
    steps: ['echo hi $WHO']
"#;

/// The app with history kept in `history`.
fn screen(history: &Path) -> AppScreen {
    AppScreen::new(&format!("history: {{ dir: '{}' }}\n{CHAINS}", history.display()), tempfile::tempdir().unwrap())
}

/// The Runs panel's rows, top to bottom, as drawn.
fn run_rows(out: &str) -> Vec<String> {
    let lines: Vec<&str> = out.lines().collect();
    let top = lines.iter().position(|l| l.contains("3 Runs")).expect(out);
    lines[top + 1..]
        .iter()
        .take_while(|l| !l.starts_with('╰'))
        .map(|l| l.split('│').nth(1).unwrap_or_default().trim().trim_start_matches('▌').to_string())
        .collect()
}

/// Whether the cell where `text` starts on screen is dimmed.
fn dimmed(s: &AppScreen, text: &str) -> bool {
    let buf = s.term.backend().buffer();
    for y in 0..buf.area.height {
        let row = common::row_text(buf, y);
        if let Some(x) = row.find(text) {
            let x = row[..x].chars().count() as u16;
            return buf[(x, y)].modifier.contains(Modifier::DIM);
        }
    }
    panic!("{text} not on screen");
}

#[test]
fn a_finished_run_is_listed_after_a_restart_dimmed() {
    let history = tempfile::tempdir().unwrap();
    let mut s = screen(history.path());
    s.typ("T");
    s.until("✔ 0");
    assert!(!dimmed(&s, "later"), "this session's run");

    s.restart();
    let out = s.draw();

    assert!(run_rows(&out).iter().any(|r| r.starts_with("✔ 0") && r.contains("hh:mm:ss later")), "{out}");
    assert!(dimmed(&s, "later"), "from an earlier session");
}

#[test]
fn live_runs_come_first_then_finished_runs_newest_first() {
    let history = tempfile::tempdir().unwrap();
    let mut s = screen(history.path());
    s.typ("T");
    s.until("✔ 0");
    s.restart();
    s.typ("3");
    s.typ("W");
    s.until("started-waiting");
    s.ctrl('g');
    s.typ("L");
    let out = until_done(&mut s, "listing", 1);

    let rows = run_rows(&out);
    let label = |r: &String| r.split("hh:mm:ss ").nth(1).and_then(|x| x.split_whitespace().next()).map(String::from);
    let labels: Vec<String> = rows.iter().filter_map(label).collect();
    assert_eq!(&labels[..3], ["waits", "listing", "later"], "{out}");
}

#[test]
fn a_finished_row_shows_its_duration_and_the_pane_how_long_ago() {
    let history = tempfile::tempdir().unwrap();
    let mut s = screen(history.path());

    s.typ("S");
    let out = s.until("✔ 0");

    let row = run_rows(&out).into_iter().find(|r| r.contains("slow")).unwrap();
    assert!(row.ends_with(" 1s"), "duration on the right: {row}");
    assert!(out.contains("started hh:mm:ss · 1s · just now"), "{out}");
}

#[test]
fn a_run_from_another_day_shows_its_date() {
    use rpt_tui::history::{Ended, Invocation, Limits, Record, Store};
    let history = tempfile::tempdir().unwrap();
    let s = screen(history.path());
    let two_days_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 24 * 3600);
    let ms = two_days_ago.duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
    let store = Store::open(history.path(), s.report.path(), Limits::default()).unwrap();
    let record = Record {
        label: "older".into(),
        invocation: Invocation { command: "chain later".into(), ..Invocation::default() },
        started_ms: ms,
        duration_ms: 1000,
        ended: Ended::Exited { code: 0, failed_step: None },
        takeover: false,
    };
    store.save(&record, "old output").unwrap();
    let mut s = s;

    s.restart();
    let out = s.draw();

    let row = run_rows(&out).into_iter().find(|r| r.contains("older")).expect(&out);
    let when = row.split_whitespace().nth(2).unwrap_or_default().to_string()
        + " "
        + row.split_whitespace().nth(3).unwrap_or_default();
    let shape: String = when.chars().map(|c| if c.is_ascii_digit() { 'd' } else { c }).collect();
    assert_eq!(shape, "dd-dd dd:dd", "{row}");
}

#[test]
fn open_shows_a_finished_runs_transcript_full_width_and_back_returns() {
    let history = tempfile::tempdir().unwrap();
    let mut s = screen(history.path());
    s.typ("L");
    s.until("✔ 0");

    s.key(KeyCode::Enter);
    let top = s.draw();
    assert!(!top.contains("1 Findings"), "full width: {top}");
    assert!(top.contains("│line-1 ") && !top.contains("line-300"), "from the top: {top}");
    s.typ("G");
    assert!(s.draw().contains("│line-300"), "G goes to the end");
    s.typ("k");
    let up = s.draw();
    assert!(!up.contains("line-300") && up.contains("│line-299"), "k scrolls up: {up}");

    s.key(KeyCode::Esc);
    assert!(s.draw().contains("3 Runs"));
}

#[test]
fn a_run_from_an_earlier_session_shows_its_transcript_too() {
    let history = tempfile::tempdir().unwrap();
    let mut s = screen(history.path());
    s.typ("L");
    s.until("✔ 0");
    s.restart();
    s.typ("3");

    let pane = s.draw();
    assert!(pane.contains("│line-300"), "the pane shows the end: {pane}");
    s.key(KeyCode::Enter);
    let full = s.draw();

    assert!(full.contains("│line-1 ") && !full.contains("1 Findings"), "{full}");
}

/// Rows of the Runs panel whose label is `label`.
fn count_rows(out: &str, label: &str) -> usize {
    run_rows(out).iter().filter(|r| r.contains(&format!("hh:mm:ss {label}"))).count()
}

/// Draws until `n` runs labelled `label` have finished, for up to eight seconds.
fn until_done(s: &mut AppScreen, label: &str, n: usize) -> String {
    let t0 = std::time::Instant::now();
    loop {
        let out = s.draw();
        let done =
            run_rows(&out).iter().filter(|r| r.starts_with("✔ 0") && r.contains(&format!("hh:mm:ss {label}"))).count();
        if done >= n || t0.elapsed() > std::time::Duration::from_secs(8) {
            return out;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn r_runs_a_chain_without_parameters_again_even_from_an_earlier_session() {
    let history = tempfile::tempdir().unwrap();
    let mut s = screen(history.path());
    s.typ("T");
    s.until("✔ 0");
    s.restart();
    s.typ("3");

    s.typ("r");
    let out = until_done(&mut s, "later", 2);

    assert_eq!(count_rows(&out, "later"), 2, "{out}");
}

#[test]
fn r_opens_a_chains_form_filled_with_the_values_it_ran_with() {
    let history = tempfile::tempdir().unwrap();
    let mut s = screen(history.path());
    s.typ("a");
    s.typ("bob");
    s.key(KeyCode::Enter);
    until_done(&mut s, "ask WHO=bob", 1);
    // A later run remembers dave; the older run still re-runs with bob.
    s.typ("a");
    s.ctrl('u');
    s.typ("dave");
    s.key(KeyCode::Enter);
    until_done(&mut s, "ask WHO=dave", 1);
    s.typ("j");

    s.typ("r");
    let form = s.draw();
    let field = form.lines().find(|l| l.contains("WHO") && l.contains("text")).expect(&form);
    assert!(field.contains("bob"), "{form}");
    s.key(KeyCode::Enter);
    let out = until_done(&mut s, "ask WHO=bob", 2);

    assert_eq!(count_rows(&out, "ask WHO=bob"), 2, "{out}");
}

#[test]
fn r_opens_a_plugins_argument_line_with_the_text_it_ran_with() {
    let history = tempfile::tempdir().unwrap();
    let plugins = tempfile::tempdir().unwrap();
    common::plugin(plugins.path(), "greet", r#"echo '{"usage":"rpt greet <name>"}'"#, r#"echo "hello $1""#);
    let mut s = AppScreen::new(&format!("history: {{ dir: '{}' }}\n", history.path().display()), plugins);
    s.choose("greet");
    s.ctrl('u');
    s.typ("world");
    s.key(KeyCode::Enter);
    s.until("hello world");
    s.until("✔ 0");

    s.typ("r");
    let out = s.draw();

    let line = out.lines().find(|l| l.contains("rpt greet ") && !l.contains("╭")).expect(&out);
    assert!(out.contains("ctrl-u clear") && line.contains("rpt greet world"), "the argument line: {out}");
}

#[test]
fn r_says_so_when_the_chain_is_no_longer_in_the_config() {
    use rpt_tui::history::{Ended, Invocation, Limits, Record, Store};
    let history = tempfile::tempdir().unwrap();
    let mut s = screen(history.path());
    let store = Store::open(history.path(), s.report.path(), Limits::default()).unwrap();
    let record = Record {
        label: "gone".into(),
        invocation: Invocation { command: "chain gone".into(), ..Invocation::default() },
        started_ms: 1000 * std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
        duration_ms: 1000,
        ended: Ended::Exited { code: 0, failed_step: None },
        takeover: false,
    };
    store.save(&record, "").unwrap();
    s.restart();
    s.typ("3");

    s.typ("r");

    assert!(s.draw().contains("chain gone is no longer in the config"));
}

#[test]
fn r_on_a_live_run_says_it_is_still_running() {
    let history = tempfile::tempdir().unwrap();
    let mut s = screen(history.path());
    s.typ("W");
    s.until("started-waiting");
    s.ctrl('g');

    s.typ("r");

    assert!(s.draw().contains("still running"));
}

#[test]
fn x_deletes_a_finished_run_after_a_yes() {
    let history = tempfile::tempdir().unwrap();
    let mut s = screen(history.path());
    s.typ("T");
    let out = s.until("✔ 0");
    let footer = out.lines().last().unwrap();
    assert!(footer.contains("enter open · r rerun · x delete"), "{footer}");

    s.typ("x");
    assert!(s.draw().contains("delete run later? y/n"));
    s.typ("n");
    assert_eq!(count_rows(&s.draw(), "later"), 1, "n keeps it");
    s.typ("x");
    s.typ("y");

    assert!(s.draw().contains("No runs yet."));
    s.restart();
    assert!(s.draw().contains("No runs yet."), "gone from history too");
}

#[test]
fn clear_history_empties_it_after_a_yes_leaving_live_runs() {
    let history = tempfile::tempdir().unwrap();
    let mut s = screen(history.path());
    s.typ("T");
    s.until("✔ 0");
    s.typ("T");
    until_done(&mut s, "later", 2);
    s.restart();
    s.typ("3");
    s.typ("W");
    s.until("started-waiting");
    s.ctrl('g');

    s.choose("clear-history");
    assert!(s.draw().contains("clear 2 finished runs from this report's history? y/n"));
    s.typ("y");

    let out = s.draw();
    assert_eq!(count_rows(&out, "later"), 0, "{out}");
    assert_eq!(count_rows(&out, "waits"), 1, "the live run stays: {out}");
    s.restart();
    assert_eq!(count_rows(&s.draw(), "later"), 0);
}

#[test]
fn a_remembered_value_fills_the_form_in_a_later_session() {
    let history = tempfile::tempdir().unwrap();
    let mut s = screen(history.path());
    s.typ("a");
    s.typ("carol");
    s.key(KeyCode::Enter);
    s.until("hi carol");

    s.restart();
    s.typ("a");

    let form = s.draw();
    let field = form.lines().find(|l| l.contains("WHO") && l.contains("text")).expect(&form);
    assert!(field.contains("carol"), "{form}");
}

#[test]
fn recently_used_commands_lead_the_palette_in_a_later_session() {
    let history = tempfile::tempdir().unwrap();
    let mut s = screen(history.path());
    s.typ("S");
    s.until("● live");
    s.ctrl('g');
    s.typ("T");
    until_done(&mut s, "later", 1);

    s.restart();
    s.typ(":");
    let out = s.draw();

    let rows: Vec<&str> = out.lines().filter(|l| l.contains("chain  ")).collect();
    assert!(out.contains("recent"), "{out}");
    assert!(rows[0].contains("chain  later") && rows[1].contains("chain  slow"), "newest first: {out}");
}

#[test]
fn a_run_killed_on_quit_is_saved_as_killed_with_its_output_so_far() {
    let history = tempfile::tempdir().unwrap();
    let mut s = screen(history.path());
    s.typ("W");
    s.until("started-waiting");
    s.ctrl('g');
    s.typ("q");
    s.typ("y");
    assert!(s.app.should_quit());

    s.restart();
    s.typ("3");
    let out = s.draw();

    assert!(run_rows(&out).iter().any(|r| r.starts_with("killed") && r.contains("waits")), "{out}");
    assert!(out.contains("│started-waiting"), "the transcript so far: {out}");
}

#[test]
fn a_run_history_cannot_read_is_counted_in_a_toast_at_startup() {
    let history = tempfile::tempdir().unwrap();
    let mut s = screen(history.path());
    let runs = history.path().join("reports").join(rpt_tui::history::report_id(s.report.path())).join("runs");
    std::fs::create_dir_all(runs.join("1-1-0")).unwrap();
    std::fs::write(runs.join("1-1-0").join("meta.json"), "not json").unwrap();

    s.restart();

    assert!(s.draw().contains("1 run in history could not be read"));
}

/// Runs `listing` and opens `export-transcript` on it, the path line cleared and `path` typed.
fn export_listing(s: &mut AppScreen, path: &Path) -> String {
    s.typ("L");
    until_done(s, "listing", 1);
    s.choose("export-transcript");
    let line = s.draw();
    s.ctrl('u');
    s.typ(&path.display().to_string());
    s.key(KeyCode::Enter);
    line
}

#[test]
fn export_transcript_writes_the_selected_runs_text_where_you_say() {
    let (history, out) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut s = screen(history.path());
    let file = out.path().join("listing.txt");

    let line = export_listing(&mut s, &file);

    let prefill = line.lines().find(|l| l.contains("~/listing-")).expect(&line);
    let name = prefill.split("~/listing-").nth(1).unwrap().split_whitespace().next().unwrap();
    let shape: String = name.chars().map(|c| if c.is_ascii_digit() { 'd' } else { c }).collect();
    assert_eq!(shape, "dddddddd-dddddd.txt", "{prefill}");
    let text = std::fs::read_to_string(&file).unwrap();
    let want: String = (1..=300).map(|i| format!("line-{i}\n")).collect();
    assert_eq!(text, want);
}

#[test]
fn export_asks_before_writing_over_a_file() {
    let (history, out) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut s = screen(history.path());
    let file = out.path().join("there.txt");
    std::fs::write(&file, "keep me").unwrap();

    export_listing(&mut s, &file);
    assert!(s.draw().contains(&format!("overwrite {}? y/n", file.display())));
    s.typ("n");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "keep me");

    s.choose("export-transcript");
    s.ctrl('u');
    s.typ(&file.display().to_string());
    s.key(KeyCode::Enter);
    s.typ("y");
    assert!(std::fs::read_to_string(&file).unwrap().starts_with("line-1\n"));
}

#[test]
fn export_refuses_a_path_inside_the_report() {
    let history = tempfile::tempdir().unwrap();
    let mut s = screen(history.path());
    let file = s.report.path().join("findings").join("out.txt");

    export_listing(&mut s, &file);

    assert!(s.draw().contains("inside the report: rpt-tui doesn't write there"));
    assert!(!file.exists());
}

#[test]
fn export_without_a_run_says_so() {
    let history = tempfile::tempdir().unwrap();
    let mut s = screen(history.path());

    s.choose("export-transcript");

    assert!(s.draw().contains("no run selected"));
}

#[test]
fn pruning_after_a_save_keeps_the_taken_over_run_taken_over() {
    let history = tempfile::tempdir().unwrap();
    let config = format!(
        "history: {{ dir: '{}', max_runs: 1 }}\n{CHAINS}  full:\n    key: F\n    display: takeover\n    steps: ['echo in-takeover; sleep 30']\n",
        history.path().display()
    );
    let mut s = AppScreen::new(&config, tempfile::tempdir().unwrap());
    s.typ("T");
    s.until("✔ 0");
    s.restart();
    s.typ("3");
    s.typ("S");
    s.until("● live");
    s.ctrl('g');
    s.typ("F");
    s.until("in-takeover");

    // `slow` ends under the takeover: saving it prunes the earlier run listed above it.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    s.draw();
    let out = s.draw();

    assert!(out.contains("in-takeover") && !out.contains("1 Findings"), "still taken over: {out}");
    s.app.kill_all();
}

#[test]
fn clear_history_removes_only_the_runs_it_counted() {
    use rpt_tui::history::{Ended, Invocation, Limits, Record, Store};
    let history = tempfile::tempdir().unwrap();
    let mut s = screen(history.path());
    s.typ("T");
    s.until("✔ 0");
    // Another session saves a run this one never listed.
    let store = Store::open(history.path(), s.report.path(), Limits::default()).unwrap();
    let record = Record {
        label: "elsewhere".into(),
        invocation: Invocation { command: "chain later".into(), ..Invocation::default() },
        started_ms: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64,
        duration_ms: 1000,
        ended: Ended::Exited { code: 0, failed_step: None },
        takeover: false,
    };
    store.save(&record, "").unwrap();

    s.choose("clear-history");
    assert!(s.draw().contains("clear 1 finished run from"));
    s.typ("y");

    let left: Vec<String> = store.list().runs.into_iter().map(|r| r.record.label).collect();
    assert_eq!(left, ["elsewhere"]);
}
