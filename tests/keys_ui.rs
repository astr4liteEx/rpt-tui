//! Keys in the app: chain hotkeys, `needs:`, the selection variables, rebinding, a user `edit`.
mod common;

use common::AppScreen;
use ratatui::crossterm::event::KeyCode;

fn screen(config: &str) -> AppScreen {
    AppScreen::new(config, tempfile::tempdir().unwrap())
}

/// A chain printing the selection variables, `[unset]` for each one not set.
const SHOW: &str = r#"
chains:
  show:
    key: h
    steps: ['for v in RPT_FINDING_ID RPT_FINDING_FILE RPT_PROBLEM_FILE RPT_PROBLEM_LINE; do printf "%s=%s\n" $v "${!v-[unset]}"; done; sleep 5']
  preview:
    description: Render the selected finding
    key: p
    needs: finding
    steps: ['echo previewing "$RPT_FINDING_ID"; sleep 5']
"#;

#[test]
fn a_chain_hotkey_runs_the_chain() {
    let mut s = screen(SHOW);

    s.typ("p");
    let out = s.until("previewing F-003");

    assert!(out.contains("previewing F-003"), "{out}");
}

#[test]
fn a_run_gets_the_selected_finding() {
    let mut s = screen(SHOW);
    s.typ("j");

    s.typ("h");
    let out = s.until("RPT_PROBLEM_LINE=");

    assert!(out.contains("RPT_FINDING_ID=F-001"), "{out}");
    assert!(out.contains("RPT_FINDING_FILE=<report>/findings/xss-search.md"), "absolute: {out}");
    assert!(out.contains("RPT_PROBLEM_FILE=[unset]") && out.contains("RPT_PROBLEM_LINE=[unset]"), "{out}");
}

#[test]
fn a_run_gets_the_selected_problem_once_problems_was_focused_last() {
    let mut s = screen(SHOW);
    s.typ("2");
    s.typ("3");

    s.typ("h");
    let out = s.until("RPT_PROBLEM_LINE=");

    assert!(out.contains("RPT_PROBLEM_FILE=<report>/findings/tls-weak.md"), "{out}");
    assert!(out.contains("RPT_PROBLEM_LINE=5"), "{out}");
    assert!(out.contains("RPT_FINDING_ID=[unset]"), "{out}");
}

#[test]
fn needs_greys_the_chain_out_and_its_hotkey_says_why() {
    let mut s = screen(SHOW);
    s.typ("2");

    s.typ("p");
    let out = s.draw();
    assert!(out.contains("preview needs a finding"), "{out}");
    assert!(out.contains("No runs yet."), "{out}");

    s.typ(":preview");
    let out = s.draw();
    assert!(out.contains("needs a finding"), "the row says why: {out}");
    s.key(KeyCode::Enter);
    assert!(s.draw().contains("No runs yet."));
}

#[test]
fn the_palette_shows_a_chains_hotkey() {
    let mut s = screen(SHOW);

    s.typ(":preview");
    let out = s.draw();

    let row = out.lines().find(|l| l.contains("chain  preview")).expect(&out);
    assert!(row.contains("Render the selected finding") && row.contains(" p │"), "the key on the right: {row}");
}

#[test]
fn a_rebound_action_answers_on_its_new_key_only() {
    let mut s = screen("keys:\n  validate: V\n  focus_runs: R\n");

    s.typ("v");
    assert!(s.draw().contains("No runs yet."));
    s.typ("V");
    let out = s.until("rpt validate");

    assert!(out.contains("rpt validate"), "{out}");
}

#[test]
fn a_user_edit_chain_replaces_the_built_in_on_e() {
    let mut s = screen(
        "chains:\n  edit:\n    description: Edit in a pane\n    steps: ['echo my-editor \"$RPT_FINDING_FILE\"; sleep 5']\n",
    );

    s.typ(":");
    let palette = s.until("Edit in a pane");
    let first = palette.lines().find(|l| l.contains("chain  ")).unwrap();
    assert!(first.contains("chain  edit") && first.contains(" e │"), "the built-in's first row, keyed e: {palette}");
    s.typ("edit");
    let palette = s.draw();
    assert_eq!(palette.matches("chain  edit").count(), 1, "{palette}");
    assert!(palette.contains("Edit in a pane"), "{palette}");
    s.key(KeyCode::Esc);
    s.typ("e");
    let out = s.until("my-editor");

    assert!(out.contains("my-editor <report>/findings/sqli-login.md"), "in a pane: {out}");
}

