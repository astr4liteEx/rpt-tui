//! The `theme` picker: previews each theme live; enter keeps it and writes it to `config.yaml`,
//! esc puts the old one back.

use std::path::Path;

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use serde_json::Value;

use super::{App, theme};

/// The open picker.
pub struct ThemePicker {
    /// The themes offered: built-ins, then theme files.
    pub names: Vec<String>,
    /// The one previewed.
    pub sel: usize,
}

impl App {
    /// The folder `themes/` is read from: the config file's.
    pub(super) fn config_dir(&self) -> Option<&Path> {
        self.config_path.as_deref().and_then(Path::parent)
    }

    /// The theme `theme:` names now: its value, or its `base:` when it is a map.
    fn theme_name(&self) -> String {
        match &self.theme_setting {
            Some(Value::String(name)) => name.clone(),
            Some(Value::Object(m)) => m.get("base").and_then(Value::as_str).unwrap_or("charm").to_string(),
            _ => "charm".into(),
        }
    }

    /// `theme:` with `name` chosen: a map keeps its overrides on the new base.
    fn theme_with(&self, name: &str) -> Value {
        match &self.theme_setting {
            Some(Value::Object(m)) => {
                let mut m = m.clone();
                m.insert("base".into(), Value::String(name.into()));
                Value::Object(m)
            }
            _ => Value::String(name.into()),
        }
    }

    /// Draws everything in the theme `setting` describes; its errors, as at startup, go in
    /// the toast (a theme file may be one never used before, or edited since).
    fn show_theme(&mut self, setting: Option<&Value>) {
        let (theme, errors) = theme::resolve(setting, self.config_dir(), self.truecolor);
        self.theme = theme;
        if !errors.is_empty() {
            self.toast = Some(errors.join("; "));
        }
    }

    /// `theme`: the picker, on the theme in use.
    pub(super) fn open_theme_picker(&mut self) {
        let names = theme::names(self.config_dir());
        let sel = names.iter().position(|n| *n == self.theme_name()).unwrap_or(0);
        self.theme_picker = Some(ThemePicker { names, sel });
    }

    /// Keys in the picker: j/k or arrows preview, enter keeps, esc puts the old theme back.
    pub(super) fn theme_picker_key(&mut self, key: KeyEvent) {
        let Some(p) = self.theme_picker.as_mut() else { return };
        let last = p.names.len().saturating_sub(1);
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => p.sel = (p.sel + 1).min(last),
            KeyCode::Up | KeyCode::Char('k') => p.sel = p.sel.saturating_sub(1),
            KeyCode::Esc => {
                self.theme_picker = None;
                self.show_theme(self.theme_setting.clone().as_ref());
                return;
            }
            KeyCode::Enter => {
                let name = p.names[p.sel].clone();
                self.theme_picker = None;
                self.keep_theme(&name);
                return;
            }
            _ => return,
        }
        let name = p.names[p.sel].clone();
        self.show_theme(Some(&self.theme_with(&name)));
    }

    /// Uses theme `name` from now on, and writes it to `config.yaml`.
    fn keep_theme(&mut self, name: &str) {
        let setting = self.theme_with(name);
        self.show_theme(Some(&setting));
        let by_hand = match &setting {
            Value::Object(_) => format!("under theme:, write `base: {name}`"),
            _ => format!("write `theme: {name}`"),
        };
        self.theme_setting = Some(setting);
        self.edit_config(|text| crate::config::set_theme(text, name), &by_hand);
    }
}
