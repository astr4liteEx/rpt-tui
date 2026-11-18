//! `config.yaml`: chains with every key and rule, and the errors the startup toast lists.
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use rpt_tui::config::{self, Config, Display, Needs, ParamKind, TakeoverStyle};

/// The spec's §11 example, with chains only.
const EXAMPLE: &str = r#"
theme: charm
file_picker: "fzf --height 10"
keys:
  leave_run: ctrl-g
history: { dir: ~/.local/state/rpt-tui, max_runs: 200 }
notify:  { desktop: true }

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

fn chain<'a>(c: &'a Config, name: &str) -> &'a config::Chain {
    c.chains.iter().find(|x| x.name == name).unwrap_or_else(|| panic!("no chain {name}: {:?}", c.errors))
}

/// Loads one chain body under `chains: { <name>: <body> }`.
fn one(name: &str, body: &str) -> Config {
    config::load(&format!("chains:\n  {name}:\n{}", indent(body)))
}

fn indent(body: &str) -> String {
    body.lines().map(|l| format!("    {l}\n")).collect()
}

/// The single error, which must name the chain and contain `what`.
fn broken(c: &Config, name: &str, what: &str) {
    assert!(c.chains.iter().all(|x| x.name != name), "{name} should be left out: {:?}", c.chains);
    assert_eq!(c.errors.len(), 1, "{:?}", c.errors);
    assert!(c.errors[0].contains(name) && c.errors[0].contains(what), "{:?}", c.errors);
}

#[test]
fn the_spec_example_loads_every_chain_and_key() {
    let c = config::load(EXAMPLE);

    assert!(c.errors.is_empty(), "{:?}", c.errors);
    let names: Vec<&str> = c.chains.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, ["import-nessus", "pdf-draft", "preview"], "in file order");
    assert_eq!(c.file_picker.as_deref(), Some("fzf --height 10"));

    let nessus = chain(&c, "import-nessus");
    assert_eq!(nessus.description, "Parse a Nessus scan, match scope, add findings");
    assert_eq!(nessus.key.as_deref(), Some("ctrl-n"));
    assert_eq!(nessus.steps.len(), 3);
    assert_eq!(nessus.steps[2].run, r#"rpt add < "$RUN_TMP/scoped.jsonl""#);
    let scan = &nessus.params[0];
    assert_eq!(scan.name, "SCAN");
    assert_eq!(scan.kind, ParamKind::File { glob: Some("*.nessus".into()) });
    assert!(scan.remember && scan.required());
    let strict = &nessus.params[1];
    assert_eq!(strict.kind, ParamKind::Choice { options: vec!["yes".into(), "no".into()] });
    assert_eq!(strict.default.as_deref(), Some("no"));
    assert!(!strict.required());

    let pdf = chain(&c, "pdf-draft");
    assert!(pdf.steps[0].allow_failure && !pdf.steps[1].allow_failure);
    assert_eq!(pdf.params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["CLIENT", "PERIOD"]);
    assert_eq!((pdf.display, pdf.takeover, pdf.ephemeral), (Display::Pane, TakeoverStyle::Emulated, false));

    let preview = chain(&c, "preview");
    assert_eq!(preview.needs, Some(Needs::Finding));
    assert!(preview.ephemeral);
}

#[test]
fn no_config_text_means_no_chains_and_no_errors() {
    let c = config::load("");
    assert!(c.chains.is_empty() && c.errors.is_empty() && c.file_picker.is_none());
}

#[test]
fn yaml_that_does_not_parse_is_one_error_and_no_chains() {
    let c = config::load("chains: [unclosed\n");
    assert!(c.chains.is_empty());
    assert_eq!(c.errors.len(), 1);
    assert!(c.errors[0].contains("config.yaml"), "{:?}", c.errors);
}

