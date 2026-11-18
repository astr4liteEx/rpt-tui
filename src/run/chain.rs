//! A chain's steps as one run: a small bash driver runs each step in `bash -o pipefail -c`,
//! printing a header between steps and stopping at the first failure it isn't told to allow.

use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::Launch;
use crate::config::Step;

/// The driver: `$1` is where the failing step's number is written (for two or more steps;
/// one step's failure is its exit code alone), then a line and an allow-failure flag per step.
const DRIVER: &str = r#"result=$1; shift; n=$(( $# / 2 )); i=0
while [ $# -gt 0 ]; do
  i=$((i + 1)); step=$1; allow=$2; shift 2
  [ "$n" -gt 1 ] && printf '\033[2m── step %d/%d: %s\033[0m\n' "$i" "$n" "$step"
  bash -o pipefail -c "$step"; code=$?
  if [ "$code" -ne 0 ]; then
    if [ "$allow" = 1 ]; then
      printf '\033[2m── step %d/%d failed (%d), continuing\033[0m\n' "$i" "$n" "$code"
    else
      [ "$n" -gt 1 ] && printf '%d' "$i" > "$result"; exit "$code"
    fi
  fi
done
exit 0"#;

/// A run's private folder: `RUN_TMP` for its steps, and the driver's notes. Deleted on drop.
#[derive(Debug)]
pub struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    /// A new folder under the system temp folder, readable by this user only.
    pub fn new() -> std::io::Result<Scratch> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("rpt-tui-{}-{n}", std::process::id()));
        std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
        std::fs::create_dir(dir.join("tmp"))?;
        Ok(Scratch { dir })
    }

    /// The steps' scratch folder, `$RUN_TMP`.
    pub fn run_tmp(&self) -> PathBuf {
        self.dir.join("tmp")
    }

    /// A file outside `RUN_TMP` for a run's own output, such as the file picker's choice.
    pub fn output(&self) -> PathBuf {
        self.dir.join("output")
    }

    /// The number (from 1) of the step a chain of two or more steps stopped at, once it failed.
    pub fn failed_step(&self) -> Option<usize> {
        std::fs::read_to_string(self.dir.join("failed-step")).ok()?.trim().parse().ok()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The launch running `steps` in `cwd` with `env` added and `RUN_TMP` set.
pub fn steps_launch(steps: &[Step], env: Vec<(String, String)>, cwd: &Path, scratch: &Scratch) -> Launch {
    let mut argv = vec!["bash".into(), "-c".into(), DRIVER.into(), "rpt-tui-chain".into()];
    argv.push(scratch.dir.join("failed-step").display().to_string());
    for s in steps {
        argv.push(s.run.clone());
        argv.push(if s.allow_failure { "1" } else { "0" }.into());
    }
    let mut launch = Launch::new(argv, cwd);
    launch.env = env;
    launch.env.push(("RUN_TMP".into(), scratch.run_tmp().display().to_string()));
    launch
}
