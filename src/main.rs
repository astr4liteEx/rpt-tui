//! rpt-tui: `--describe`, finding the report and rpt, then the TUI.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rpt_tui::{rpt, ui};

const DESCRIBE: &str = concat!(
    r#"{"summary":"Terminal UI for a report: run rpt, plugins and chains","usage":"rpt tui","version":"v"#,
    env!("CARGO_PKG_VERSION"),
    r#""}"#
);

/// Exit codes, as in rpt: 0 success, 2 usage or environment problem.
const EXIT_OK: i32 = 0;
const EXIT_USAGE: i32 = 2;

/// Where the TUI runs: the report root and the rpt binary.
struct Startup {
    report: PathBuf,
    rpt: PathBuf,
}

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => {}
        [a] if a == "--describe" => {
            println!("{DESCRIBE}");
            return EXIT_OK;
        }
        _ => {
            eprintln!("usage: rpt-tui [--describe]   (run inside a report folder)");
            return EXIT_USAGE;
        }
    }
    let startup = match locate() {
        Ok(s) => s,
        Err(msg) => {
            eprintln!("rpt-tui: {msg}");
            return EXIT_USAGE;
        }
    };
    ui::run(&startup.report, &startup.rpt)
}

/// Finds the report root and the rpt binary, or says why not.
fn locate() -> Result<Startup, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("cannot read the current folder: {e}"))?;
    let rpt = rpt::find_on_path(&std::env::var_os("PATH").unwrap_or_default());
    let Some(report) = rpt::find_report(&cwd, std::env::var_os("RPT_REPORT_DIR").as_deref()) else {
        return Err(no_report(rpt.as_deref(), &cwd));
    };
    let rpt = rpt.ok_or("rpt not found on PATH: install rpt first")?;
    Ok(Startup { report, rpt })
}

/// How long `rpt ls` may search for reports under the current folder.
const LS_TIMEOUT: Duration = Duration::from_secs(2);
/// How many of the reports `rpt ls` finds the no-report message lists.
const LS_SHOWN: usize = 10;

/// Outside a report: the reports `rpt ls` finds under `cwd`, each with its absolute path to
/// paste after `cd` or into `RPT_REPORT_DIR`. Without rpt, or when it finds none, fails or
/// takes too long, just how to get into a report.
fn no_report(rpt: Option<&Path>, cwd: &Path) -> String {
    let found = rpt.and_then(|bin| rpt::list_reports(bin, cwd, LS_TIMEOUT).ok()).unwrap_or_default();
    if found.is_empty() {
        return "no report: run inside a folder with rpt.yaml, set RPT_REPORT_DIR, or create one with `rpt new <dir>`"
            .into();
    }
    let width = found.iter().take(LS_SHOWN).map(|l| l.path.display().to_string().chars().count()).max().unwrap_or(0);
    let mut msg = String::from("no report here. Reports under this folder:\n");
    for l in found.iter().take(LS_SHOWN) {
        let count = match l.findings {
            Some(1) => "1 finding".to_string(),
            Some(n) => format!("{n} findings"),
            None => "rpt.yaml doesn't parse".to_string(),
        };
        msg += &format!("  {:<width$}   {count}\n", l.path.display().to_string());
    }
    if found.len() > LS_SHOWN {
        msg += &format!("  … and {} more (rpt ls lists them all)\n", found.len() - LS_SHOWN);
    }
    msg + "Run rpt tui inside one, set RPT_REPORT_DIR to one, or create one with `rpt new <dir>`."
}
