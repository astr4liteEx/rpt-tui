//! Chains from the config in the app: listed in the palette, the parameter form, running.
mod common;

use common::AppScreen;
use ratatui::crossterm::event::KeyCode;

fn screen(config: &str) -> AppScreen {
    AppScreen::new(config, tempfile::tempdir().unwrap())
}

const PARAMS: &str = r#"
chains:
  scan-import:
    description: Import a scan
    params:
      SCAN:   { kind: text }
      MODE:   { kind: choice, options: [fast, full, deep], default: fast }
      STRICT: { kind: flag, default: no }
      NOTE:   { kind: text, optional: true }
    steps: ['printf "got[%s][%s][%s][%s]\n" "$SCAN" "$MODE" "$STRICT" "$NOTE"; sleep 5']
"#;

#[test]
fn chains_are_listed_in_the_palette_with_their_descriptions() {
    let mut s = screen("chains:\n  hello:\n    description: Say hi from a chain\n    steps: [echo hi]\n");

    s.typ(":");
    let out = s.until("Say hi from a chain");

    let edit = out.find("chain  edit").unwrap();
    let hello = out.find("chain  hello").expect("the chain is listed");
    assert!(edit < hello && hello < out.find("plugins").unwrap_or(usize::MAX).min(out.find(" rpt ").unwrap()), "{out}");
}

#[test]
fn a_chain_without_parameters_runs_when_chosen() {
    let mut s = screen("chains:\n  hello:\n    steps: ['echo hi-from-chain; echo \"$RPT_REPORT_DIR\"']\n");

    s.choose("hello");
    let out = s.until("hi-from-chain");

    assert!(out.contains("hi-from-chain\n") || out.contains("hi-from-chain"), "{out}");
    assert!(out.contains("<report>"), "{out}");
    assert!(out.contains("hello"), "the run is labelled with the chain's name: {out}");
}

#[test]
fn a_failing_chain_names_its_step_in_runs() {
    let mut s = screen("chains:\n  two:\n    steps: [true, exit 2]\n");

    s.choose("two");
    let out = s.until("✘ step 2: 2");

    assert!(out.contains("✘ step 2: 2"), "{out}");
}

#[test]
fn a_chain_with_parameters_opens_its_form() {
    let mut s = screen(PARAMS);

    s.choose("scan-import");
    let out = s.draw();

    insta::assert_snapshot!(out);
}

#[test]
fn enter_with_a_required_field_empty_marks_it_and_runs_nothing() {
    let mut s = screen(PARAMS);
    s.choose("scan-import");

    s.key(KeyCode::Enter);
    let out = s.draw();

    assert!(out.contains("SCAN is required"), "{out}");
    assert!(out.contains("No runs yet."), "{out}");

    s.typ("a");
    assert!(!s.draw().contains("SCAN is required"), "typing clears the message");
}

#[test]
fn a_failing_one_step_chain_is_marked_with_its_code_alone() {
    let mut s = screen(
        "chains:
  one:
    steps: ['exit 3']
",
    );

    s.choose("one");
    let out = s.until("✘ 3");

    assert!(out.contains("✘ 3") && !out.contains("step 1"), "{out}");
}

#[test]
fn the_form_fills_text_choice_and_flag_fields_and_runs_with_them_as_variables() {
    let mut s = screen(PARAMS);
    s.choose("scan-import");

    s.typ("scans/a b.nessus");
    s.key(KeyCode::Tab);
    s.key(KeyCode::Right);
    s.key(KeyCode::Right);
    s.key(KeyCode::Tab);
    s.key(KeyCode::Char(' '));
    s.key(KeyCode::Enter);
    let out = s.until("got[");

    assert!(out.contains("got[scans/a b.nessus][deep][yes][]"), "{out}");
    assert!(out.contains("scan-import SCAN=scans/a b.nessus MODE=deep"), "labelled with its values: {out}");
}

#[test]
fn shift_tab_and_up_move_back_and_left_cycles_a_choice_backwards() {
    let mut s = screen(PARAMS);
    s.choose("scan-import");

    s.typ("x");
    s.key(KeyCode::Down);
    s.key(KeyCode::Left);
    s.key(KeyCode::Up);
    s.typ("y");
    s.key(KeyCode::Enter);
    let out = s.until("got[");

    assert!(out.contains("got[xy][deep][no][]"), "{out}");
}

#[test]
fn esc_in_the_form_goes_back_to_the_palette_with_the_query_kept() {
    let mut s = screen(PARAMS);
    s.choose("scan-import");

    s.key(KeyCode::Esc);
    let out = s.draw();

    assert!(out.contains("> scan-import") && out.contains("Import a scan"), "{out}");
}

#[test]
fn a_chain_named_like_a_plugin_is_dropped_with_a_toast_once_plugins_are_listed() {
    let plugins = tempfile::tempdir().unwrap();
    common::plugin(plugins.path(), "greet", r#"echo '{"summary":"Say hello"}'"#, "true");
    let mut s = AppScreen::new("chains:\n  greet:\n    description: My greet\n    steps: [true]\n", plugins);

    let out = s.until("greet");

    assert!(out.contains("chain greet") && out.contains("plugin"), "a toast names the clash: {out}");
    s.typ(":");
    let out = s.until("Say hello");
    assert!(!out.contains("My greet"), "{out}");
}

#[test]
fn config_errors_show_in_the_startup_toast() {
    let mut s = screen("chains:\n  Bad:\n    steps: [true]\n");

    let out = s.draw();

    assert!(out.contains("chain Bad"), "{out}");
}

#[test]
fn an_ephemeral_chain_leaves_no_run_and_a_takeover_chain_opens_full_screen() {
    let mut s = screen(
        "chains:\n  peek:\n    ephemeral: true\n    steps: [echo peeked]\n  big:\n    display: takeover\n    steps: ['echo big-one; sleep 5']\n",
    );

    s.choose("peek");
    s.until("peeked");
    let out = s.until("No runs yet.");
    assert!(out.contains("No runs yet."), "{out}");

    s.ctrl('g');
    s.choose("big");
    let out = s.until("big-one");
    assert!(out.contains("ctrl-t pane") && !out.contains("1 Findings"), "{out}");
}

#[test]
fn run_tmp_is_gone_once_the_chain_ends() {
    let mut s = screen("chains:\n  stage:\n    steps: ['echo \"$RUN_TMP\" > where.txt; echo staged']\n");

    s.choose("stage");
    s.until("✔ 0");

    let tmp = std::fs::read_to_string(s.report.path().join("where.txt")).unwrap();
    assert!(!std::path::Path::new(tmp.trim()).exists(), "{tmp}");
}

#[test]
fn a_chain_named_like_an_rpt_verb_is_left_out_with_a_startup_toast() {
    let mut s = AppScreen::new("chains:\n  sections:\n    steps: ['echo mine']\n", tempfile::tempdir().unwrap());

    let out = s.draw();

    assert!(out.contains("chain sections: `sections` is an rpt built-in"), "{out}");
}
