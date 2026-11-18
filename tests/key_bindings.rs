//! The live bindings: defaults, `keys:` rebinding, chain hotkeys, and the clashes found.
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rpt_tui::config;
use rpt_tui::keys::{Action, Bindings, Key, Panel, Side, Trouble};

fn bindings(yaml: &str) -> Bindings {
    let c = config::load(yaml);
    assert!(c.errors.is_empty(), "{:?}", c.errors);
    Bindings::new(&c.keys, &c.chains)
}

fn ch(c: char) -> KeyEvent {
    let shift = if c.is_ascii_uppercase() { KeyModifiers::SHIFT } else { KeyModifiers::NONE };
    KeyEvent::new(KeyCode::Char(c), shift)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn messages(b: &Bindings) -> Vec<String> {
    b.troubles.iter().map(Trouble::to_string).collect()
}

const PANELS: [Panel; 3] = [Panel::Findings, Panel::Problems, Panel::Runs];

#[test]
fn the_defaults_bind_each_action_in_its_context() {
    let b = bindings("");
    for p in PANELS {
        assert_eq!(b.action(&ch(':'), p), Some(Action::Palette));
        assert_eq!(b.action(&ctrl('p'), p), Some(Action::Palette));
        assert_eq!(b.action(&ch('q'), p), Some(Action::Quit));
        assert_eq!(b.action(&ctrl('c'), p), Some(Action::Quit));
        assert_eq!(b.action(&ch('?'), p), Some(Action::Help));
        assert_eq!(b.action(&ch('v'), p), Some(Action::Validate));
        assert_eq!(b.action(&ch('3'), p), Some(Action::FocusRuns));
        assert_eq!(b.action(&ch('j'), p), Some(Action::Down));
        assert_eq!(b.action(&KeyEvent::from(KeyCode::Up), p), Some(Action::Up));
        assert_eq!(b.action(&ch('G'), p), Some(Action::Bottom));
        assert_eq!(b.action(&KeyEvent::from(KeyCode::Enter), p), Some(Action::Open));
        assert_eq!(b.action(&KeyEvent::from(KeyCode::Esc), p), Some(Action::Back));
        assert_eq!(b.action(&ctrl('g'), p), None, "run keys are not panel keys");
    }
    assert_eq!(b.action(&ch('e'), Panel::Findings), Some(Action::Edit));
    assert_eq!(b.action(&ch('e'), Panel::Runs), None);
    assert_eq!(b.action(&ch('x'), Panel::Runs), Some(Action::KillOrDelete));
    assert_eq!(b.action(&ch('x'), Panel::Findings), None);
    assert_eq!(b.run_action(&ctrl('g')), Some(Action::LeaveRun));
    assert_eq!(b.run_action(&ctrl('t')), Some(Action::ToggleTakeover));
    assert_eq!(b.run_action(&ch('q')), None, "everything else goes to the run");
    assert_eq!(b.toggle_bytes(), [0x14]);
    assert!(b.troubles.is_empty());
}

#[test]
fn keys_rebinds_an_action_to_a_key_a_list_or_nothing() {
    let b = bindings("keys:\n  validate: V\n  palette: [':', ctrl-k]\n  help: null\n");

    assert_eq!(b.action(&ch('V'), Panel::Findings), Some(Action::Validate));
    assert_eq!(b.action(&ch('v'), Panel::Findings), None);
    assert_eq!(b.action(&ctrl('k'), Panel::Runs), Some(Action::Palette));
    assert_eq!(b.action(&ctrl('p'), Panel::Runs), None);
    assert_eq!(b.action(&ch('?'), Panel::Runs), None);
    assert!(b.changed(Action::Validate) && b.changed(Action::Help) && !b.changed(Action::Quit));
    assert_eq!(b.keys(Action::Help), []);
}

#[test]
fn a_chain_hotkey_works_in_every_panel() {
    let b = bindings("chains:\n  preview:\n    key: p\n    steps: [true]\n");

    for p in PANELS {
        assert_eq!(b.action(&ch('p'), p), None);
    }
    assert_eq!(b.chain(&ch('p')), Some("preview"));
    assert_eq!(b.chain_key("preview").map(|k| k.to_string()).as_deref(), Some("p"));
}

#[test]
fn a_chain_hotkey_on_a_panel_key_is_dropped_and_the_chain_kept() {
    let b = bindings("chains:\n  jump:\n    key: j\n    steps: [true]\n  kill:\n    key: x\n    steps: [true]\n");

    assert_eq!(b.chain(&ch('j')), None);
    assert_eq!(b.action(&ch('j'), Panel::Findings), Some(Action::Down));
    assert_eq!(b.chain(&ch('x')), None, "x is a Runs key, and hotkeys are global");
    assert_eq!(
        b.troubles[0],
        Trouble::Clash { key: Key::parse("j").unwrap(), a: Side::Chain("jump".into()), b: Side::Action(Action::Down) }
    );
    assert_eq!(messages(&b)[0], "chain jump: key j is taken by down (lists); loaded without its hotkey");
}

#[test]
fn a_chain_may_use_a_run_key_since_runs_take_those_only_while_they_have_the_keys() {
    let b = bindings("chains:\n  go:\n    key: ctrl-g\n    steps: [true]\n");

    assert_eq!(b.chain(&ctrl('g')), Some("go"));
    assert!(b.troubles.is_empty());
}

#[test]
fn of_two_chains_on_one_key_the_first_keeps_it() {
    let b = bindings("chains:\n  one:\n    key: p\n    steps: [true]\n  two:\n    key: p\n    steps: [true]\n");

    assert_eq!(b.chain(&ch('p')), Some("one"));
    assert_eq!(messages(&b), ["chain two: key p is taken by chain one; loaded without its hotkey"]);
}

#[test]
fn rebinding_an_action_frees_its_old_key_for_a_chain() {
    let b = bindings("keys:\n  validate: V\nchains:\n  vet:\n    key: v\n    steps: [true]\n");

    assert_eq!(b.chain(&ch('v')), Some("vet"));
    assert!(b.troubles.is_empty());
}

#[test]
fn an_action_rebound_onto_a_taken_key_keeps_its_default() {
    let b = bindings("keys:\n  validate: q\n");

    assert_eq!(b.action(&ch('q'), Panel::Findings), Some(Action::Quit));
    assert_eq!(b.action(&ch('v'), Panel::Findings), Some(Action::Validate));
    assert_eq!(messages(&b), ["keys: validate: key q is taken by quit (global); validate keeps v"]);
}

#[test]
fn a_swap_is_judged_as_a_whole() {
    let b = bindings("keys:\n  quit: v\n  validate: q\n");

    assert_eq!(b.action(&ch('v'), Panel::Runs), Some(Action::Quit));
    assert_eq!(b.action(&ch('q'), Panel::Runs), Some(Action::Validate));
    assert!(b.troubles.is_empty(), "{:?}", messages(&b));
}

#[test]
fn panel_keys_clash_only_within_a_panel_they_share() {
    // rerun is a Runs key and edit a Findings/Problems key: no panel has both.
    let b = bindings("keys:\n  rerun: e\n  takeover: d\n  edit: d\n");

    assert_eq!(b.action(&ch('e'), Panel::Runs), Some(Action::Rerun));
    assert_eq!(b.action(&ch('d'), Panel::Runs), Some(Action::Takeover));
    assert_eq!(b.action(&ch('d'), Panel::Problems), Some(Action::Edit));
    assert!(b.troubles.is_empty(), "{:?}", messages(&b));

    let lists = bindings("keys:\n  rerun: j\n");
    assert_eq!(lists.action(&ch('j'), Panel::Runs), Some(Action::Down), "j moves in every list, Runs included");
    assert_eq!(lists.troubles.len(), 1);
}

#[test]
fn actions_that_do_not_work_yet_still_hold_their_keys() {
    let b = bindings("chains:\n  redo:\n    key: r\n    steps: [true]\n");

    assert_eq!(b.chain(&ch('r')), None);
    assert_eq!(messages(&b), ["chain redo: key r is taken by rerun (runs); loaded without its hotkey"]);
}

#[test]
fn toggle_takeover_must_be_one_byte_for_raw_takeover() {
    let b = bindings("keys:\n  toggle_takeover: f5\n  leave_run: ctrl-]\n");

    assert_eq!(b.run_action(&ctrl('t')), Some(Action::ToggleTakeover));
    assert_eq!(b.run_action(&KeyEvent::new(KeyCode::Char('5'), KeyModifiers::CONTROL)), Some(Action::LeaveRun));
    assert_eq!(b.toggle_bytes(), [0x14]);
    assert_eq!(
        messages(&b),
        ["keys: toggle_takeover: f5 is not one byte (use a character or a ctrl- key); toggle_takeover keeps ctrl-t"]
    );

    let moved = bindings("keys:\n  toggle_takeover: [ctrl-o, 'ctrl-]']\n");
    assert_eq!(moved.toggle_bytes(), [0x0f, 0x1d]);
}

#[test]
fn bad_notation_keeps_the_default_or_drops_the_hotkey() {
    let b = bindings("keys:\n  quit: ctrl-foo\nchains:\n  hi:\n    key: hyper-h\n    steps: [true]\n");

    assert_eq!(b.action(&ch('q'), Panel::Findings), Some(Action::Quit));
    assert_eq!(b.chain_key("hi"), None);
    assert_eq!(
        messages(&b),
        [
            "keys: quit: `ctrl-foo`: `foo` is not a key; quit keeps q, ctrl-c",
            "chain hi: `hyper-h`: `hyper` is not a modifier (ctrl, alt); loaded without its hotkey",
        ]
    );
}

#[test]
fn an_unknown_action_is_a_trouble() {
    let b = bindings("keys:\n  frobnicate: z\n");

    assert_eq!(messages(&b), ["keys: unknown action `frobnicate`"]);
}

#[test]
fn keys_must_be_a_map_of_keys_or_lists() {
    assert_eq!(config::load("keys: [q]\n").errors, ["config.yaml: keys: not a map of actions"]);
    assert_eq!(
        config::load("keys:\n  quit: { a: b }\n").errors,
        ["config.yaml: keys: quit: not a key or a list of keys"]
    );
}

#[test]
fn every_action_has_a_name_and_round_trips() {
    for a in Action::ALL {
        assert_eq!(Action::named(a.name()), Some(*a));
    }
    assert_eq!(Action::named("reload-config"), Some(Action::ReloadConfig));
    assert_eq!(Action::named("kill_or_delete"), Some(Action::KillOrDelete));
}

#[test]
fn two_rebound_actions_on_one_key_both_keep_their_defaults() {
    let b = bindings("keys:\n  validate: z\n  help: z\n");

    assert_eq!(b.action(&ch('z'), Panel::Findings), None);
    assert_eq!(b.action(&ch('v'), Panel::Findings), Some(Action::Validate));
    assert_eq!(b.action(&ch('?'), Panel::Findings), Some(Action::Help));
    assert_eq!(b.troubles.len(), 2, "{:?}", messages(&b));
}

#[test]
fn an_action_rebound_onto_a_chains_hotkey_keeps_its_default_and_the_chain_its_hotkey() {
    let b = bindings("keys:\n  validate: p\nchains:\n  preview:\n    key: p\n    steps: [true]\n");

    assert_eq!(b.chain(&ch('p')), Some("preview"));
    assert_eq!(b.action(&ch('v'), Panel::Findings), Some(Action::Validate));
    assert_eq!(messages(&b), ["keys: validate: key p is taken by chain preview; validate keeps v"]);
}

#[test]
fn a_user_edit_chain_may_take_the_edit_key() {
    let b = bindings("chains:\n  edit:\n    key: e\n    steps: [true]\n");

    assert!(b.troubles.is_empty(), "{:?}", messages(&b));
}

#[test]
fn a_key_that_is_not_one_value_drops_the_hotkey_not_the_chain() {
    let c = config::load("chains:\n  two:\n    key: [a, b]\n    steps: [true]\n");
    assert!(c.errors.is_empty(), "{:?}", c.errors);
    assert_eq!(c.chains.len(), 1);

    let b = Bindings::new(&c.keys, &c.chains);
    assert_eq!(b.chain_key("two"), None);
    assert_eq!(b.troubles.len(), 1);
}
