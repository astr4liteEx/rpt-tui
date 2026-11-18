//! Commands: what the palette lists, the palette's keys, and launching each kind of command.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::form::Outcome;
use super::palette::{self, ArgLine, Entry, Kind, Row};
use super::runs::{Display, TakeoverStyle};
use super::{App, Focus};
use crate::config::Step;
use crate::keys::Action;
use crate::rpt::{self, Client, Plugin};

/// rpt's built-in verbs the palette leaves out: `add` reads stdin, so it is for chains (as is
/// `scope match`); `new` can't make a report inside one, where the TUI always is; `ls` lists other
/// reports, and the TUI is about the one it's in; `plugins` and `verbs` the palette lists itself.
const UNOFFERED_VERBS: [&str; 5] = ["add", "new", "ls", "plugins", "verbs"];

/// A verb takes an argument line when its usage, after `rpt <verb>`, starts with a `<…>`
/// placeholder, as `rpt set <id> <key> <value>` does.
fn takes_arguments(v: &rpt::Verb) -> bool {
    v.usage.strip_prefix(&format!("rpt {}", v.name)).is_some_and(|rest| rest.trim_start().starts_with('<'))
}

/// Actions the palette leaves out: list movement, run keys, and those with a row of their own
/// (`validate` is rpt's, `edit` a chain's).
const UNLISTED: [Action; 11] = [
    Action::Palette,
    Action::Down,
    Action::Up,
    Action::Top,
    Action::Bottom,
    Action::Open,
    Action::Back,
    Action::Edit,
    Action::Validate,
    Action::LeaveRun,
    Action::ToggleTakeover,
];

/// rpt's plugins: being listed in the background, then the list.
pub(super) enum Plugins {
    Loading(Receiver<Result<Vec<Plugin>, rpt::Error>>),
    Ready(Vec<Plugin>),
}

impl Plugins {
    /// Starts `rpt plugins` on a thread, so the TUI is up before plugins' `--describe` answers.
    pub(super) fn load(client: &Client) -> Plugins {
        let (tx, rx) = std::sync::mpsc::channel();
        let client = client.clone();
        std::thread::spawn(move || {
            let _ = tx.send(client.plugins());
        });
        Plugins::Loading(rx)
    }
}

fn plugin_entry(p: &Plugin) -> Entry {
    let mut e = Entry::new(Kind::Plugin, &p.name, p.summary.as_deref().unwrap_or(""));
    e.io = match (&p.input, &p.output) {
        (None, None) => None,
        (i, o) => Some(format!("{} → {}", i.as_deref().unwrap_or("?"), o.as_deref().unwrap_or("?"))),
    };
    e.args = Some(p.usage.clone().unwrap_or_default());
    e
}

impl App {
    /// Takes `rpt plugins`' answer once it is in. True when it arrived.
    pub(super) fn poll_plugins(&mut self) -> bool {
        let Plugins::Loading(rx) = &self.plugins else { return false };
        let list = match rx.try_recv() {
            Err(TryRecvError::Empty) => return false,
            Ok(Ok(list)) => list,
            Ok(Err(e)) => {
                self.toast = Some(format!("could not list plugins: {e}"));
                vec![]
            }
            Err(TryRecvError::Disconnected) => vec![],
        };
        let names: Vec<String> = list.iter().map(|p| p.name.clone()).collect();
        self.plugins = Plugins::Ready(list);
        self.drop_chains_named_like_plugins(&names);
        true
    }

