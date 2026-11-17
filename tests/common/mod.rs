//! Shared test helpers: the real rpt from ../rpt/dist and a writable copy of rpt's fixture report.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

const RPT_SOURCE: &str = "../rpt";
const FIXTURE: &str = "../rpt/testdata/fixture";

/// Folder holding an `rpt` built from ../rpt's source (once per test run): the dist
/// archive predates `rpt severities`.
pub fn rpt_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("rpt-src");
        std::fs::create_dir_all(&dir).unwrap();
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join(RPT_SOURCE);
        let ok = Command::new("go")
            .args(["build", "-trimpath", "-o"])
            .arg(dir.join("rpt"))
            .arg("./cmd/rpt")
            .current_dir(&src)
            .env("CGO_ENABLED", "0")
            .status()
            .expect("tests need Go to build rpt from ../rpt");
        assert!(ok.success(), "could not build rpt from {}", src.display());
        dir
    })
}

pub fn rpt_bin() -> PathBuf {
    rpt_dir().join("rpt")
}

/// A fresh, writable copy of rpt's fixture report.
pub fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE), tmp.path());
    tmp
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dst = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dst);
        } else {
            std::fs::copy(entry.path(), dst).unwrap();
        }
    }
}

/// Writes an executable shell script standing in for rpt into `dir` and returns its path.
pub fn stand_in_rpt(dir: &Path, name: &str, script: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{script}")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// One row of a ratatui buffer as text, the way a terminal shows it: the cell after a
/// wide character is hidden behind it, so it is skipped.
pub fn row_text(buf: &ratatui::buffer::Buffer, y: u16) -> String {
    let mut out = String::new();
    let mut x = 0;
    while x < buf.area.width {
        let symbol = buf[(x, y)].symbol();
        out.push_str(symbol);
        x += unicode_width::UnicodeWidthStr::width(symbol).max(1) as u16;
    }
    out.trim_end().to_string()
}
