//! Keys: the notation shared by `keys:` and a chain's `key:`, the named actions with their
//! contexts and defaults, and the live bindings built from both with clashes found.

use std::fmt;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// One key as the notation writes it: `g`, `G`, `enter`, `ctrl-n`, `ctrl-alt-x`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    pub code: KeyCode,
    pub ctrl: bool,
    pub alt: bool,
}

const NAMED: [(&str, KeyCode); 13] = [
    ("enter", KeyCode::Enter),
    ("esc", KeyCode::Esc),
    ("tab", KeyCode::Tab),
    ("space", KeyCode::Char(' ')),
    ("backspace", KeyCode::Backspace),
    ("up", KeyCode::Up),
    ("down", KeyCode::Down),
    ("left", KeyCode::Left),
    ("right", KeyCode::Right),
    ("home", KeyCode::Home),
    ("end", KeyCode::End),
    ("pgup", KeyCode::PageUp),
    ("pgdn", KeyCode::PageDown),
];

impl Key {
    /// A key with no modifiers.
    pub const fn plain(code: KeyCode) -> Key {
        Key { code, ctrl: false, alt: false }
    }

    /// A character key with ctrl.
    pub const fn ctrl(c: char) -> Key {
        Key { code: KeyCode::Char(c), ctrl: true, alt: false }
    }

    /// Parses the notation; the error names what is wrong.
    pub fn parse(s: &str) -> Result<Key, String> {
        let bad = |why: String| format!("`{s}`: {why}");
        let (mut ctrl, mut alt, mut rest) = (false, false, s);
        // A modifier is `ctrl-` or `alt-` with something after it; `-` alone is the key.
        loop {
            if let Some(r) = rest.strip_prefix("ctrl-").filter(|r| !r.is_empty()) {
                ctrl = true;
                rest = r;
            } else if let Some(r) = rest.strip_prefix("alt-").filter(|r| !r.is_empty()) {
                alt = true;
                rest = r;
            } else {
                break;
            }
        }
        let mut chars = rest.chars();
        let code = match (chars.next(), chars.next()) {
            (Some(c), None) => KeyCode::Char(c),
            (None, _) => return Err(bad("no key".into())),
            _ => match NAMED.iter().find(|(n, _)| *n == rest) {
                Some((_, code)) => *code,
                None => match rest.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
                    Some(n @ 1..=12) => KeyCode::F(n),
                    _ if rest.ends_with('-') => return Err(bad("no key after the modifier".into())),
                    _ if rest.contains('-') => {
                        let m = rest.split('-').next().unwrap_or(rest);
                        return Err(bad(format!("`{m}` is not a modifier (ctrl, alt)")));
                    }
                    _ => return Err(bad(format!("`{rest}` is not a key"))),
                },
            },
        };
        Ok(Key { code, ctrl, alt })
    }

    /// The key a terminal event is, in the notation's terms: shift folds into the character,
    /// and ctrl-4…7 (how crossterm reports bytes 0x1c…0x1f) are ctrl-\ ] ^ _.
    pub fn of(ev: &KeyEvent) -> Key {
        let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);
        let code = match ev.code {
            KeyCode::Char(c @ '4'..='7') if ctrl => KeyCode::Char(['\\', ']', '^', '_'][c as usize - '4' as usize]),
            code => code,
        };
        Key { code, ctrl, alt: ev.modifiers.contains(KeyModifiers::ALT) }
    }

    /// Whether the terminal event is this key.
    pub fn matches(&self, ev: &KeyEvent) -> bool {
        Key::of(ev) == *self
    }

    /// The one byte a terminal sends for this key, when it is one byte: a plain ASCII
    /// character, or ctrl with a letter, space or one of `@[\]^_`.
    pub fn byte(&self) -> Option<u8> {
        let KeyCode::Char(c) = self.code else { return None };
        if self.alt || !c.is_ascii() {
            return None;
        }
        match (self.ctrl, c) {
            (false, c) if !c.is_ascii_control() => Some(c as u8),
            (true, ' ' | '@') => Some(0),
            (true, 'a'..='z') => Some(c as u8 - b'a' + 1),
            (true, '[' | '\\' | ']' | '^' | '_') => Some(c as u8 - b'@'),
            _ => None,
        }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.ctrl {
            f.write_str("ctrl-")?;
        }
        if self.alt {
            f.write_str("alt-")?;
        }
        match self.code {
            KeyCode::Char(c) => match NAMED.iter().find(|(_, k)| *k == self.code) {
                Some((name, _)) => f.write_str(name),
                None => write!(f, "{c}"),
            },
            KeyCode::F(n) => write!(f, "f{n}"),
            code => f.write_str(NAMED.iter().find(|(_, k)| *k == code).map_or("?", |(n, _)| n)),
        }
    }
}