    /// Everything the palette offers, group by group.
    pub(super) fn entries(&self) -> Vec<Entry> {
        let mut out = vec![];
        let written = |a: Action| Some(self.bindings.written(a)).filter(|k| !k.is_empty());
        // `edit` comes first, keyed by the `edit` action: the user's chain of that name, or the
        // built-in.
        let mut edit = match self.user_edit() {
            Some(c) => {
                let mut e = Entry::new(Kind::Chain, "edit", &c.description);
                e.disabled = self.unmet(c.needs).map(String::from);
                e
            }
            None => {
                let mut e = Entry::new(Kind::Chain, "edit", "Open the selected finding or problem in your editor");
                e.builtin = true;
                e.disabled = self.edit_target().is_none().then(|| "needs a finding or problem".into());
                e
            }
        };
        edit.key = written(Action::Edit);
        out.push(edit);
        for c in self.chains.iter().filter(|c| c.name != "edit") {
            let mut e = Entry::new(Kind::Chain, &c.name, &c.description);
            e.key = self.bindings.chain_key(&c.name).map(|k| k.to_string());
            e.disabled = self.unmet(c.needs).map(String::from);
            out.push(e);
        }
        if let Plugins::Ready(list) = &self.plugins {
            out.extend(list.iter().map(plugin_entry));
        }
        for v in self.verbs.iter().filter(|v| !UNOFFERED_VERBS.contains(&v.name.as_str())) {
            let mut e = Entry::new(Kind::Rpt, &v.name, &v.summary);
            e.args = takes_arguments(v).then(|| v.usage.clone());
            e.key = if v.name == "validate" { written(Action::Validate) } else { None };
            out.push(e);
        }
        let fixable = self.fixable_troubles() > 0;
        for a in Action::ALL.iter().filter(|a| a.works() && !UNLISTED.contains(a)) {
            if *a == Action::FixKeys && !fixable {
                continue;
            }
            let mut e = Entry::new(Kind::Tui, a.name(), a.summary());
            e.key = written(*a);
            out.push(e);
        }
        out
    }

    /// The palette's rows for the current query.
    pub(super) fn palette_rows(&self, entries: &[Entry]) -> Vec<Row> {
        let query = self.palette.as_ref().map_or("", |p| p.query.as_str());
        palette::order(entries, query, &self.recent, matches!(self.plugins, Plugins::Loading(_)))
    }

    /// The highlighted entry's index into `entries`.
    pub(super) fn palette_selected(&self, rows: &[Row]) -> Option<usize> {
        let sel = self.palette.as_ref()?.sel;
        rows.iter().filter_map(|r| if let Row::Entry(i) = r { Some(*i) } else { None }).nth(sel)
    }

    pub(super) fn open_palette(&mut self) {
        self.palette = Some(palette::State::default());
    }

    /// Keys while the palette is open.
    pub(super) fn palette_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(state) = self.palette.as_mut() else { return };
        if let Some(form) = state.form.as_mut() {
            match form.key(key) {
                Outcome::Stay => {}
                Outcome::PickFile => self.pick_file(),
                Outcome::Back => state.form = None,
                Outcome::Run => {
                    let form = state.form.take().expect("the form just ran");
                    self.palette = None;
                    self.run_chain(&form);
                }
            }
            return;
        }
        if let Some(line) = state.export.as_mut() {
            match key.code {
                KeyCode::Esc => state.export = None,
                KeyCode::Enter => {
                    let (id, text) = (line.run, line.text.clone());
                    self.palette = None;
                    self.export(id, &text);
                }
                KeyCode::Backspace => {
                    line.text.pop();
                }
                KeyCode::Char('u') if ctrl => line.text.clear(),
                KeyCode::Char(c) if !ctrl => line.text.push(c),
                _ => {}
            }
            return;
        }
        if let Some(args) = state.args.as_mut() {
            match key.code {
                KeyCode::Esc => state.args = None,
                KeyCode::Enter => {
                    let (id, line) = (args.entry.id(), args.command());
                    self.palette = None;
                    self.run_line(id, &line);
                }
                KeyCode::Backspace => {
                    args.text.pop();
                }
                KeyCode::Char('u') if ctrl => args.text.clear(),
                KeyCode::Char('f') if ctrl => self.pick_file(),
                KeyCode::Char(c) if !ctrl => args.text.push(c),
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Esc => self.palette = None,
            KeyCode::Enter => self.choose(),
            KeyCode::Up => state.sel = state.sel.saturating_sub(1),
            KeyCode::Char('p') if ctrl => state.sel = state.sel.saturating_sub(1),
            KeyCode::Down => state.sel += 1,
            KeyCode::Char('n') if ctrl => state.sel += 1,
            KeyCode::Backspace => {
                state.query.pop();
                state.sel = 0;
            }
            KeyCode::Char(c) if !ctrl => {
                state.query.push(c);
                state.sel = 0;
            }
            _ => {}
        }
        self.clamp_palette();
    }

