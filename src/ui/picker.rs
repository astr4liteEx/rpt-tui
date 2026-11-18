//! The file picker: fzf (or the `file_picker:` command) as a short run in a pane, or the
//! built-in list without either. Whatever it picks fills a form field or the argument line.

use std::path::Path;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::runs::Display;
use super::{App, Focus};
use crate::config::ParamKind;
use crate::run::Launch;
use crate::run::chain::Scratch;

/// fzf: the highlighted file, or what was typed when nothing matched (so a path outside the
/// report can be given); esc and errors print nothing and fail.
const FZF: &str =
    r#"out=$(fzf --print-query); code=$?; [ "$code" -le 1 ] || exit "$code"; printf '%s\n' "$out" | tail -n 1"#;

/// The report's files `glob` matches, as paths from the report, sorted; `.git` is skipped.
/// A glob without a `/` matches file names at any depth, one with a `/` the whole path.
pub fn matching_files(root: &Path, glob: Option<&str>) -> Vec<String> {
    let mut out = vec![];
    walk(root, root, &mut out);
    out.retain(|rel| match glob {
        None => true,
        Some(g) if g.contains('/') => glob_match(g, rel),
        Some(g) => glob_match(g, rel.rsplit('/').next().unwrap_or(rel)),
    });
    out.sort();
    out
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let Ok(kind) = e.file_type() else { continue };
        let path = e.path();
        if kind.is_dir() && e.file_name() != ".git" {
            walk(root, &path, out);
        } else if (kind.is_file() || kind.is_symlink() && path.is_file())
            && let Ok(rel) = path.strip_prefix(root)
        {
            out.push(rel.display().to_string());
        }
    }
}

/// `*` matches any run of characters but `/`, `?` any one character but `/`.
fn glob_match(glob: &str, s: &str) -> bool {
    let (g, s): (Vec<char>, Vec<char>) = (glob.chars().collect(), s.chars().collect());
    fn go(g: &[char], s: &[char]) -> bool {
        match g.first() {
            None => s.is_empty(),
            Some('*') => (0..=s.len()).take_while(|&i| i == 0 || s[i - 1] != '/').any(|i| go(&g[1..], &s[i..])),
            Some('?') => s.first().is_some_and(|c| *c != '/') && go(&g[1..], &s[1..]),
            Some(c) => s.first() == Some(c) && go(&g[1..], &s[1..]),
        }
    }
    go(&g, &s)
}

/// Quotes `s` for the shell when it needs it: `it's` becomes `'it'\''s'`.
fn shell_quote(s: &str) -> String {
    let plain = !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "_./-+:@,=%".contains(c));
    if plain { s.to_string() } else { format!("'{}'", s.replace('\'', r"'\''")) }
}

/// What a picked file fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The form's field with this index.
    Field(usize),
    /// The argument line, where the path is added shell-quoted.
    Args,
}

/// A picker run in progress.
pub struct Picking {
    /// The picker run's id among the runs.
    run: u64,
    scratch: Scratch,
    target: Target,
    focus: Focus,
}

/// The built-in list, used when there is no fzf and no `file_picker:`.
#[derive(Debug)]
pub struct ListPicker {
    pub files: Vec<String>,
    pub query: String,
    pub sel: usize,
    target: Target,
}

impl ListPicker {
    /// The files containing the query (ignoring case).
    pub fn shown(&self) -> Vec<&String> {
        let q = self.query.to_lowercase();
        self.files.iter().filter(|f| f.to_lowercase().contains(&q)).collect()
    }
}

impl App {
    /// ctrl-f: picks a file for the form's current field or the argument line.
    pub(super) fn pick_file(&mut self) {
        let Some(state) = self.palette.as_ref() else { return };
        let (target, glob) = match (&state.form, &state.args) {
            (Some(form), _) => match &form.param().kind {
                ParamKind::File { glob } => (Target::Field(form.field), glob.clone()),
                _ => return,
            },
            (None, Some(_)) => (Target::Args, None),
            (None, None) => return,
        };
        let files = matching_files(&self.view.root, glob.as_deref());
        let command = match &self.file_picker {
            Some(c) => c.clone(),
            None if crate::rpt::which(self.client.path(), "fzf").is_some() => FZF.into(),
            None => {
                self.list_picker = Some(ListPicker { files, query: String::new(), sel: 0, target });
                return;
            }
        };
        if let Err(e) = self.start_picker(&command, &files, target) {
            self.toast = Some(format!("could not start the file picker: {e}"));
        }
    }