#[test]
fn an_unknown_top_level_key_is_an_error_but_the_rest_loads() {
    let c = config::load("chainz: {}\nchains:\n  ok:\n    steps: [true]\n");
    assert_eq!(c.chains.len(), 1);
    assert_eq!(c.errors.len(), 1);
    assert!(c.errors[0].contains("chainz"), "{:?}", c.errors);
}

#[test]
fn an_unknown_chain_key_breaks_only_that_chain() {
    let c = config::load("chains:\n  bad:\n    stepz: [true]\n    steps: [true]\n  good:\n    steps: [true]\n");
    broken(&c, "bad", "stepz");
    assert_eq!(c.chains[0].name, "good");
}

#[test]
fn a_chain_needs_steps() {
    broken(&one("nosteps", "description: nothing to do"), "nosteps", "steps");
    broken(&one("empty", "steps: []"), "empty", "steps");
}

#[test]
fn a_step_is_a_string_or_a_run_with_allow_failure() {
    let c = one("mixed", "steps:\n  - echo a\n  - { run: echo b, allow_failure: true }\n  - { run: echo c }");
    let steps = &chain(&c, "mixed").steps;
    assert_eq!((steps[1].run.as_str(), steps[1].allow_failure), ("echo b", true));
    assert!(!steps[2].allow_failure);

    broken(&one("norun", "steps: [{ allow_failure: true }]"), "norun", "run");
    broken(&one("extra", "steps: [{ run: x, retry: 3 }]"), "extra", "retry");
}

#[test]
fn a_parameter_kind_must_be_known() {
    broken(&one("badkind", "params: { X: { kind: number } }\nsteps: [true]"), "badkind", "number");
    broken(&one("nokind", "params: { X: { default: 1 } }\nsteps: [true]"), "nokind", "kind");
}

#[test]
fn a_parameter_name_must_be_an_environment_variable_name() {
    broken(&one("badparam", "params: { 2BAD: { kind: text } }\nsteps: [true]"), "badparam", "2BAD");
    broken(&one("dash", "params: { A-B: { kind: text } }\nsteps: [true]"), "dash", "A-B");
}

#[test]
fn an_unknown_parameter_key_breaks_the_chain() {
    broken(&one("pkey", "params: { X: { kind: text, colour: red } }\nsteps: [true]"), "pkey", "colour");
}

#[test]
fn a_choice_needs_options_and_its_default_must_be_one_of_them() {
    broken(&one("noopts", "params: { X: { kind: choice } }\nsteps: [true]"), "noopts", "options");
    broken(
        &one("baddef", "params: { X: { kind: choice, options: [a, b], default: c } }\nsteps: [true]"),
        "baddef",
        "c",
    );
}

#[test]
fn a_flag_default_is_yes_or_no() {
    let c = one("flags", "params: { A: { kind: flag, default: true }, B: { kind: flag, default: no } }\nsteps: [true]");
    let params = &chain(&c, "flags").params;
    assert_eq!(params[0].default.as_deref(), Some("yes"));
    assert_eq!(params[1].default.as_deref(), Some("no"));
    broken(&one("badflag", "params: { A: { kind: flag, default: maybe } }\nsteps: [true]"), "badflag", "maybe");
}

#[test]
fn a_parameter_is_required_unless_it_has_a_default_or_is_optional() {
    let c = one(
        "req",
        "params: { A: { kind: text }, B: { kind: text, default: x }, C: { kind: text, optional: true } }\nsteps: [true]",
    );
    let req: Vec<bool> = chain(&c, "req").params.iter().map(|p| p.required()).collect();
    assert_eq!(req, [true, false, false]);
}

#[test]
fn a_chain_name_is_lowercase_letters_digits_and_hyphens() {
    broken(&one("Upper", "steps: [true]"), "Upper", "name");
    broken(&one("under_score", "steps: [true]"), "under_score", "name");
    assert!(one("ok-2", "steps: [true]").errors.is_empty());
}

