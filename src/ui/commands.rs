//! Commands: what the palette lists, the palette's keys, and launching each kind of command.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::palette::{self, ArgLine, Entry, Kind, Row};
use super::runs::{Display, TakeoverStyle};
use super::{App, Focus};
use crate::rpt::{self, Client, Plugin};

/// rpt's built-in verbs the palette offers, with the usage of those taking an argument line.
/// `add` and `scope match` read stdin, so they are left to chains.
const RPT_BUILTINS: [(&str, &str, Option<&str>); 7] = [
    ("validate", "Check the report and list its problems", None),
    ("findings", "List the report's findings", None),
    ("scope", "List the report's scope", None),
    ("assemble", "Assemble the report into one JSON document", None),
    ("version", "Show rpt's version and formats", None),
    ("set", "Set a field of a finding", Some("rpt set <id> <key> <value>")),
    ("delete", "Delete findings", Some("rpt delete <id>...")),
];

/// The TUI's own actions that work so far, with their keys.
const TUI_ACTIONS: [(&str, &str, &str); 7] = [
    ("quit", "Quit rpt-tui", "q"),
    ("focus_next", "Focus the next panel", "tab"),
    ("focus_findings", "Focus Findings", "1"),
    ("focus_problems", "Focus Problems", "2"),
    ("focus_runs", "Focus Runs", "3"),
    ("takeover", "Take over the selected run", "f"),
    ("kill_or_delete", "Kill the selected run", "x"),
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
        self.plugins = Plugins::Ready(list);
        true
    }

    /// Everything the palette offers, group by group.
    pub(super) fn entries(&self) -> Vec<Entry> {
        let mut out = vec![];
        let mut edit = Entry::new(Kind::Chain, "edit", "Open the selected finding or problem in your editor");
        edit.key = Some("e".into());
        edit.disabled = self.edit_target().is_none().then(|| "needs a finding or problem".into());
        out.push(edit);
        if let Plugins::Ready(list) = &self.plugins {
            out.extend(list.iter().map(plugin_entry));
        }
        for (name, summary, usage) in RPT_BUILTINS {
            let mut e = Entry::new(Kind::Rpt, name, summary);
            e.args = usage.map(String::from);
            e.key = (name == "validate").then(|| "v".into());
            out.push(e);
        }
        for (name, summary, key) in TUI_ACTIONS {
            let mut e = Entry::new(Kind::Tui, name, summary);
            e.key = Some(key.into());
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
        if let Some(args) = state.args.as_mut() {
            match key.code {
                KeyCode::Esc => state.args = None,
                KeyCode::Enter => {
                    let (id, line) = (args.entry.id(), args.command());
                    self.palette = None;
                    self.run_shell(id, &line);
                }
                KeyCode::Backspace => {
                    args.text.pop();
                }
                KeyCode::Char('u') if ctrl => args.text.clear(),
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
        self.palette = None;
        match entry.kind {
            Kind::Chain => self.edit(),
            Kind::Plugin | Kind::Rpt => self.run_shell(entry.id(), &format!("rpt {}", entry.name)),
            Kind::Tui => self.action(&entry.name),
        }
    }

    /// A TUI action by name.
    fn action(&mut self, name: &str) {
        match name {
            "quit" => self.quit = true,
            "focus_next" => self.focus_next(),
            "focus_findings" => self.set_focus(Focus::Findings),
            "focus_problems" => self.set_focus(Focus::Problems),
            "focus_runs" => self.set_focus(Focus::Runs),
            "takeover" => self.take_over_selected(),
            "kill_or_delete" => self.kill_selected(),
            _ => {}
        }
    }

    /// Notes a command as used, most recent first.
    fn used(&mut self, id: String) {
        self.recent.retain(|r| *r != id);
        self.recent.insert(0, id);
    }

    /// Runs command `id` as a shell line in a pane, the way a one-step chain runs:
    /// `bash -o pipefail -c`.
    pub(super) fn run_shell(&mut self, id: String, line: &str) {
        self.used(id);
        let argv = vec!["bash".into(), "-o".into(), "pipefail".into(), "-c".into(), line.into()];
        self.launch(line, argv, Display::Pane);
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
