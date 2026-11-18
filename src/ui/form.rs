//! The parameter form a chain opens before it runs: one field per parameter.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::{Chain, Param, ParamKind};

/// The open form: the chain, a value per parameter, and the field with the cursor.
#[derive(Debug)]
pub struct Form {
    pub chain: Chain,
    pub values: Vec<String>,
    pub field: usize,
    /// Why enter didn't run, such as a required field left empty.
    pub error: Option<String>,
}

/// What a key in the form asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    /// Back to the palette.
    Back,
    /// Run the chain with the values.
    Run,
    /// Pick a file for the current field.
    PickFile,
}

impl Form {
    /// Fields start at their defaults; a flag without one starts at `no`.
    pub fn new(chain: Chain) -> Form {
        let values = chain
            .params
            .iter()
            .map(|p| match (&p.default, &p.kind) {
                (Some(d), _) => d.clone(),
                (None, ParamKind::Flag) => "no".into(),
                (None, _) => String::new(),
            })
            .collect();
        Form { chain, values, field: 0, error: None }
    }

    /// The parameter of the field with the cursor.
    pub fn param(&self) -> &Param {
        &self.chain.params[self.field]
    }

    /// One key in the form; any key but enter clears the last error.
    pub fn key(&mut self, key: KeyEvent) -> Outcome {
        if key.code != KeyCode::Enter {
            self.error = None;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let n = self.values.len();
        let kind = self.param().kind.clone();
        let value = &mut self.values[self.field];
        match key.code {
            KeyCode::Esc => return Outcome::Back,
            KeyCode::Enter => return self.submit(),
            KeyCode::Tab | KeyCode::Down => self.field = (self.field + 1) % n,
            KeyCode::BackTab | KeyCode::Up => self.field = (self.field + n - 1) % n,
            KeyCode::Char('f') if ctrl && matches!(kind, ParamKind::File { .. }) => return Outcome::PickFile,
            KeyCode::Left | KeyCode::Right if matches!(kind, ParamKind::Choice { .. }) => {
                let ParamKind::Choice { options } = &kind else { unreachable!() };
                let at = options.iter().position(|o| o == value);
                let next = match (at, key.code == KeyCode::Right) {
                    (None, true) => 0,
                    (None, false) => options.len() - 1,
                    (Some(i), true) => (i + 1) % options.len(),
                    (Some(i), false) => (i + options.len() - 1) % options.len(),
                };
                *value = options[next].clone();
            }
            KeyCode::Char(' ') if kind == ParamKind::Flag => {
                *value = if value == "yes" { "no" } else { "yes" }.into();
            }
            _ if matches!(kind, ParamKind::Choice { .. } | ParamKind::Flag) => {}
            KeyCode::Backspace => {
                value.pop();
            }
            KeyCode::Char('u') if ctrl => value.clear(),
            KeyCode::Char(c) if !ctrl => value.push(c),
            _ => {}
        }
        Outcome::Stay
    }

    /// Enter: runs, unless a required field is empty; then the cursor goes there.
    fn submit(&mut self) -> Outcome {
        let missing = self.chain.params.iter().zip(&self.values).position(|(p, v)| p.required() && v.is_empty());
        match missing {
            Some(i) => {
                self.field = i;
                self.error = Some(format!("{} is required", self.chain.params[i].name));
                Outcome::Stay
            }
            None => Outcome::Run,
        }
    }

    /// The parameters as environment variables; an empty optional one is set to "".
    pub fn env(&self) -> Vec<(String, String)> {
        self.chain.params.iter().zip(&self.values).map(|(p, v)| (p.name.clone(), v.clone())).collect()
    }

    /// The run's label: the chain's name, then each value given (`SCAN=a.nessus`).
    pub fn label(&self) -> String {
        let mut label = self.chain.name.clone();
        for (k, v) in self.env().into_iter().filter(|(_, v)| !v.is_empty()) {
            label.push_str(&format!(" {k}={v}"));
        }
        label
    }
}
