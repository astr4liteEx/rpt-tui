//! Run history in the UI: saving runs as they end, listing earlier sessions' runs, deleting
//! and clearing them, replaying a transcript for drawing, and `export-transcript`.

use std::path::Path;

use ratatui::layout::Rect;

use super::runs::RunEntry;
use super::{App, Confirm};
use crate::run::Size;

/// A finished run's transcript played into a terminal, for drawing.
pub(super) struct Replay {
    /// The run replayed.
    id: u64,
    /// The terminal it was replayed into.
    size: Size,
    /// All of it kept in scrollback, for the full view.
    full: bool,
    parser: vt100::Parser,
}

impl App {
    /// Saves run `id` to history once it has ended (unless ephemeral, or no command), then
    /// prunes; runs pruned away leave Runs.
    pub(super) fn save_run(&mut self, id: u64) {
        let Some(store) = &self.history else { return };
        let Some(e) = self.runs.entries.iter_mut().find(|e| e.id == id) else { return };
        if e.ephemeral || e.saved.is_some() {
            return;
        }
        let Some(record) = e.record() else { return };
        let transcript = e.run.as_ref().map(crate::run::Run::transcript).unwrap_or_default();
        let failed = match store.save(&record, &transcript) {
            Ok(key) => {
                e.saved = Some(key);
                None
            }
            Err(err) => Some(format!("could not save {} to history: {err}", e.label)),
        };
        let gone = store.prune(std::time::SystemTime::now());
        if let Some(msg) = failed {
            self.add_toast(msg);
        }
        // Through `remove`, which keeps the selection and takeover on the same runs.
        while let Some(i) =
            self.runs.entries.iter().position(|e| !e.is_live() && e.saved.as_ref().is_some_and(|k| gone.contains(k)))
        {
            self.runs.remove(i);
        }
    }

    /// Lists the runs history kept from earlier sessions (after pruning it); a toast says how
    /// many could not be read.
    pub(super) fn load_history(&mut self) {
        let Some(store) = &self.history else { return };
        store.prune(std::time::SystemTime::now());
        let listing = store.list();
        for saved in listing.runs {
            self.runs.next_id += 1;
            self.runs.entries.push(RunEntry::earlier(self.runs.next_id, saved));
        }
        if listing.unreadable > 0 {
            let n = listing.unreadable;
            self.add_toast(format!("{n} {} in history could not be read", super::plural(n, "run")));
        }
        self.runs.select_end(false);
    }

    /// Removes a finished run from Runs and from history; the row below it is selected.
    pub(super) fn delete_run(&mut self, id: u64) {
        let Some(i) = self.runs.entries.iter().position(|e| e.id == id && !e.is_live()) else { return };
        let order = self.runs.order();
        let at = order.iter().position(|&j| j == i).unwrap_or(0);
        let e = self.runs.remove(i);
        if let (Some(key), Some(store)) = (&e.saved, &self.history)
            && let Err(err) = store.delete(key)
        {
            self.toast = Some(format!("could not delete {} from history: {err}", e.label));
        }
        let order = self.runs.order();
        if let Some(&j) = order.get(at).or(order.last()) {
            self.runs.sel = j;
        }
    }

    /// `clear-history`: asks first, unless there is nothing to clear.
    pub(super) fn ask_clear_history(&mut self) {
        match &self.history {
            None => self.toast = Some("run history is off".into()),
            Some(_) if !self.runs.entries.iter().any(|e| e.saved.is_some()) => {
                self.toast = Some("this report's history is empty".into())
            }
            Some(_) => self.confirm = Some(Confirm::Clear),
        }
    }

    /// Empties this report's history; live runs stay.
    /// Only the runs listed (those the question counted): another session's stay.
    pub(super) fn clear_history(&mut self) {
        let Some(store) = self.history.clone() else { return };
        let mut failed = 0;
        while let Some(i) = self.runs.entries.iter().position(|e| e.saved.is_some()) {
            let e = self.runs.remove(i);
            failed += e.saved.is_some_and(|key| store.delete(&key).is_err()) as usize;
        }
        if failed > 0 {
            self.add_toast(format!("could not delete {failed} {} from history", super::plural(failed, "run")));
        }
        self.runs.select_end(false);
    }

