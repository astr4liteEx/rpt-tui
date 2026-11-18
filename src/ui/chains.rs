//! Chains from the config: starting them as one run, and the ones plugins now shadow.

use super::App;
use super::form::Form;
use super::runs::Display;
use crate::config::{self, Chain, Step};
use crate::run::chain::{Scratch, steps_launch};

impl App {
    /// Starts `steps` as one run: a chain, or a plugin or rpt built-in as a one-step chain.
    pub(super) fn start_steps(
        &mut self,
        id: String,
        label: &str,
        steps: &[Step],
        env: Vec<(String, String)>,
        display: Display,
        ephemeral: bool,
    ) {
        self.used(id);
        let scratch = match Scratch::new() {
            Ok(s) => s,
            Err(e) => {
                self.toast = Some(format!("could not start {label}: no scratch folder: {e}"));
                return;
            }
        };
        let launch = steps_launch(steps, env, &self.view.root, &scratch);
        if let Some(i) = self.start(label, launch, display, Some(scratch)) {
            self.runs.entries[i].ephemeral = ephemeral;
        }
    }

    /// Runs a chain with its form's values (none for a chain without parameters).
    pub(super) fn run_chain(&mut self, form: &Form) {
        let chain = &form.chain;
        let display = match chain.display {
            config::Display::Pane => Display::Pane,
            config::Display::Takeover => Display::Takeover(chain.takeover),
        };
        let id = super::palette::Entry::new(super::palette::Kind::Chain, &chain.name, "").id();
        let steps = chain.steps.clone();
        self.start_steps(id, &form.label(), &steps, form.env(), display, chain.ephemeral);
    }

    /// Opens the chain's form, or runs it at once when it has no parameters.
    pub(super) fn choose_chain(&mut self, chain: Chain) {
        let form = Form::new(chain);
        if form.chain.params.is_empty() {
            self.palette = None;
            self.run_chain(&form);
        } else if let Some(state) = self.palette.as_mut() {
            state.form = Some(form);
        }
    }

    /// Once plugins are listed: chains named like a plugin are left out, with a toast.
    pub(super) fn drop_chains_named_like_plugins(&mut self, plugins: &[String]) {
        let (clash, keep): (Vec<Chain>, Vec<Chain>) =
            std::mem::take(&mut self.chains).into_iter().partition(|c| plugins.contains(&c.name));
        self.chains = keep;
        let msgs: Vec<String> =
            clash.iter().map(|c| format!("chain {}: named like the plugin {0}; left out", c.name)).collect();
        if !msgs.is_empty() {
            let mut all: Vec<String> = self.toast.take().into_iter().collect();
            all.extend(msgs);
            self.toast = Some(all.join("; "));
        }
    }
}
