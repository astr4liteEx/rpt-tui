//! Which editor `edit` runs, and how it is told the file and line.
mod common;

use std::ffi::OsStr;
use std::path::Path;

use rpt_tui::ui::edit::editor_argv;

const FILE: &str = "/r/findings/a.md";

fn argv(visual: Option<&str>, editor: Option<&str>, path: &OsStr, line: Option<u32>) -> Option<Vec<String>> {
    editor_argv(visual, editor, path, Path::new(FILE), line)
}

#[test]
fn visual_wins_over_editor() {
    let got = argv(Some("vim"), Some("nano"), OsStr::new(""), None).unwrap();
    assert_eq!(got, ["vim", FILE]);
}

#[test]
fn editor_is_used_without_visual_and_may_carry_arguments() {
    let got = argv(None, Some("code -w"), OsStr::new(""), Some(4)).unwrap();
    assert_eq!(got, ["code", "-w", FILE], "+LINE only goes to editors known to take it");
}

#[test]
fn known_editors_get_the_line_before_the_file() {
    for ed in ["vi", "vim", "nvim", "nano", "micro", "emacs", "kak", "hx", "/usr/local/bin/nvim"] {
        let got = argv(Some(ed), None, OsStr::new(""), Some(12)).unwrap();
        assert_eq!(got, [ed, "+12", FILE]);
    }
}

#[test]
fn without_either_variable_the_first_of_nvim_vim_vi_nano_on_path_is_used() {
    let dir = tempfile::tempdir().unwrap();
    common::stand_in_rpt(dir.path(), "nano", "");
    common::stand_in_rpt(dir.path(), "vi", "");
    let path = format!("/nonexistent:{}", dir.path().display());

    let got = argv(Some(""), None, OsStr::new(&path), Some(3)).unwrap();

    assert_eq!(got, [dir.path().join("vi").display().to_string(), "+3".into(), FILE.into()]);
}

#[test]
fn no_editor_at_all_is_none() {
    assert_eq!(argv(None, None, OsStr::new("/nonexistent"), None), None);
}
