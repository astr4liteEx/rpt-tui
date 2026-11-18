//! The file picker: which files it offers, the built-in list, fzf, and a `file_picker:` command.
mod common;

use std::path::Path;

use common::AppScreen;
use ratatui::crossterm::event::KeyCode;
use rpt_tui::ui::picker::matching_files;

fn touch(root: &Path, rel: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, "").unwrap();
}

fn tree() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    for f in ["a.nessus", "scans/b.nessus", "scans/deep/c.nessus", "scans/notes.md", ".git/x.nessus", "rpt.yaml"] {
        touch(t.path(), f);
    }
    t
}

#[test]
fn a_glob_without_a_slash_matches_file_names_at_any_depth() {
    let t = tree();
    assert_eq!(matching_files(t.path(), Some("*.nessus")), ["a.nessus", "scans/b.nessus", "scans/deep/c.nessus"]);
}

#[test]
fn a_glob_with_a_slash_matches_the_path_from_the_report() {
    let t = tree();
    assert_eq!(matching_files(t.path(), Some("scans/*.nessus")), ["scans/b.nessus"]);
    assert_eq!(matching_files(t.path(), Some("scans/?.nessus")), ["scans/b.nessus"]);
}

#[test]
fn a_symlinked_file_is_offered() {
    let t = tree();
    let shared = tempfile::tempdir().unwrap();
    touch(shared.path(), "d.nessus");
    std::os::unix::fs::symlink(shared.path().join("d.nessus"), t.path().join("scans/d.nessus")).unwrap();

    assert!(matching_files(t.path(), Some("*.nessus")).contains(&"scans/d.nessus".to_string()));
}

#[test]
fn no_glob_offers_every_file_but_never_git() {
    let t = tree();
    let all = matching_files(t.path(), None);
    assert_eq!(all, ["a.nessus", "rpt.yaml", "scans/b.nessus", "scans/deep/c.nessus", "scans/notes.md"]);
}

const FORM: &str = r#"
chains:
  take:
    params: { SCAN: { kind: file, glob: "*.md" } }
    steps: ['echo "took[$SCAN]"; sleep 5']
"#;

fn with_picker(command: &str) -> AppScreen {
    AppScreen::new(&format!("file_picker: {command:?}\n{FORM}"), tempfile::tempdir().unwrap())
}

#[test]
fn without_fzf_ctrl_f_opens_the_built_in_list_of_matching_files() {
    let mut s = AppScreen::new(FORM, tempfile::tempdir().unwrap());
    s.choose("take");

    s.ctrl('f');
    let out = s.draw();

    insta::assert_snapshot!(out);
}

#[test]
fn the_built_in_list_filters_as_you_type_and_enter_fills_the_field() {
    let mut s = AppScreen::new(FORM, tempfile::tempdir().unwrap());
    s.choose("take");
    s.ctrl('f');

    s.typ("xss");
    s.key(KeyCode::Enter);
    s.key(KeyCode::Enter);
    let out = s.until("took[");

    assert!(out.contains("took[findings/xss-search.md]"), "{out}");
}

#[test]
fn the_built_in_list_takes_a_typed_path_that_matches_nothing() {
    let mut s = AppScreen::new(FORM, tempfile::tempdir().unwrap());
    s.choose("take");
    s.ctrl('f');

    s.typ("/elsewhere/scan.md");
    s.key(KeyCode::Enter);
    let out = s.draw();

    assert!(out.contains("SCAN*  file   /elsewhere/scan.md"), "{out}");
}

#[test]
fn esc_in_the_built_in_list_leaves_the_field_as_it_was() {
    let mut s = AppScreen::new(FORM, tempfile::tempdir().unwrap());
    s.choose("take");
    s.typ("kept");
    s.ctrl('f');

    s.key(KeyCode::Esc);
    let out = s.draw();

    assert!(out.contains("SCAN*  file   kept"), "{out}");
}

#[test]
fn a_file_picker_command_gets_the_matching_files_and_its_first_line_is_the_value() {
    let mut s = with_picker("sort -r | head -n 2");
    s.choose("take");

    s.ctrl('f');
    let out = s.until("SCAN*  file   findings/xss");

    assert!(out.contains("SCAN*  file   findings/xss-search.md"), "{out}");
    assert!(out.contains("No runs yet."), "the picker's run leaves no trace: {out}");
}

#[test]
fn a_file_picker_that_fails_leaves_the_field_as_it_was() {
    let mut s = with_picker("echo nope; exit 130");
    s.choose("take");
    s.typ("kept");

    s.ctrl('f');
    s.until("No runs yet.");
    let out = s.until("SCAN*  file   kept");

    assert!(out.contains("SCAN*  file   kept") && !out.contains("nope"), "{out}");
}

#[test]
fn ctrl_f_in_an_argument_line_inserts_the_path_shell_quoted() {
    let plugins = tempfile::tempdir().unwrap();
    common::plugin(plugins.path(), "greet", r#"echo '{"summary":"Say hello","usage":"rpt greet -v"}'"#, "true");
    let mut s = AppScreen::new("file_picker: \"echo \\\"it's a/b c.md\\\"\"\n", plugins);
    s.choose("greet");

    s.ctrl('f');
    let out = s.until("rpt greet -v '");

    assert!(out.contains(r#"rpt greet -v 'it'\''s a/b c.md'"#), "{out}");
}

#[test]
fn fzf_picks_a_file_in_a_pane_when_it_is_on_path() {
    if rpt_tui::rpt::which("/usr/bin:/bin".as_ref(), "fzf").is_none() {
        eprintln!("skipped: fzf not on PATH");
        return;
    }
    let mut s = AppScreen::new(FORM, tempfile::tempdir().unwrap());
    s.choose("take");

    s.ctrl('f');
    s.until("pick a file");
    s.typ("xss");
    std::thread::sleep(std::time::Duration::from_millis(300));
    s.key(KeyCode::Enter);
    let out = s.until("SCAN*  file   findings/xss");

    assert!(out.contains("SCAN*  file   findings/xss-search.md"), "{out}");
}
