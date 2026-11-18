//! The palette's order: groups for an empty query, ranked matches for a typed one.
use rpt_tui::ui::palette::{Entry, Kind, Row, order};

fn entry(kind: Kind, name: &str, summary: &str) -> Entry {
    Entry::new(kind, name, summary)
}

fn entries() -> Vec<Entry> {
    vec![
        entry(Kind::Plugin, "parse-nuclei", "Parse nuclei JSON Lines"),
        entry(Kind::Plugin, "render-pdf", "Render the report as a PDF"),
        entry(Kind::Plugin, "checklist", "Tick off the test plan"),
        entry(Kind::Rpt, "validate", "Check the report"),
        entry(Kind::Rpt, "set", "Set a field of a finding"),
        entry(Kind::Tui, "quit", "Quit rpt-tui"),
    ]
}

/// The rows as text: `# group` for headers, `kind name` for entries.
fn show(entries: &[Entry], rows: &[Row]) -> Vec<String> {
    rows.iter()
        .map(|r| match r {
            Row::Header(h) => format!("# {h}"),
            Row::Loading => "loading plugins…".to_string(),
            Row::Entry(i) => format!("{} {}", entries[*i].kind.tag(), entries[*i].name),
        })
        .collect()
}

#[test]
fn an_empty_query_lists_recent_commands_then_each_group() {
    let e = entries();
    let recent = vec!["rpt set".to_string(), "plugin checklist".to_string()];

    let rows = order(&e, "", &recent, false);

    assert_eq!(
        show(&e, &rows),
        [
            "# recent",
            "rpt set",
            "plugin checklist",
            "# plugins",
            "plugin parse-nuclei",
            "plugin render-pdf",
            "# rpt",
            "rpt validate",
            "# TUI actions",
            "tui quit",
        ]
    );
}

#[test]
fn while_plugins_load_their_group_shows_a_loading_row() {
    let e: Vec<Entry> = entries().into_iter().filter(|e| e.kind != Kind::Plugin).collect();

    let rows = order(&e, "", &[], true);

    assert_eq!(show(&e, &rows)[..3], ["# plugins", "loading plugins…", "# rpt"]);
}

#[test]
fn a_typed_query_ranks_name_then_letters_in_order_then_summary_then_kind() {
    let e = vec![
        entry(Kind::Tui, "pdq", "has nothing"),
        entry(Kind::Plugin, "summary-hit", "about PDF output"),
        entry(Kind::Plugin, "p-d-f", "letters in order"),
        entry(Kind::Rpt, "render-pdf", "name contains it"),
        entry(Kind::Plugin, "unrelated", "nothing here"),
    ];

    let rows = order(&e, "pdf", &[], false);

    assert_eq!(show(&e, &rows), ["rpt render-pdf", "plugin p-d-f", "plugin summary-hit"]);
}

#[test]
fn a_query_naming_a_kind_lists_that_kind() {
    let e = entries();

    let rows = order(&e, "plug", &[], false);

    assert_eq!(show(&e, &rows), ["plugin parse-nuclei", "plugin render-pdf", "plugin checklist"]);
}

#[test]
fn matching_is_case_insensitive_and_ties_go_to_the_most_recent() {
    let e = entries();
    let recent = vec!["plugin render-pdf".to_string()];

    let rows = order(&e, "E", &recent, false);

    assert_eq!(show(&e, &rows)[..3], ["plugin render-pdf", "plugin parse-nuclei", "plugin checklist"]);
}