#[test]
fn a_chain_name_must_not_be_an_rpt_built_in() {
    for verb in ["validate", "add", "set", "severities", "plugins"] {
        broken(&one(verb, "steps: [true]"), verb, "rpt");
    }
}

#[test]
fn display_takeover_and_needs_take_only_their_values() {
    let c = one("full", "display: takeover\ntakeover: raw\nneeds: any\nnotify: true\nsteps: [true]");
    let full = chain(&c, "full");
    assert_eq!(
        (full.display, full.takeover, full.needs, full.notify),
        (Display::Takeover, TakeoverStyle::Raw, Some(Needs::Any), Some(true))
    );

    broken(&one("baddisp", "display: window\nsteps: [true]"), "baddisp", "window");
    broken(&one("badtake", "takeover: native\nsteps: [true]"), "badtake", "native");
    broken(&one("badneeds", "needs: scope\nsteps: [true]"), "badneeds", "scope");
}

#[test]
fn the_config_lives_under_xdg_config_home_else_home() {
    let p = config::path(Some(OsStr::new("/x/cfg")), Some(OsStr::new("/home/u")));
    assert_eq!(p, Some(PathBuf::from("/x/cfg/rpt-tui/config.yaml")));
    let p = config::path(None, Some(OsStr::new("/home/u")));
    assert_eq!(p, Some(PathBuf::from("/home/u/.config/rpt-tui/config.yaml")));
    let p = config::path(Some(OsStr::new("")), Some(OsStr::new("/home/u")));
    assert_eq!(p, Some(PathBuf::from("/home/u/.config/rpt-tui/config.yaml")), "an empty XDG_CONFIG_HOME is unset");
    assert_eq!(config::path(None, None), None);
}

#[test]
fn a_missing_config_file_is_no_config() {
    let c = config::load_file(std::path::Path::new("/nonexistent/rpt-tui/config.yaml"));
    assert!(c.chains.is_empty() && c.errors.is_empty());
}

#[test]
fn scalars_are_read_as_written_true_is_a_step_and_yes_is_a_word() {
    let c = one("words", "params: { M: { kind: choice, options: [yes, no, on], default: on } }\nsteps: [true, false]");
    let w = chain(&c, "words");
    assert_eq!(w.steps.iter().map(|s| s.run.as_str()).collect::<Vec<_>>(), ["true", "false"]);
    assert_eq!(w.params[0].kind, ParamKind::Choice { options: vec!["yes".into(), "no".into(), "on".into()] });
}

#[test]
fn a_true_or_false_key_takes_only_true_or_false() {
    broken(&one("yesno", "ephemeral: yes\nsteps: [true]"), "yesno", "ephemeral");
}

#[test]
fn file_picker_must_be_a_command_line() {
    let c = config::load("file_picker: [fzf]\n");
    assert!(c.file_picker.is_none());
    assert_eq!(c.errors.len(), 1);
    assert!(c.errors[0].contains("file_picker"), "{:?}", c.errors);
}

#[test]
fn a_config_that_is_not_a_map_is_one_error() {
    let c = config::load("- just\n- a list\n");
    assert!(c.chains.is_empty());
    assert_eq!(c.errors.len(), 1, "{:?}", c.errors);
}

#[test]
fn settings_read_by_other_parts_of_the_tui_load_without_errors() {
    let c =
        config::load("theme: charm\nkeys: { leave_run: ctrl-g }\nhistory: { max_runs: 3 }\nnotify: { bell: true }\n");
    assert!(c.errors.is_empty(), "{:?}", c.errors);
}

#[test]
fn history_settings_load_and_fill_in_defaults() {
    let c = config::load("history: { dir: ~/h, max_runs: 3, max_days: 7, max_transcript_mib: 2 }\n");
    assert!(c.errors.is_empty(), "{:?}", c.errors);
    assert_eq!(c.history.dir, Some(PathBuf::from("~/h")));
    assert_eq!(c.history.limits.max_runs, 3);
    assert_eq!(c.history.limits.max_days, 7);
    assert_eq!(c.history.limits.max_transcript, 2 * 1024 * 1024);

    let d = config::load("");
    assert_eq!(d.history.dir, None);
    assert_eq!(d.history.limits, rpt_tui::history::Limits::default());
}