    /// Runs `command` in a pane with the files on its stdin and its stdout kept.
    fn start_picker(&mut self, command: &str, files: &[String], target: Target) -> std::io::Result<()> {
        let scratch = Scratch::new()?;
        let list = scratch.run_tmp().join("files");
        std::fs::write(&list, files.iter().map(|f| format!("{f}\n")).collect::<String>())?;
        let script = format!("{{ {command}\n}} < \"$1\" > \"$2\"");
        let argv = vec![
            "bash".into(),
            "-c".into(),
            script,
            "rpt-tui-pick".into(),
            list.display().to_string(),
            scratch.output().display().to_string(),
        ];
        let focus = self.focus;
        if let Some(i) = self.start("pick a file", Launch::new(argv, &self.view.root), Display::Pane, None, None) {
            self.picking = Some(Picking { run: self.runs.entries[i].id, scratch, target, focus });
        }
        Ok(())
    }

    /// Once the picker run ends: its first line fills the target if it succeeded, and the
    /// run is removed, leaving no trace.
    pub(super) fn finish_picker(&mut self) {
        let Some(p) = &self.picking else { return };
        let Some(i) = self.runs.entries.iter().position(|e| e.id == p.run) else {
            self.picking = None;
            return;
        };
        if self.runs.entries[i].is_live() {
            return;
        }
        let entry = self.runs.remove(i);
        let p = self.picking.take().expect("checked above");
        if entry.status() == crate::run::Status::Exited(0) {
            let out = std::fs::read_to_string(p.scratch.output()).unwrap_or_default();
            if let Some(line) = out.lines().next().filter(|l| !l.trim().is_empty()) {
                self.fill(p.target, line.trim_end());
            }
        }
        self.runs.keys = false;
        self.set_focus(p.focus);
    }

    /// Puts a picked path into the form field or the argument line.
    fn fill(&mut self, target: Target, path: &str) {
        let Some(state) = self.palette.as_mut() else { return };
        match target {
            Target::Field(i) => {
                if let Some(v) = state.form.as_mut().and_then(|f| f.values.get_mut(i)) {
                    *v = path.to_string();
                }
            }
            Target::Args => {
                if let Some(args) = state.args.as_mut() {
                    if !args.text.is_empty() && !args.text.ends_with(' ') {
                        args.text.push(' ');
                    }
                    args.text.push_str(&shell_quote(path));
                }
            }
        }
    }

    /// Keys while the built-in list is open.
    pub(super) fn list_picker_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(lp) = self.list_picker.as_mut() else { return };
        let count = lp.shown().len();
        match key.code {
            KeyCode::Esc => self.list_picker = None,
            KeyCode::Enter => {
                let picked = lp.shown().get(lp.sel).map(|s| s.to_string()).unwrap_or_else(|| lp.query.clone());
                let target = lp.target;
                self.list_picker = None;
                if !picked.is_empty() {
                    self.fill(target, &picked);
                }
            }
            KeyCode::Up => lp.sel = lp.sel.saturating_sub(1),
            KeyCode::Char('p') if ctrl => lp.sel = lp.sel.saturating_sub(1),
            KeyCode::Down => lp.sel = (lp.sel + 1).min(count.saturating_sub(1)),
            KeyCode::Char('n') if ctrl => lp.sel = (lp.sel + 1).min(count.saturating_sub(1)),
            KeyCode::Backspace => {
                lp.query.pop();
                lp.sel = 0;
            }
            KeyCode::Char(c) if !ctrl => {
                lp.query.push(c);
                lp.sel = 0;
            }
            _ => {}
        }
    }
}
