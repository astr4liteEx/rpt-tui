//! The guide that fixes key troubles (clashes, bad notation), one at a time: rebind or unbind
//! either side. A fix applies at once and is saved to `config.yaml` when it can be done safely.

use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::App;
use crate::config::{self, KeySetting};
use crate::keys::{Action, Bindings, Key, Side, Trouble};

/// One way to fix a problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fix {
    /// A new key for the side: in place of `old` among an action's keys, or as its only key
    /// (or the chain's hotkey) without one.
    Rebind(Side, Option<Key>),
    /// Drops `old` from an action's keys; without one, drops the chain's hotkey or the
    /// action's `keys:` entry (back to its default).
    Unbind(Side, Option<Key>),
}

/// The guide's state.
#[derive(Debug, Default)]
pub struct Guide {
    /// Problems passed over with esc; the guide shows the rest.
    skipped: Vec<Trouble>,
    pub sel: usize,
    /// A rebind waiting for its key press.
    pub pressing: Option<Fix>,
    /// Why the last key or choice was refused.
    pub refused: Option<String>,
}

/// The choices for a problem, with their labels.
pub fn options(t: &Trouble) -> Vec<(String, Fix)> {
    let side_options = |side: &Side, key: Option<Key>, lost: bool| -> Vec<(String, Fix)> {
        match (side, key) {
            (Side::Chain(n), _) => vec![
                (
                    format!("give chain {n} {} key", if key.is_some() { "another" } else { "a" }),
                    Fix::Rebind(side.clone(), None),
                ),
                (format!("leave chain {n} without a hotkey"), Fix::Unbind(side.clone(), None)),
            ],
            (Side::Action(a), Some(k)) => {
                let a = a.name();
                let drop =
                    if lost { format!("drop {k} from {a} (keep its default)") } else { format!("unbind {k} from {a}") };
                vec![
                    (format!("give {a} another key instead of {k}"), Fix::Rebind(side.clone(), Some(k))),
                    (drop, Fix::Unbind(side.clone(), Some(k))),
                ]
            }
            (Side::Action(a), None) => vec![
                (format!("give {} a key", a.name()), Fix::Rebind(side.clone(), None)),
                (format!("keep {} on {}", a.name(), a.defaults().join(", ")), Fix::Unbind(side.clone(), None)),
            ],
        }
    };
    match t {
        Trouble::Clash { key, a, b } => {
            let mut v = side_options(a, Some(*key), true);
            v.extend(side_options(b, Some(*key), false));
            v
        }
        Trouble::BadKey { side, .. } => side_options(side, None, true),
        Trouble::NotOneByte { .. } => side_options(&Side::Action(Action::ToggleTakeover), None, true),
        Trouble::UnknownAction(_) => vec![],
    }
}

/// What a fix changes in the config file.
enum Edit {
    ChainKey(String, Option<String>),
    ActionKeys(String, Option<Vec<String>>),
}

impl Edit {
    /// How to make the change by hand: `under chain jump, write `key: h``.
    fn by_hand(&self) -> String {
        match self {
            Edit::ChainKey(n, Some(k)) => format!("under chain {n}, write `key: {k}`"),
            Edit::ChainKey(n, None) => format!("remove `key:` from chain {n}"),
            Edit::ActionKeys(a, Some(k)) => format!("under keys:, write `{a}: {}`", config::yaml_keys(k)),
            Edit::ActionKeys(a, None) => format!("remove `{a}` from keys:"),
        }
    }
}

impl App {
    /// Problems the guide can fix.
    fn fixable(&self) -> Vec<&Trouble> {
        self.bindings.troubles.iter().filter(|t| t.side().is_some()).collect()
    }

    pub(super) fn fixable_troubles(&self) -> usize {
        self.fixable().len()
    }

    /// Opens the guide when there is something to fix.
    pub(super) fn open_guide(&mut self) {
        if self.fixable_troubles() > 0 {
            self.guide = Some(Guide::default());
        }
    }

    /// The problem the guide shows, with how many are left (it included).
    pub(super) fn guide_trouble(&self) -> Option<(Trouble, usize)> {
        let g = self.guide.as_ref()?;
        let left: Vec<&Trouble> = self.fixable().into_iter().filter(|t| !g.skipped.contains(t)).collect();
        left.first().map(|t| ((*t).clone(), left.len()))
    }

    /// Keys while the guide is open: choose with arrows (or j/k) and enter, esc skips; while
    /// a rebind waits, the next key is the new one (esc cancels).
    pub(super) fn guide_key(&mut self, key: KeyEvent) {
        let Some(trouble) = self.guide_trouble().map(|(t, _)| t) else {
            self.guide = None;
            return;
        };
        let opts = options(&trouble);
        let Some(g) = self.guide.as_mut() else { return };
        if let Some(fix) = g.pressing.clone() {
            if key.code == KeyCode::Esc {
                g.pressing = None;
                g.refused = None;
            } else {
                self.try_fix(&trouble, &fix, Some(Key::of(&key)));
            }
        } else {
            match key.code {
                KeyCode::Esc => {
                    g.skipped.push(trouble);
                    g.sel = 0;
                    g.refused = None;
                }
                KeyCode::Up | KeyCode::Char('k') => g.sel = g.sel.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => g.sel = (g.sel + 1).min(opts.len().saturating_sub(1)),
                KeyCode::Enter => match opts.get(g.sel).map(|(_, f)| f.clone()) {
                    Some(fix @ Fix::Rebind(..)) => {
                        g.pressing = Some(fix);
                        g.refused = None;
                    }
                    Some(fix) => self.try_fix(&trouble, &fix, None),
                    None => {}
                },
                _ => {}
            }
        }
        if self.guide_trouble().is_none() {
            self.guide = None;
        }
    }