#[test]
fn the_history_dir_takes_a_leading_tilde_and_defaults_to_the_state_dir() {
    let (state, home) = (Some(OsStr::new("/x/state")), Some(OsStr::new("/home/u")));
    let dir = |written: Option<&str>, state, home| config::history_dir(written.map(Path::new), state, home);

    assert_eq!(dir(Some("~/h"), state, home), Some(PathBuf::from("/home/u/h")));
    assert_eq!(dir(Some("/abs/h"), state, home), Some(PathBuf::from("/abs/h")));
    assert_eq!(dir(None, state, home), Some(PathBuf::from("/x/state/rpt-tui")));
    assert_eq!(dir(None, Some(OsStr::new("")), home), Some(PathBuf::from("/home/u/.local/state/rpt-tui")));
    assert_eq!(dir(None, None, None), None);
}

#[test]
fn history_limits_are_positive_whole_numbers_and_keys_are_known() {
    for (bad, what) in [
        ("max_runs: 0", "max_runs"),
        ("max_days: -1", "max_days"),
        ("max_transcript_mib: 1.5", "max_transcript_mib"),
        ("max_runs: lots", "max_runs"),
        ("maxruns: 3", "maxruns"),
    ] {
        let c = config::load(&format!("history: {{ {bad} }}\n"));
        assert_eq!(c.errors.len(), 1, "{bad}: {:?}", c.errors);
        assert!(c.errors[0].starts_with("config.yaml: history:") && c.errors[0].contains(what), "{:?}", c.errors);
        assert_eq!(c.history.limits, rpt_tui::history::Limits::default(), "{bad}");
    }
    assert_eq!(config::load("history: [a]\n").errors, ["config.yaml: history: not a map of settings"]);
}

#[test]
fn a_named_theme_is_rewritten_in_place_keeping_comments() {
    let text = "# my config\ntheme: charm   # the default\nkeys: { quit: Q }\n";

    let out = config::set_theme(text, "light").unwrap();

    assert_eq!(out, "# my config\ntheme: light   # the default\nkeys: { quit: Q }\n");
}

#[test]
fn a_theme_map_has_only_its_base_changed() {
    let block = "theme:\n  # mine\n  base: charm  # was\n  colors:\n    accent: red\n";
    assert_eq!(
        config::set_theme(block, "night").unwrap(),
        "theme:\n  # mine\n  base: night  # was\n  colors:\n    accent: red\n"
    );
    let no_base = "theme:\n  colors:\n    accent: red\n";
    assert_eq!(config::set_theme(no_base, "light").unwrap(), "theme:\n  base: light\n  colors:\n    accent: red\n");
    let flow = "theme: { base: terminal, colors: { accent: red } }\n";
    assert_eq!(config::set_theme(flow, "light").unwrap(), "theme: { base: light, colors: { accent: red } }\n");
    let flow = "theme: { colors: { accent: red } }  # mine\n";
    assert_eq!(config::set_theme(flow, "light").unwrap(), "theme: { base: light, colors: { accent: red } }  # mine\n");
}

#[test]
fn a_config_without_a_theme_gets_one_at_the_end() {
    assert_eq!(config::set_theme("keys: { quit: Q }", "light").unwrap(), "keys: { quit: Q }\ntheme: light\n");
    assert_eq!(config::set_theme("", "light").unwrap(), "theme: light\n");
}

#[test]
fn a_theme_line_that_cannot_be_edited_safely_is_left_alone() {
    assert_eq!(config::set_theme("theme: >\n  charm\n", "light"), None);
}