#[test]
fn a_hotkey_with_parameters_opens_the_form() {
    let mut s = screen(
        "chains:\n  ask:\n    key: a\n    params: { WHO: { kind: text } }\n    steps: ['echo hi $WHO; sleep 5']\n",
    );

    s.typ("a");
    s.typ("bob");
    s.key(KeyCode::Enter);
    let out = s.until("hi bob");

    assert!(out.contains("hi bob"), "{out}");
}

#[test]
fn open_shows_a_finding_full_width_and_back_returns_to_the_list() {
    let mut s = screen("");

    s.key(KeyCode::Enter);
    let full = s.draw();
    insta::assert_snapshot!("open_finding_full_width", full);
    assert!(!full.contains("1 Findings"), "the left column is hidden: {full}");

    s.term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 12)).unwrap();
    s.draw();
    s.typ("jjjjj");
    let scrolled = s.draw();
    assert!(!scrolled.contains("F-003  SQL injection in login"), "j scrolls the view: {scrolled}");
    assert!(scrolled.contains("Use parameterised queries."), "{scrolled}");

    s.key(KeyCode::Esc);
    let home = s.draw();
    assert!(home.contains("▌F-003"), "back on the same finding: {home}");
}

#[test]
fn open_shows_a_problem_full_width() {
    let mut s = screen("");
    s.typ("2");

    s.key(KeyCode::Enter);
    let out = s.draw();

    assert!(!out.contains("1 Findings") && out.contains("findings/tls-weak.md:5"), "{out}");
}

#[test]
fn e_in_the_full_view_edits_the_item() {
    let mut s = screen("chains:\n  edit:\n    steps: ['echo editing \"$RPT_FINDING_ID\"; sleep 5']\n");
    s.key(KeyCode::Enter);

    s.typ("e");
    let out = s.until("editing F-003");

    assert!(out.contains("editing F-003"), "{out}");
}

#[test]
fn back_in_runs_returns_to_the_list_focused_last() {
    let mut s = screen("");
    s.typ("23");

    s.key(KeyCode::Esc);
    let out = s.draw();

    assert!(out.contains("Problem"), "{out}");
    assert!(out.contains("e edit"), "focus is on a list again: {out}");
}

#[test]
fn help_lists_the_live_bindings_by_context_with_chain_hotkeys_and_changes_marked() {
    let mut s = screen(&format!("keys:\n  validate: V\n  help: ['?', f1]\n  takeover: null\n{SHOW}"));

    s.typ("?");
    let out = s.draw();

    insta::assert_snapshot!("help", out);
    s.typ("?");
    assert!(!s.draw().contains("chain hotkeys"));
}

#[test]
fn the_footer_names_the_live_keys() {
    let mut s = screen("keys:\n  palette: ctrl-k\n  quit: Q\n");

    let out = s.draw();
    let footer = out.lines().last().unwrap();

    assert!(footer.contains("ctrl-k commands") && footer.contains("Q quit"), "{footer}");
    assert!(footer.contains("? help"), "{footer}");
}

#[test]
fn quitting_with_runs_live_asks_first() {
    let mut s = screen("chains:\n  wait:\n    key: w\n    steps: ['sleep 30']\n");
    s.typ("w");
    s.until("● live");
    s.ctrl('g');

    s.typ("q");
    let out = s.draw();
    assert!(out.contains("kill 1 live run and quit? y/n"), "{out}");
    s.typ("n");
    assert!(!s.app.should_quit());
    assert!(!s.draw().contains("y/n"));

    s.typ("q");
    s.typ("y");
    assert!(s.app.should_quit());
}

#[test]
fn quitting_with_nothing_live_does_not_ask() {
    let mut s = screen("");

    s.typ("q");

    assert!(s.app.should_quit());
}

#[test]
fn a_run_is_listed_with_the_time_it_started() {
    let mut s = screen("chains:\n  listing:\n    key: L\n    steps: [echo listed]\n");

    s.typ("L");
    let out = s.until("✔ 0");

    // The screen masks a clock time (two digits, colon, …) as hh:mm:ss.
    assert!(out.contains("▌✔ 0         hh:mm:ss listing"), "the start time before the label: {out}");
}

#[test]
fn when_an_ephemeral_run_ends_focus_returns_to_where_it_was() {
    let mut s = screen("chains:\n  peek:\n    key: P\n    ephemeral: true\n    steps: ['sleep 0.3']\n");
    s.typ("2");

    s.typ("P");
    assert!(s.until("keys go to the run").contains("keys go to the run"));
    let out = s.until("e edit");

    assert!(out.contains("No runs yet.") && out.contains("e edit"), "back on Problems: {out}");
    assert!(out.contains("╭ Problem"), "{out}");
}
