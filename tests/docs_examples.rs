//! The config examples in README.md and docs/config.md load without errors.
use rpt_tui::config;

/// The ```yaml blocks in `text` that are whole configs (they start with a top-level key).
fn configs(text: &str) -> Vec<String> {
    const TOP: [&str; 7] = ["theme:", "file_picker:", "keys:", "history:", "notify:", "chains:", "#"];
    text.split("```yaml\n")
        .skip(1)
        .filter_map(|b| b.split("```").next())
        .filter(|b| TOP.iter().any(|k| b.starts_with(k)))
        .map(String::from)
        .collect()
}

#[test]
fn every_whole_config_example_in_the_docs_loads_cleanly() {
    let root = env!("CARGO_MANIFEST_DIR");
    let mut checked = 0;
    for doc in ["README.md", "docs/config.md"] {
        let text = std::fs::read_to_string(format!("{root}/{doc}")).unwrap();
        for example in configs(&text) {
            let c = config::load(&example);
            assert!(c.errors.is_empty(), "{doc}:\n{example}\n{:?}", c.errors);
            let (theme, errors) = rpt_tui::ui::theme::resolve(c.theme.as_ref(), None, true);
            assert!(errors.is_empty(), "{doc}:\n{example}\n{errors:?}");
            drop(theme);
            let troubles = rpt_tui::keys::Bindings::new(&c.keys, &c.chains).troubles;
            assert!(troubles.is_empty(), "{doc}:\n{example}\n{troubles:?}");
            checked += 1;
        }
    }
    assert!(checked >= 7, "found only {checked} examples");
}