/// A panel keys can be scoped to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Findings,
    Problems,
    Runs,
}

const LISTS: &[Panel] = &[Panel::Findings, Panel::Problems, Panel::Runs];
const ITEMS: &[Panel] = &[Panel::Findings, Panel::Problems];
const RUNS: &[Panel] = &[Panel::Runs];

/// Where an action's keys work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Whenever keys go to the TUI.
    Global,
    /// Only while one of these panels is focused.
    Panels(&'static [Panel]),
    /// While a run has the keys.
    Run,
}

impl Scope {
    /// Whether one key could mean both: a global key meets every panel; run keys meet only
    /// run keys.
    fn overlaps(self, other: Scope) -> bool {
        match (self, other) {
            (Scope::Run, o) | (o, Scope::Run) => o == Scope::Run,
            (Scope::Global, _) | (_, Scope::Global) => true,
            (Scope::Panels(a), Scope::Panels(b)) => a.iter().any(|p| b.contains(p)),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Scope::Global => "global",
            Scope::Run => "run",
            Scope::Panels(p) if p == LISTS => "lists",
            Scope::Panels(p) if p == ITEMS => "findings, problems",
            Scope::Panels(p) if p == RUNS => "runs",
            Scope::Panels(_) => "panels",
        }
    }
}

macro_rules! actions {
    ($($variant:ident $name:literal $scope:expr, [$($key:literal),*], $works:literal, $summary:literal;)*) => {
        /// A named built-in thing the TUI does on a key.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Action { $($variant),* }

        impl Action {
            /// Every action, in the order help lists them.
            pub const ALL: &[Action] = &[$(Action::$variant),*];

            /// The name `keys:` and the palette use.
            pub fn name(self) -> &'static str {
                match self { $(Action::$variant => $name),* }
            }

            pub fn scope(self) -> Scope {
                match self { $(Action::$variant => $scope),* }
            }

            /// Default keys, in notation.
            pub fn defaults(self) -> &'static [&'static str] {
                match self { $(Action::$variant => &[$($key),*]),* }
            }

            /// Whether it does anything yet; one that doesn't still holds its default keys.
            pub fn works(self) -> bool {
                match self { $(Action::$variant => $works),* }
            }

            pub fn summary(self) -> &'static str {
                match self { $(Action::$variant => $summary),* }
            }
        }
    };
}

