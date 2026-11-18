mod common;

use std::process::{Command, Output};

fn rpt_tui(cwd: &std::path::Path, path_var: &std::ffi::OsStr, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rpt-tui"))
        .args(args)
        .current_dir(cwd)
        .env_remove("RPT_REPORT_DIR")
        .env("PATH", path_var)
        .output()
        .unwrap()
}

#[test]
fn describe_answers_the_plugin_protocol() {
    let out = rpt_tui(&std::env::temp_dir(), common::rpt_dir().as_os_str(), &["--describe"]);

    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1, "one line of JSON: {stdout:?}");
    let d: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(d["summary"], "Terminal UI for a report: run rpt, plugins and chains");
    assert_eq!(d["usage"], "rpt tui");
    assert_eq!(d["version"], "v0.1.0");
    assert!(d.get("input").is_none() && d.get("output").is_none());
}

#[test]
fn outside_a_report_it_exits_2_with_a_message_pointing_at_rpt_new() {
    let plain = tempfile::tempdir().unwrap();

    let out = rpt_tui(plain.path(), common::rpt_dir().as_os_str(), &[]);

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.starts_with("rpt-tui: no report: run inside"), "stderr was {stderr:?}");
    assert!(stderr.contains("or create one with `rpt new <dir>`"), "stderr was {stderr:?}");
}

#[test]
fn without_rpt_on_path_it_exits_2_with_a_message() {
    let report = common::fixture();
    let empty = tempfile::tempdir().unwrap();

    let out = rpt_tui(report.path(), empty.path().as_os_str(), &[]);

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.starts_with("rpt-tui: rpt not found on PATH"), "stderr was {stderr:?}");
}

#[test]
fn outside_a_report_it_lists_the_reports_under_the_folder_with_their_paths() {
    let dir = common::reports_under(&["report-01", "report-02"]);

    let out = rpt_tui(dir.path(), common::rpt_dir().as_os_str(), &[]);

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.starts_with("rpt-tui: no report here. Reports under this folder:"), "{stderr}");
    for n in ["01", "02"] {
        let path = dir.path().join(format!("report-{n}"));
        let line = stderr.lines().find(|l| l.contains(&format!("report-{n}"))).unwrap_or_else(|| panic!("{stderr}"));
        assert!(line.contains(&path.display().to_string()), "an absolute path: {line}");
        assert!(line.contains("3 findings"), "{line}");
    }
    assert!(stderr.contains("set RPT_REPORT_DIR"), "{stderr}");
}

#[test]
fn the_listing_stops_at_ten_reports_and_says_how_many_more() {
    let names: Vec<String> = (1..=12).map(|n| format!("report-{n:02}")).collect();
    let dir = common::reports_under(&names.iter().map(String::as_str).collect::<Vec<_>>());

    let out = rpt_tui(dir.path(), common::rpt_dir().as_os_str(), &[]);

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(stderr.lines().filter(|l| l.contains("report-")).count(), 10, "{stderr}");
    assert!(stderr.contains("… and 2 more (rpt ls lists them all)"), "{stderr}");
}

#[test]
fn when_rpt_ls_fails_the_message_is_the_plain_one() {
    let dir = common::reports_under(&["report-01"]);
    let bin = tempfile::tempdir().unwrap();
    common::stand_in_rpt(bin.path(), "rpt", "echo 'rpt: ls broke' >&2\nexit 2\n");

    let out = rpt_tui(dir.path(), bin.path().as_os_str(), &[]);

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.starts_with("rpt-tui: no report: run inside"), "{stderr}");
}