    /// Applies `fix` (with `new`, the pressed key, for a rebind) unless it leaves this problem
    /// or makes a new one; then the guide says why and waits.
    fn try_fix(&mut self, trouble: &Trouble, fix: &Fix, new: Option<Key>) {
        let (chains, settings, edit) = self.fixed(fix, new);
        let after = Bindings::new(&settings, &chains);
        let worse = after.troubles.iter().find(|t| *t == trouble || !self.bindings.troubles.contains(t));
        if let Some(t) = worse {
            if let Some(g) = self.guide.as_mut() {
                g.refused = Some(t.reason());
            }
            return;
        }
        self.chains = chains;
        self.key_settings = settings;
        self.rebind();
        if let Some(g) = self.guide.as_mut() {
            *g = Guide { skipped: std::mem::take(&mut g.skipped), ..Guide::default() };
        }
        self.save(&edit);
    }

    /// The chains and `keys:` with the fix made, and the edit to save.
    fn fixed(&self, fix: &Fix, new: Option<Key>) -> (Vec<config::Chain>, Vec<KeySetting>, Edit) {
        let mut chains = self.chains.clone();
        let mut settings = self.key_settings.clone();
        let new = new.map(|k| k.to_string());
        let edit = match fix {
            Fix::Rebind(Side::Chain(n), _) | Fix::Unbind(Side::Chain(n), _) => {
                let key = if matches!(fix, Fix::Rebind(..)) { new } else { None };
                if let Some(c) = chains.iter_mut().find(|c| &c.name == n) {
                    c.key = key.clone();
                }
                Edit::ChainKey(n.clone(), key)
            }
            Fix::Rebind(Side::Action(a), old) | Fix::Unbind(Side::Action(a), old) => {
                let at = settings.iter().position(|s| s.action == a.name());
                let mut keys: Vec<String> = match at {
                    Some(i) => settings[i].keys.clone(),
                    None => a.defaults().iter().map(|k| k.to_string()).collect(),
                };
                let is_old = |k: &String| Key::parse(k).ok() == *old;
                let keys = match (fix, old, new) {
                    (Fix::Rebind(..), Some(_), Some(new)) => {
                        match keys.iter().position(is_old) {
                            Some(i) => keys[i] = new,
                            None => keys.push(new),
                        }
                        Some(keys)
                    }
                    (Fix::Rebind(..), _, new) => Some(new.into_iter().collect()),
                    (Fix::Unbind(..), Some(_), _) => {
                        keys.retain(|k| !is_old(k));
                        // Emptying a rebound action's keys returns it to its default.
                        if keys.is_empty() && at.is_some() { None } else { Some(keys) }
                    }
                    (Fix::Unbind(..), None, _) => None,
                };
                match (&keys, at) {
                    (Some(k), Some(i)) => settings[i].keys = k.clone(),
                    (Some(k), None) => settings.push(KeySetting { action: a.name().into(), keys: k.clone() }),
                    (None, Some(i)) => {
                        settings.remove(i);
                    }
                    (None, None) => {}
                }
                Edit::ActionKeys(a.name().into(), keys)
            }
        };
        (chains, settings, edit)
    }

    /// Saves a fix to the config file, backing it up first; when the file can't be edited
    /// safely, the fix lasts this session and a toast says how to make it by hand.
    fn save(&mut self, edit: &Edit) {
        let Some(path) = self.config_path.clone() else { return };
        // A file that can't be read can't be edited safely; never write over it.
        let edited = std::fs::read_to_string(&path).ok().and_then(|text| match edit {
            Edit::ChainKey(n, k) => config::set_chain_key(&text, n, k.as_deref()),
            Edit::ActionKeys(a, k) => config::set_action_keys(&text, a, k.as_deref()),
        });
        let written = edited.map(|new| {
            if !self.backed_up {
                std::fs::copy(&path, path.with_extension("yaml.bak"))?;
                self.backed_up = true;
            }
            std::fs::write(&path, new)
        });
        match written {
            Some(Ok(())) => {}
            Some(Err(e)) => {
                self.toast = Some(format!("could not save config.yaml ({e}); to keep this, {}", edit.by_hand()))
            }
            None => {
                self.toast = Some(format!(
                    "fixed for this session (config.yaml can't be edited safely); to keep it, {}",
                    edit.by_hand()
                ))
            }
        }
    }
}