actions! {
    Palette "palette" Scope::Global, [":", "ctrl-p"], true, "Open the command palette";
    Quit "quit" Scope::Global, ["q", "ctrl-c"], true, "Quit rpt-tui";
    Help "help" Scope::Global, ["?"], true, "List the keys";
    FocusNext "focus_next" Scope::Global, ["tab"], true, "Focus the next panel";
    FocusFindings "focus_findings" Scope::Global, ["1"], true, "Focus Findings";
    FocusProblems "focus_problems" Scope::Global, ["2"], true, "Focus Problems";
    FocusRuns "focus_runs" Scope::Global, ["3"], true, "Focus Runs";
    Validate "validate" Scope::Global, ["v"], true, "Run rpt validate";
    Down "down" Scope::Panels(LISTS), ["j", "down"], true, "Move down";
    Up "up" Scope::Panels(LISTS), ["k", "up"], true, "Move up";
    Top "top" Scope::Panels(LISTS), ["g"], true, "Go to the top";
    Bottom "bottom" Scope::Panels(LISTS), ["G"], true, "Go to the bottom";
    Open "open" Scope::Panels(LISTS), ["enter"], true, "Full view, or the keys to a run";
    Back "back" Scope::Panels(LISTS), ["esc"], true, "Leave the full view, or Runs";
    Edit "edit" Scope::Panels(ITEMS), ["e"], true, "Run edit on the selected item";
    Rerun "rerun" Scope::Panels(RUNS), ["r"], true, "Run the selected run again";
    KillOrDelete "kill_or_delete" Scope::Panels(RUNS), ["x"], true, "Kill a live run, or delete a finished one";
    Takeover "takeover" Scope::Panels(RUNS), ["f"], true, "Take over the selected run";
    LeaveRun "leave_run" Scope::Run, ["ctrl-g"], true, "Give the keys back to the TUI";
    ToggleTakeover "toggle_takeover" Scope::Run, ["ctrl-t"], true, "Switch between pane and takeover";
    Theme "theme" Scope::Global, [], true, "Switch the theme";
    ReloadConfig "reload-config" Scope::Global, [], false, "Read the config again";
    ClearHistory "clear-history" Scope::Global, [], true, "Empty this report's run history";
    ExportTranscript "export-transcript" Scope::Global, [], true, "Save the selected run's transcript";
    FixKeys "fix-keys" Scope::Global, [], true, "Fix key clashes in the config";
}

impl Action {
    pub fn named(name: &str) -> Option<Action> {
        Action::ALL.iter().copied().find(|a| a.name() == name)
    }

    fn default_keys(self) -> Vec<Key> {
        self.defaults().iter().map(|k| Key::parse(k).expect("default keys parse")).collect()
    }
}

/// One side of a key trouble: an action or a chain's hotkey.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Side {
    Action(Action),
    Chain(String),
}

impl Side {
    /// `chain jump`, or the action's name.
    pub fn who(&self) -> String {
        match self {
            Side::Action(a) => a.name().to_string(),
            Side::Chain(name) => format!("chain {name}"),
        }
    }
}

/// With an action's scope: `down (lists)`.
impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Side::Action(a) => write!(f, "{} ({})", a.name(), a.scope().label()),
            Side::Chain(_) => f.write_str(&self.who()),
        }
    }
}

/// Something wrong with the configured keys, found when the bindings are built. (Not a
/// "problem", which is rpt's word for what `rpt validate` reports.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trouble {
    /// `a` wanted `key`, which `b` holds; `a` lost it.
    Clash {
        key: Key,
        a: Side,
        b: Side,
    },
    /// The key as written doesn't parse.
    BadKey {
        side: Side,
        written: String,
        why: String,
    },
    /// `toggle_takeover` on a key raw takeover can't see.
    NotOneByte {
        key: Key,
    },
    UnknownAction(String),
}

impl Trouble {
    /// The side the fix is for, when there is one.
    pub fn side(&self) -> Option<Side> {
        match self {
            Trouble::Clash { a, .. } | Trouble::BadKey { side: a, .. } => Some(a.clone()),
            Trouble::NotOneByte { .. } => Some(Side::Action(Action::ToggleTakeover)),
            Trouble::UnknownAction(_) => None,
        }
    }

    /// Whose key it is: `chain jump`, `keys: validate`.
    pub fn at(&self) -> String {
        match self.side() {
            Some(Side::Action(a)) => format!("keys: {}", a.name()),
            Some(side) => side.who(),
            None => "keys".into(),
        }
    }