    /// Calls `f` with run `i`'s transcript played into a terminal the size of `area`: with all
    /// of it in scrollback for the full view, else just the end. Kept until the run or size
    /// changes, since a transcript can be megabytes.
    pub(super) fn with_replay<R>(
        &self,
        i: usize,
        area: Rect,
        full: bool,
        f: impl FnOnce(&mut vt100::Parser) -> R,
    ) -> R {
        let size = Size { rows: area.height.max(1), cols: area.width.max(1) };
        let id = self.runs.entries.get(i).map_or(0, |e| e.id);
        let mut cache = self.replay.borrow_mut();
        let stale = cache.as_ref().is_none_or(|r| (r.id, r.size, r.full) != (id, size, full));
        if stale {
            let text = self.runs.entries.get(i).map(|e| self.transcript_of(e)).unwrap_or_default();
            let (rows, cols) = size.cells();
            let mut parser = vt100::Parser::new(rows, cols, if full { usize::MAX } else { 0 });
            parser.process(text.replace('\n', "\r\n").as_bytes());
            *cache = Some(Replay { id, size, full, parser });
        }
        f(&mut cache.as_mut().expect("filled above").parser)
    }

    /// `export-transcript`: a path line for the selected run, pre-filled
    /// `~/<label>-<YYYYMMDD-HHMMSS>.txt`.
    pub(super) fn ask_export(&mut self) {
        let Some(e) = self.runs.selected() else {
            self.toast = Some("export-transcript: no run selected".into());
            return;
        };
        let slug: String =
            e.label.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '-' }).collect();
        let text = format!("~/{slug}-{}.txt", crate::time::stamp(e.started));
        let line = super::palette::ExportLine { run: e.id, text };
        self.palette = Some(super::palette::State { export: Some(line), ..Default::default() });
    }

    /// Saves run `id`'s transcript as plain text to `typed` (a leading `~/` is the home folder),
    /// asking before writing over a file; never inside the report.
    pub(super) fn export(&mut self, id: u64, typed: &str) {
        let typed = Path::new(typed.trim());
        let path = crate::config::expand_home(typed, std::env::var_os("HOME").as_deref())
            .unwrap_or_else(|| typed.to_path_buf());
        let path = std::path::absolute(&path).unwrap_or(path);
        if self.inside_report(&path) {
            self.toast = Some("inside the report: rpt-tui doesn't write there".into());
        } else if path.exists() {
            self.confirm = Some(Confirm::Overwrite { run: id, path });
        } else {
            self.write_transcript(id, &path);
        }
    }

    /// Whether `path` is in the report folder, links followed.
    fn inside_report(&self, path: &Path) -> bool {
        let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let root = real(&self.view.root);
        // The file may not exist yet: its nearest folder that does decides.
        let mut at = path.to_path_buf();
        while !at.exists() && at.pop() {}
        real(&at).starts_with(&root) || path.starts_with(&self.view.root)
    }

    /// Writes run `id`'s transcript to `path` as plain text; a toast says how it went.
    pub(super) fn write_transcript(&mut self, id: u64, path: &Path) {
        let Some(e) = self.runs.entries.iter().find(|e| e.id == id) else { return };
        let mut text = crate::run::plain(&self.transcript_of(e));
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        self.toast = Some(match std::fs::write(path, text) {
            Ok(()) => format!("saved the transcript of {} to {}", e.label, path.display()),
            Err(err) => format!("could not save the transcript to {}: {err}", path.display()),
        });
    }

    /// A run's coloured transcript: from the process, else from history.
    pub(super) fn transcript_of(&self, e: &RunEntry) -> String {
        match (&e.run, &e.saved, &self.history) {
            (Some(run), ..) => run.transcript(),
            (None, Some(key), Some(store)) => {
                store.transcript(key).unwrap_or_else(|err| format!("(the transcript could not be read: {err})"))
            }
            _ => String::new(),
        }
    }
}
