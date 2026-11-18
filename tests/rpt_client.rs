mod common;

use rpt_tui::rpt::Client;

#[test]
fn version_reports_record_schema_and_format() {
    let report = common::fixture();
    let client = Client::new(common::rpt_bin(), report.path());

    let v = client.version().unwrap();

    assert!(v.version.starts_with("v0.1"), "version was {}", v.version);
    assert_eq!(v.schema_version, 1);
    assert_eq!(v.format, 1);
}

#[test]
fn findings_come_back_in_rpt_order_with_unknown_members_kept() {
    let report = common::fixture();
    let client = Client::new(common::rpt_bin(), report.path());

    let findings = client.findings().unwrap();

    let ids: Vec<&str> = findings.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(ids, ["F-003", "F-002", "F-001"]);
    let sqli = &findings[0];
    assert_eq!(sqli.title, "SQL injection in login");
    assert_eq!(sqli.severity, "high");
    assert_eq!(sqli.path, "findings/sqli-login.md");
    assert_eq!(sqli.scope, ["S-12", "S-14"]);
    assert_eq!(sqli.sections["remediation"], "Use parameterised queries.");
    assert_eq!(sqli.extra["cvss"], "9.1");
}

#[test]
fn problems_are_a_result_even_though_validate_exits_1() {
    let report = common::fixture();
    let client = Client::new(common::rpt_bin(), report.path());

    let problems = client.problems().unwrap();

    assert_eq!(problems.len(), 1);
    let p = &problems[0];
    assert_eq!(p.path, "findings/tls-weak.md");
    assert_eq!(p.line, Some(5));
    assert_eq!(p.rule, "scopeIDsExist");
    assert_eq!(p.level, "error");
    assert_eq!(p.message, "S-99 is not in scope.csv");
}

#[test]
fn rpt_exiting_2_is_an_error_carrying_its_message() {
    // rpt itself trusts RPT_REPORT_DIR, so a usage failure is staged with a stand-in rpt.
    let report = common::fixture();
    let fake = common::stand_in_rpt(report.path(), "fake-rpt", "echo 'rpt: no report: boom' >&2\nexit 2\n");
    let client = Client::new(&fake, report.path());

    let err = client.findings().unwrap_err();

    match err {
        rpt_tui::rpt::Error::Failed { code, stderr, .. } => {
            assert_eq!(code, Some(2));
            assert!(stderr.contains("no report"), "stderr was {stderr:?}");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn a_missing_rpt_binary_is_a_spawn_error() {
    let report = common::fixture();
    let client = Client::new(report.path().join("no-such-rpt"), report.path());

    assert!(matches!(client.version(), Err(rpt_tui::rpt::Error::Spawn(_))));
}

#[test]
fn find_on_path_finds_rpt_and_skips_relative_entries() {
    let dir = common::rpt_dir();
    let path = std::env::join_paths(["", "relative/bin", dir.to_str().unwrap()]).unwrap();

    assert_eq!(rpt_tui::rpt::find_on_path(&path), Some(dir.join("rpt")));
}

#[test]
fn find_on_path_is_none_without_rpt() {
    let empty = tempfile::tempdir().unwrap();

    assert_eq!(rpt_tui::rpt::find_on_path(empty.path().as_os_str()), None);
}

#[test]
fn severities_come_back_most_severe_first() {
    let report = common::fixture();
    let client = Client::new(common::rpt_bin(), report.path());

    let levels = client.severities().unwrap();

    assert_eq!(levels, ["critical", "high", "medium", "low", "info"]);
}

#[test]
fn severities_follow_a_report_s_custom_levels() {
    let report = common::fixture();
    std::fs::write(report.path().join("rpt.yaml"), "format: 1\nseverities: [severe, moderate, minor]\n").unwrap();
    let client = Client::new(common::rpt_bin(), report.path());

    assert_eq!(client.severities().unwrap(), ["severe", "moderate", "minor"]);
}

#[test]
fn plugins_are_listed_with_their_describe_answers() {
    let report = common::fixture();
    let dir = tempfile::tempdir().unwrap();
    common::stand_in_rpt(
        dir.path(),
        "rpt-parse-nuclei",
        r#"echo '{"summary":"Parse nuclei JSON Lines","usage":"rpt parse-nuclei < results.jsonl","version":"0.3.1","input":"jsonl/nuclei","output":"records"}'"#,
    );
    common::stand_in_rpt(dir.path(), "rpt-broken", "exit 3");
    let client = Client::new(common::rpt_bin(), report.path()).with_path(dir.path().as_os_str());

    let mut plugins = client.plugins().unwrap();
    plugins.sort_by(|a, b| a.name.cmp(&b.name));

    let names: Vec<&str> = plugins.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["broken", "parse-nuclei"]);
    let broken = &plugins[0];
    assert_eq!(broken.summary, None, "a plugin whose --describe fails is still listed");
    let nuclei = &plugins[1];
    assert_eq!(nuclei.summary.as_deref(), Some("Parse nuclei JSON Lines"));
    assert_eq!(nuclei.usage.as_deref(), Some("rpt parse-nuclei < results.jsonl"));
    assert_eq!(nuclei.input.as_deref(), Some("jsonl/nuclei"));
    assert_eq!(nuclei.output.as_deref(), Some("records"));
}

#[test]
fn the_path_runs_get_puts_this_rpt_first() {
    let report = common::fixture();
    let client = Client::new(common::rpt_bin(), report.path()).with_path(std::ffi::OsStr::new("/usr/bin:/bin"));

    let path = client.path();

    assert_eq!(path.to_str().unwrap(), format!("{}:/usr/bin:/bin", common::rpt_dir().display()));
}

#[test]
fn sections_come_back_in_rpt_order_with_the_report_s_headings() {
    let report = common::fixture();
    std::fs::write(report.path().join("rpt.yaml"), common::RENAMED_SECTIONS_CONFIG).unwrap();
    let client = Client::new(common::rpt_bin(), report.path());

    let sections = client.sections().unwrap();

    let got: Vec<(&str, &str, bool, bool)> =
        sections.iter().map(|s| (s.key.as_str(), s.heading.as_str(), s.fixed, s.required)).collect();
    assert_eq!(
        got,
        [
            ("description", "Description", true, true),
            ("remediation", "Recommendation", true, false),
            ("poc", "Proof of concept", true, false),
            ("Risk", "Risk", false, true),
        ]
    );
}

#[test]
fn verbs_lists_rpt_s_built_ins_with_their_summary_and_usage() {
    let report = common::fixture();
    let client = Client::new(common::rpt_bin(), report.path());

    let verbs = client.verbs().unwrap();

    let names: Vec<&str> = verbs.iter().map(|v| v.name.as_str()).collect();
    for name in ["add", "new", "sections", "set", "validate", "verbs"] {
        assert!(names.contains(&name), "{name} missing from {names:?}");
    }
    let set = verbs.iter().find(|v| v.name == "set").unwrap();
    assert_eq!(set.summary, "Change one frontmatter value of a finding");
    assert_eq!(set.usage, "rpt set <id> <key> <value>");
}