    /// What is wrong: `key j is taken by down (lists)`.
    pub fn reason(&self) -> String {
        match self {
            Trouble::Clash { key, b, .. } => format!("key {key} is taken by {b}"),
            Trouble::BadKey { why, .. } => why.clone(),
            Trouble::NotOneByte { key } => format!("{key} is not one byte (use a character or a ctrl- key)"),
            Trouble::UnknownAction(name) => format!("unknown action `{name}`"),
        }
    }
}

impl fmt::Display for Trouble {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}: {}", self.at(), self.reason())?;
        match self.side() {
            Some(Side::Action(a)) => match a.defaults() {
                [] => write!(f, "; {} stays unbound", a.name()),
                keys => write!(f, "; {} keeps {}", a.name(), keys.join(", ")),
            },
            Some(Side::Chain(_)) => f.write_str("; loaded without its hotkey"),
            None => Ok(()),
        }
    }
}

/// The live bindings: each action's keys and each chain's hotkey, with what went wrong.
#[derive(Debug, Clone)]
pub struct Bindings {
    actions: Vec<(Action, Vec<Key>)>,
    /// Actions `keys:` rebound (and that kept the new keys).
    changed: Vec<Action>,
    chains: Vec<(String, Key)>,
    pub troubles: Vec<Trouble>,
}

