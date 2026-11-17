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
fn outside_a_report_it_exits_2_with_a_message() {
    let plain = tempfile::tempdir().unwrap();

    let out = rpt_tui(plain.path(), common::rpt_dir().as_os_str(), &[]);

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.starts_with("rpt-tui: no report"), "stderr was {stderr:?}");
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
