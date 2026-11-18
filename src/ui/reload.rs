//! Reading the config again (spec §8): on `reload-config`, and after a run ends when
//! `config.yaml` or a theme file changed. No file watching.

use std::path::PathBuf;
use std::time::SystemTime;

use super::{App, commands::Plugins, theme};
use crate::config::{self, HistorySettings};

/// What the config files were when last read: to tell when one changed.
#[derive(Default)]
pub struct ConfigFiles {
    /// `config.yaml` and each `themes/*.yaml`, with when each was last changed.
    stamps: Vec<(PathBuf, Option<SystemTime>)>,
    /// `history:` as the file wrote it; a change waits for the next start.
    history: HistorySettings,
}

impl App {
    /// The config files as they now are; none without a config file.
    fn config_stamps(&self) -> Vec<(PathBuf, Option<SystemTime>)> {
        let Some(path) = &self.config_path else { return vec![] };
        let mut files = vec![path.clone()];
        if let Some(dir) = self.config_dir().and_then(|d| std::fs::read_dir(d.join("themes")).ok()) {
            let mut themes: Vec<PathBuf> = dir.flatten().map(|e| e.path()).collect();
            themes.sort();
            files.extend(themes.into_iter().filter(|p| p.extension().is_some_and(|x| x == "yaml")));
        }
        files.into_iter().map(|p| (p.clone(), std::fs::metadata(&p).and_then(|m| m.modified()).ok())).collect()
    }

    /// Notes the config files as read now: at start, after a reload, and after the TUI's own
    /// writes (which need no reload).
    pub(super) fn note_config_files(&mut self, history: Option<HistorySettings>) {
        self.config_files.stamps = self.config_stamps();
        if let Some(h) = history {
            self.config_files.history = h;
        }
    }

    /// The TUI wrote `path` itself: its new stamp needs no reload; other files' changes still
    /// do.
    pub(super) fn note_own_write(&mut self, path: &std::path::Path) {
        let now = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        if let Some(stamp) = self.config_files.stamps.iter_mut().find(|(p, _)| p == path) {
            stamp.1 = now;
        }
    }

    /// After a run ends: reloads when a config file changed since it was read.
    pub(super) fn reload_if_changed(&mut self) {
        if self.config_path.is_some() && self.config_stamps() != self.config_files.stamps {
            self.reload_config();
        }
    }

    /// `reload-config`: theme, chains, keys, `file_picker` and `notify` from the file again,
    /// and the plugins listed again. Live runs are untouched; key troubles open the guide.
    pub(super) fn reload_config(&mut self) {
        let Some(path) = self.config_path.clone() else {
            self.toast = Some("no config file to reload".into());
            return;
        };
        let new = config::load_file(&path);
        let mut errors = new.errors.clone();
        if new.history != self.config_files.history {
            errors.push("history: changes apply at the next start".into());
        }
        let (resolved, theme_errors) = theme::resolve(new.theme.as_ref(), self.config_dir(), self.truecolor);
        errors.extend(theme_errors);
        self.theme = resolved;
        self.theme_setting = new.theme;
        self.chains = new.chains;
        self.key_settings = new.keys;
        self.file_picker = new.file_picker;
        self.notify = new.notify;
        self.rebind();
        errors.extend(self.bindings.troubles.iter().filter(|t| t.side().is_none()).map(|t| t.to_string()));
        // Chains named like a plugin are dropped once the new list is in.
        self.plugins = Plugins::load(&self.client);
        // Added to what is up (a refresh after the same run may have toasted).
        if !errors.is_empty() {
            self.add_toast(errors.join("; "));
        }
        self.note_config_files(None);
        self.open_guide();
    }
}
