//! The built-in `edit` command: which editor, and how it is told the file and line.

use std::ffi::OsStr;
use std::path::Path;

/// Editors known to take `+LINE` before the file.
const TAKES_LINE: [&str; 8] = ["vi", "vim", "nvim", "nano", "micro", "emacs", "kak", "hx"];

/// Tried in order when neither `$VISUAL` nor `$EDITOR` is set.
const FALLBACKS: [&str; 4] = ["nvim", "vim", "vi", "nano"];

/// The argv that opens `file` (at `line`, when the editor takes it): `$VISUAL`, then
/// `$EDITOR` (either may carry arguments, split on whitespace), then the first of
/// nvim, vim, vi, nano on `path`. `None` when there is no editor.
pub fn editor_argv(
    visual: Option<&str>,
    editor: Option<&str>,
    path: &OsStr,
    file: &Path,
    line: Option<u32>,
) -> Option<Vec<String>> {
    let set = |v: Option<&str>| v.map(str::split_whitespace).map(|w| w.map(String::from).collect::<Vec<_>>());
    let mut argv =
        set(visual).filter(|w| !w.is_empty()).or_else(|| set(editor).filter(|w| !w.is_empty())).or_else(|| {
            let found = FALLBACKS.iter().find_map(|name| crate::rpt::which(path, name))?;
            Some(vec![found.display().to_string()])
        })?;
    let program = Path::new(&argv[0]).file_name().and_then(OsStr::to_str).unwrap_or_default();
    if let Some(line) = line.filter(|_| TAKES_LINE.contains(&program)) {
        argv.push(format!("+{line}"));
    }
    argv.push(file.display().to_string());
    Some(argv)
}