    fn clamp_palette(&mut self) {
        let entries = self.entries();
        let count = self.palette_rows(&entries).iter().filter(|r| matches!(r, Row::Entry(_))).count();
        if let Some(state) = self.palette.as_mut() {
            state.sel = state.sel.min(count.saturating_sub(1));
        }
    }

    /// Enter in the palette: an argument line for entries taking one, otherwise run it now.
    fn choose(&mut self) {
        let entries = self.entries();
        let rows = self.palette_rows(&entries);
        let Some(entry) = self.palette_selected(&rows).map(|i| entries[i].clone()) else { return };
        if entry.disabled.is_some() {
            return;
        }
        if entry.args.is_some() {
            if let Some(state) = self.palette.as_mut() {
                state.args = Some(ArgLine::new(entry));
            }
            return;
        }
        match entry.kind {
            Kind::Chain if entry.builtin => {
                self.palette = None;
                self.edit()
            }
            Kind::Chain => {
                if let Some(chain) = self.chains.iter().find(|c| c.name == entry.name).cloned() {
                    self.choose_chain(chain);
                }
            }
            Kind::Plugin | Kind::Rpt => {
                self.palette = None;
                self.run_line(entry.id(), &format!("rpt {}", entry.name))
            }
            Kind::Tui => {
                self.palette = None;
                if let Some(a) = Action::named(&entry.name) {
                    self.act(a);
                }
            }
        }
    }

    /// Does an action while keys go to the TUI.
    pub(super) fn act(&mut self, action: Action) {
        match action {
            Action::Palette => self.open_palette(),
            Action::Quit => self.request_quit(),
            Action::Help => self.help = Some(0),
            Action::FixKeys => self.open_guide(),
            Action::FocusNext => self.focus_next(),
            Action::FocusFindings => self.set_focus(Focus::Findings),
            Action::FocusProblems => self.set_focus(Focus::Problems),
            Action::FocusRuns => self.set_focus(Focus::Runs),
            Action::Validate => self.run_line(Entry::new(Kind::Rpt, "validate", "").id(), "rpt validate"),
            Action::Down => self.move_by(1),
            Action::Up => self.move_by(-1),
            Action::Top => self.move_to(0),
            Action::Bottom => self.move_to(usize::MAX),
            // A live run gets the keys; a finished one shows its transcript full width.
            Action::Open if self.focus == Focus::Runs => match self.runs.selected() {
                Some(e) if e.is_live() => self.runs.keys = true,
                Some(_) => {
                    self.full = true;
                    self.scroll = 0;
                }
                None => {}
            },
            Action::Open => {
                self.full = true;
                self.scroll = 0;
            }
            Action::Back if self.full => self.full = false,
            Action::Back if self.focus == Focus::Runs => self.set_focus(self.last_list),
            Action::Edit => self.edit_action(),
            Action::Rerun => self.rerun(),
            Action::Takeover => self.take_over_selected(),
            Action::KillOrDelete => self.kill_or_delete(),
            Action::ClearHistory => self.ask_clear_history(),
            Action::ExportTranscript => self.ask_export(),
            Action::Theme => self.open_theme_picker(),
            Action::ReloadConfig => self.reload_config(),
            _ => {}
        }
    }