impl Bindings {
    /// The defaults with `settings` applied, then the chains' hotkeys in file order. A rebound
    /// action that clashes goes back to its default; a chain that clashes loses its hotkey.
    pub fn new(settings: &[crate::config::KeySetting], chains: &[crate::config::Chain]) -> Bindings {
        let mut troubles = vec![];
        let mut actions: Vec<(Action, Vec<Key>)> = Action::ALL.iter().map(|a| (*a, a.default_keys())).collect();
        let mut changed = vec![];
        for s in settings {
            let Some(action) = Action::named(&s.action) else {
                troubles.push(Trouble::UnknownAction(s.action.clone()));
                continue;
            };
            let keys = match s.keys.iter().map(|k| Key::parse(k)).collect::<Result<Vec<Key>, String>>() {
                Ok(keys) => keys,
                Err(why) => {
                    let written = s.keys.join(", ");
                    troubles.push(Trouble::BadKey { side: Side::Action(action), written, why });
                    continue;
                }
            };
            if action == Action::ToggleTakeover
                && let Some(key) = keys.iter().find(|k| k.byte().is_none())
            {
                troubles.push(Trouble::NotOneByte { key: *key });
                continue;
            }
            actions.iter_mut().find(|(a, _)| *a == action).expect("every action is in the table").1 = keys;
            changed.push(action);
        }
        // Clashes between actions: each rebound one in the clash goes back to its default,
        // until none is left. Defaults never clash.
        while let Some((key, a, b)) = first_clash(&actions) {
            let losers: Vec<(Action, Action)> =
                [(a, b), (b, a)].into_iter().filter(|(x, _)| changed.contains(x)).collect();
            assert!(!losers.is_empty(), "default keys clash: {} and {}", a.name(), b.name());
            for (loser, holder) in losers {
                revert(&mut actions, &mut changed, loser);
                troubles.push(Trouble::Clash { key, a: Side::Action(loser), b: Side::Action(holder) });
            }
        }
        // Then the chains, in file order. A rebound action on a chain's hotkey goes back to its
        // default and the pass starts again, since its default may meet an earlier chain.
        let mut bound: Vec<(String, Key)>;
        let mut chain_troubles: Vec<Trouble>;
        'pass: loop {
            bound = vec![];
            chain_troubles = vec![];
            for c in chains {
                let Some(written) = &c.key else { continue };
                let side = Side::Chain(c.name.clone());
                let key = match Key::parse(written) {
                    Ok(k) => k,
                    Err(why) => {
                        chain_troubles.push(Trouble::BadKey { side, written: written.clone(), why });
                        continue;
                    }
                };
                // `e` always runs `edit`, so a user `edit` chain may say so.
                let holds = |a: Action, keys: &Vec<Key>| {
                    a.scope().overlaps(Scope::Global) && keys.contains(&key) && !(a == Action::Edit && c.name == "edit")
                };
                if let Some(a) =
                    actions.iter().find(|(a, keys)| changed.contains(a) && holds(*a, keys)).map(|(a, _)| *a)
                {
                    revert(&mut actions, &mut changed, a);
                    troubles.push(Trouble::Clash { key, a: Side::Action(a), b: side });
                    continue 'pass;
                }
                let holder = actions
                    .iter()
                    .find(|(a, keys)| holds(*a, keys))
                    .map(|(a, _)| Side::Action(*a))
                    .or_else(|| bound.iter().find(|(_, k)| *k == key).map(|(n, _)| Side::Chain(n.clone())));
                match holder {
                    Some(b) => chain_troubles.push(Trouble::Clash { key, a: side, b }),
                    None => bound.push((c.name.clone(), key)),
                }
            }
            break;
        }
        troubles.extend(chain_troubles);
        Bindings { actions, changed, chains: bound, troubles }
    }

    /// The action a key does with `panel` focused: a global one or one of that panel's.
    pub fn action(&self, ev: &KeyEvent, panel: Panel) -> Option<Action> {
        self.find(ev, |s| match s {
            Scope::Global => true,
            Scope::Panels(p) => p.contains(&panel),
            Scope::Run => false,
        })
    }

    /// The action a key does while a run has the keys; anything else goes to the run.
    pub fn run_action(&self, ev: &KeyEvent) -> Option<Action> {
        self.find(ev, |s| s == Scope::Run)
    }

    fn find(&self, ev: &KeyEvent, scope: impl Fn(Scope) -> bool) -> Option<Action> {
        let key = Key::of(ev);
        self.actions.iter().find(|(a, keys)| scope(a.scope()) && keys.contains(&key)).map(|(a, _)| *a)
    }

    /// The chain whose hotkey this is.
    pub fn chain(&self, ev: &KeyEvent) -> Option<&str> {
        let key = Key::of(ev);
        self.chains.iter().find(|(_, k)| *k == key).map(|(n, _)| n.as_str())
    }

    pub fn chain_key(&self, name: &str) -> Option<Key> {
        self.chains.iter().find(|(n, _)| n == name).map(|(_, k)| *k)
    }

    pub fn keys(&self, action: Action) -> &[Key] {
        self.actions.iter().find(|(a, _)| *a == action).map_or(&[], |(_, k)| k)
    }

    /// Whether `keys:` changed this action's keys from the default.
    pub fn changed(&self, action: Action) -> bool {
        self.changed.contains(&action)
    }

    /// The bytes that leave raw takeover.
    pub fn toggle_bytes(&self) -> Vec<u8> {
        self.keys(Action::ToggleTakeover).iter().filter_map(Key::byte).collect()
    }

    /// An action's keys in notation, joined: `j, down`.
    pub fn written(&self, action: Action) -> String {
        self.keys(action).iter().map(Key::to_string).collect::<Vec<_>>().join(", ")
    }
}

/// Puts a rebound action back on its default keys.
fn revert(actions: &mut [(Action, Vec<Key>)], changed: &mut Vec<Action>, action: Action) {
    changed.retain(|c| *c != action);
    actions.iter_mut().find(|(a, _)| *a == action).expect("every action is in the table").1 = action.default_keys();
}

/// The first key two actions share in overlapping scopes.
fn first_clash(actions: &[(Action, Vec<Key>)]) -> Option<(Key, Action, Action)> {
    for (i, (a, ak)) in actions.iter().enumerate() {
        for (b, bk) in &actions[i + 1..] {
            if a.scope().overlaps(b.scope())
                && let Some(k) = ak.iter().find(|k| bk.contains(k))
            {
                return Some((*k, *a, *b));
            }
        }
    }
    None
}
