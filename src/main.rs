//! rpt-tui: `--describe`, finding the report and rpt, then the TUI.

use std::path::PathBuf;

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
    // `--dev-run[-raw] <command>` is a hidden, temporary launcher until the palette exists.
    let mut dev_run = None;
    match args.as_slice() {
        [] => {}
        [a] if a == "--describe" => {
            println!("{DESCRIBE}");
            return EXIT_OK;
        }
        [flag, command] if flag == "--dev-run" || flag == "--dev-run-raw" => {
            let display =
                if flag == "--dev-run" { ui::Display::Pane } else { ui::Display::Takeover(ui::TakeoverStyle::Raw) };
            dev_run = Some(ui::DevRun { command: command.clone(), display });
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
    ui::run(&startup.report, &startup.rpt, dev_run)
}

/// Finds the report root and the rpt binary, or says why not.
fn locate() -> Result<Startup, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("cannot read the current folder: {e}"))?;
    let report = rpt::find_report(&cwd, std::env::var_os("RPT_REPORT_DIR").as_deref())
        .ok_or("no report: run inside a folder with rpt.yaml, or set RPT_REPORT_DIR")?;
    let rpt = rpt::find_on_path(&std::env::var_os("PATH").unwrap_or_default())
        .ok_or("rpt not found on PATH: install rpt first")?;
    Ok(Startup { report, rpt })
}