    /// `rerun`: the selected run's command as configured now, its form or argument line filled
    /// with what it ran with; anything else runs at once.
    fn rerun(&mut self) {
        let Some(e) = self.runs.selected() else { return };
        if e.is_live() {
            self.toast = Some(format!("{} is still running", e.label));
            return;
        }
        let Some(inv) = e.invocation.clone() else {
            self.toast = Some(format!("{} can't be run again", e.label));
            return;
        };
        let name = inv.command.split_once(' ').map_or("", |(_, n)| n).to_string();
        if inv.command.starts_with("chain ") {
            let Some(chain) = self.chains.iter().find(|c| c.name == name).cloned() else {
                self.toast = Some(format!("chain {name} is no longer in the config"));
                return;
            };
            match self.unmet(chain.needs) {
                Some(why) => self.toast = Some(format!("{name} {why}")),
                None => self.choose_chain_with(chain, &inv.params),
            }
            return;
        }
        let Some(entry) = self.entries().into_iter().find(|x| x.id() == inv.command) else {
            self.toast = Some(match self.plugins {
                Plugins::Loading(_) => "plugins are still loading; try again in a moment".into(),
                Plugins::Ready(_) => format!("{} is no longer available", inv.command),
            });
            return;
        };
        if entry.args.is_some() {
            let mut line = ArgLine::new(entry);
            line.text = inv.args.unwrap_or_default();
            self.palette = Some(palette::State { args: Some(line), ..Default::default() });
        } else {
            self.run_line(inv.command, &format!("rpt {name}"));
        }
    }

    /// `e`: a user chain named `edit`, else the built-in.
    fn edit_action(&mut self) {
        if self.user_edit().is_some() {
            self.hotkey("edit");
        } else {
            self.edit();
        }
    }

    /// The user's chain named `edit`, which replaces the built-in.
    fn user_edit(&self) -> Option<&crate::config::Chain> {
        self.chains.iter().find(|c| c.name == "edit")
    }

    /// A chain's hotkey: runs it (or opens its form), unless its `needs` aren't met.
    pub(super) fn hotkey(&mut self, name: &str) {
        let Some(chain) = self.chains.iter().find(|c| c.name == name).cloned() else { return };
        match self.unmet(chain.needs) {
            Some(why) => self.toast = Some(format!("{name} {why}")),
            None => self.choose_chain(chain),
        }
    }

    /// Notes a command as used, most recent first.
    pub(super) fn used(&mut self, id: String) {
        if let Some(store) = &self.history {
            store.note_used(&id);
        }
        self.recent.retain(|r| *r != id);
        self.recent.insert(0, id);
    }

    /// Runs command `id`, a plugin or rpt built-in, as a one-step chain in a pane.
    pub(super) fn run_line(&mut self, id: String, line: &str) {
        let verb = id.split_once(' ').map_or("", |(_, v)| v);
        let args = line.strip_prefix(&format!("rpt {verb}")).map(|a| a.trim_start().to_string());
        let invocation = crate::history::Invocation { command: id, params: vec![], args };
        self.start_steps(invocation, line, &[Step::new(line)], vec![], Display::Pane, false);
    }

    /// The file `edit` opens, and its line: the highlighted row of whichever of Findings and
    /// Problems was focused last.
    pub(super) fn edit_target(&self) -> Option<(PathBuf, Option<u32>)> {
        match self.last_list {
            Focus::Problems => {
                let p = self.selected_problem().filter(|p| !p.path.is_empty())?;
                Some((self.view.root.join(&p.path), p.line))
            }
            _ => self.selected_finding().map(|x| (self.view.root.join(&x.path), None)),
        }
    }

    /// The built-in `edit`: the editor on the selected item, raw takeover, ephemeral.
    pub(super) fn edit(&mut self) {
        let Some((file, line)) = self.edit_target() else {
            self.toast = Some("edit needs a finding or problem".into());
            return;
        };
        let (visual, editor) = &self.editor_env;
        let Some(argv) =
            super::edit::editor_argv(visual.as_deref(), editor.as_deref(), self.client.path(), &file, line)
        else {
            self.toast = Some("no editor found: set $VISUAL or $EDITOR".into());
            return;
        };
        self.used(Entry::new(Kind::Chain, "edit", "").id());
        let rel = file.strip_prefix(&self.view.root).unwrap_or(&file).display().to_string();
        if let Some(i) = self.launch(&format!("edit {rel}"), argv, Display::Takeover(TakeoverStyle::Raw)) {
            self.runs.entries[i].ephemeral = true;
        }
    }
}
