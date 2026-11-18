//! Selection variables rpt-tui itself inherited (started from a run, say) never reach its runs.
//! Alone in its binary: it sets the process environment.
mod common;

use common::AppScreen;

#[test]
fn a_selection_variable_rpt_tui_inherited_does_not_reach_a_run() {
    // SAFETY: the only test in this binary, so no other thread reads the environment.
    unsafe { std::env::set_var("RPT_PROBLEM_FILE", "/stray/inherited.md") };
    let mut s = AppScreen::new(
        "chains:\n  show:\n    key: h\n    steps: ['echo \"problem=${RPT_PROBLEM_FILE-[unset]}\"; sleep 5']\n",
        tempfile::tempdir().unwrap(),
    );

    s.typ("h");
    let out = s.until("problem=");

    assert!(out.contains("problem=[unset]"), "{out}");
}
