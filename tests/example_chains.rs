//! The spec's §11 example chains, run against the fixture report with stand-in plugins.
mod common;

use common::AppScreen;
use ratatui::crossterm::event::KeyCode;

/// The §11 chains, word for word.
const CHAINS: &str = r#"
chains:
  import-nessus:
    description: Parse a Nessus scan, match scope, add findings
    key: ctrl-n
    params:
      SCAN:   { kind: file, glob: "*.nessus", remember: true }
      STRICT: { kind: choice, options: [yes, no], default: no }
    steps:
      - rpt parse-nessus "$SCAN" > "$RUN_TMP/records.jsonl"
      - rpt scope match < "$RUN_TMP/records.jsonl" > "$RUN_TMP/scoped.jsonl"
      - rpt add < "$RUN_TMP/scoped.jsonl"
  pdf-draft:
    description: Validate, assemble and render a draft PDF
    params: { CLIENT: { kind: text, default: Acme }, PERIOD: { kind: text } }
    steps:
      - { run: rpt validate, allow_failure: true }
      - rpt assemble | rpt render-pdf --client "$CLIENT" --period "$PERIOD"
  preview:
    description: Render the selected finding
    key: p
    needs: finding
    ephemeral: true
    steps: ['glow -p "$RPT_FINDING_FILE"']
"#;

fn screen() -> AppScreen {
    let plugins = tempfile::tempdir().unwrap();
    // parse-nessus: the scan file's one line is a host; out comes a finding on it, port 443.
    common::plugin(
        plugins.path(),
        "parse-nessus",
        r#"echo '{"summary":"Parse Nessus XML","input":"xml/nessus","output":"records"}'"#,
        r#"host=$(cat "$1") || exit 1
printf '{"kind":"finding","title":"Imported from Nessus","severity":"high","scope":[],"target":{"host":"%s","port":443,"protocol":"tcp"}}\n' "$host""#,
    );
    // render-pdf: records its arguments and how much JSON it was given.
    common::plugin(
        plugins.path(),
        "render-pdf",
        r#"echo '{"summary":"Render a PDF"}'"#,
        r#"printf 'args:%s bytes:%s\n' "$*" "$(wc -c)" > draft.txt"#,
    );
    common::stand_in_rpt(plugins.path(), "glow", r#"echo "glow $*""#);
    AppScreen::new(CHAINS, plugins)
}

#[test]
fn import_nessus_parses_matches_scope_and_adds_a_finding() {
    let mut s = screen();
    std::fs::create_dir(s.report.path().join("scans")).unwrap();
    std::fs::write(s.report.path().join("scans/a.nessus"), "10.0.0.5\n").unwrap();
    s.choose("import-nessus");

    s.typ("scans/a.nessus");
    s.key(KeyCode::Enter);
    let out = s.until("4 findings");

    assert!(out.contains("✔ 0") && out.contains("Imported from Nessus"), "{out}");
    let added = std::fs::read_dir(s.report.path().join("findings"))
        .unwrap()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().starts_with("F-004"))
        .expect("rpt add wrote the finding");
    let text = std::fs::read_to_string(added.path()).unwrap();
    assert!(text.contains("S-12"), "scope match put the finding in scope: {text}");
}

#[test]
fn pdf_draft_goes_on_past_validate_failing_and_renders_with_its_parameters() {
    let mut s = screen();
    s.choose("pdf-draft");

    s.key(KeyCode::Tab);
    s.typ("2026-Q4");
    s.key(KeyCode::Enter);
    let out = s.until("✔ 0");

    assert!(out.contains("step 1/2 failed (1), continuing"), "validate fails on the fixture, allowed: {out}");
    let draft = std::fs::read_to_string(s.report.path().join("draft.txt")).unwrap();
    assert!(draft.starts_with("args:--client Acme --period 2026-Q4 bytes:"), "{draft}");
    assert!(!draft.trim().ends_with("bytes:0"), "the assembled report was piped in: {draft}");
}

#[test]
fn preview_runs_ephemeral_and_leaves_nothing_behind() {
    let mut s = screen();
    s.choose("preview");

    s.until("glow -p");
    let out = s.until("No runs yet.");

    assert!(out.contains("No runs yet."), "{out}");
}
