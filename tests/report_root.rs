use std::fs;
use std::path::Path;

use rpt_tui::rpt::find_report;

fn make_report(dir: &Path) {
    fs::create_dir_all(dir.join("findings")).unwrap();
    fs::write(dir.join("rpt.yaml"), "format: 1\n").unwrap();
}

#[test]
fn finds_the_nearest_report_above_a_subfolder() {
    let tmp = tempfile::tempdir().unwrap();
    let report = tmp.path().join("acme");
    make_report(&report);

    let found = find_report(&report.join("findings"), None);

    assert_eq!(found.as_deref(), Some(report.as_path()));
}

#[test]
fn rpt_report_dir_wins_over_the_current_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let here = tmp.path().join("here");
    let there = tmp.path().join("there");
    make_report(&here);
    make_report(&there);

    let found = find_report(&here, Some(there.as_os_str()));

    assert_eq!(found.as_deref(), Some(there.as_path()));
}

#[test]
fn no_report_anywhere_above_is_none() {
    let tmp = tempfile::tempdir().unwrap();
    let plain = tmp.path().join("plain/folder");
    fs::create_dir_all(&plain).unwrap();

    assert_eq!(find_report(&plain, None), None);
}

#[test]
fn rpt_report_dir_without_rpt_yaml_is_none() {
    let tmp = tempfile::tempdir().unwrap();
    let here = tmp.path().join("here");
    make_report(&here);

    assert_eq!(find_report(&here, Some(tmp.path().as_os_str())), None);
}
