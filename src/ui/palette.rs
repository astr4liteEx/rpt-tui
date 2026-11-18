//! The command palette's entries and their order. Drawing is in `home`; keys are in `App`.

/// What kind of command an entry launches; groups appear in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Chain,
    Plugin,
    Rpt,
    Tui,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Chain, Kind::Plugin, Kind::Rpt, Kind::Tui];

    /// The tag shown on each row.
    pub fn tag(self) -> &'static str {
        match self {
            Kind::Chain => "chain",
            Kind::Plugin => "plugin",
            Kind::Rpt => "rpt",
            Kind::Tui => "tui",
        }
    }

    /// The group header.
    pub fn group(self) -> &'static str {
        match self {
            Kind::Chain => "chains",
            Kind::Plugin => "plugins",
            Kind::Rpt => "rpt",
            Kind::Tui => "TUI actions",
        }
    }
}

/// One command in the palette.
#[derive(Debug, Clone)]
pub struct Entry {
    pub kind: Kind,
    pub name: String,
    pub summary: String,
    /// A plugin's `input → output`, when it declares either.
    pub io: Option<String>,
    /// The key bound to it, shown on the right.
    pub key: Option<String>,
    /// Why it can't be chosen right now; such a row is greyed out.
    pub disabled: Option<String>,
    /// Takes a free-text argument line, pre-filled from this usage.
    pub args: Option<String>,
    /// The TUI's own `edit`, rather than a chain from the config.
    pub builtin: bool,
}

impl Entry {
    pub fn new(kind: Kind, name: &str, summary: &str) -> Entry {
        Entry {
            kind,
            name: name.into(),
            summary: summary.into(),
            io: None,
            key: None,
            disabled: None,
            args: None,
            builtin: false,
        }
    }

    /// How run history and recents name the command: `plugin parse-nuclei`.
    pub fn id(&self) -> String {
        format!("{} {}", self.kind.tag(), self.name)
    }
}

/// One line of the palette's list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Header(&'static str),
    /// Plugins are still being listed.
    Loading,
    /// An index into the entries.
    Entry(usize),
}

/// The palette's rows. An empty query gives `recent` commands (most recent first) under a
/// header, then each group in order. A typed query gives the matching entries ranked by
/// name contains it, name has its letters in order, summary contains it, kind starts with
/// it; ties go to the most recent, then to group order.
pub fn order(entries: &[Entry], query: &str, recent: &[String], loading_plugins: bool) -> Vec<Row> {
    let recency = |e: &Entry| recent.iter().position(|r| *r == e.id()).unwrap_or(usize::MAX);
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return grouped(entries, recency, loading_plugins);
    }
    let mut ranked: Vec<(u8, usize, Kind, usize)> =
        entries.iter().enumerate().filter_map(|(i, e)| tier(e, &q).map(|t| (t, recency(e), e.kind, i))).collect();
    ranked.sort();
    let mut rows: Vec<Row> = ranked.into_iter().map(|(.., i)| Row::Entry(i)).collect();
    if loading_plugins {
        rows.push(Row::Loading);
    }
    rows
}

fn grouped(entries: &[Entry], recency: impl Fn(&Entry) -> usize, loading: bool) -> Vec<Row> {
    let mut rows = vec![];
    let mut used: Vec<(usize, usize)> =
        entries.iter().enumerate().filter(|(_, e)| recency(e) != usize::MAX).map(|(i, e)| (recency(e), i)).collect();
    used.sort();
    if !used.is_empty() {
        rows.push(Row::Header("recent"));
        rows.extend(used.iter().map(|&(_, i)| Row::Entry(i)));
    }
    for kind in Kind::ALL {
        let members: Vec<Row> = entries
            .iter()
            .enumerate()
            .filter(|(i, e)| e.kind == kind && !used.iter().any(|(_, u)| u == i))
            .map(|(i, _)| Row::Entry(i))
            .collect();
        let loading_here = loading && kind == Kind::Plugin;
        if members.is_empty() && !loading_here {
            continue;
        }
        rows.push(Row::Header(kind.group()));
        rows.extend(members);
        if loading_here {
            rows.push(Row::Loading);
        }
    }
    rows
}

/// The rank of a match (lower is better), or `None` when the entry doesn't match.
fn tier(e: &Entry, q: &str) -> Option<u8> {
    let name = e.name.to_lowercase();
    if name.contains(q) {
        Some(1)
    } else if in_order(&name, q) {
        Some(2)
    } else if e.summary.to_lowercase().contains(q) {
        Some(3)
    } else if e.kind.tag().starts_with(q) {
        Some(4)
    } else {
        None
    }
}

/// True when every character of `q` appears in `s`, in order.
fn in_order(s: &str, q: &str) -> bool {
    let mut chars = s.chars();
    q.chars().all(|c| chars.any(|x| x == c))
}

/// The open palette: the query and highlighted row, or the argument line of a chosen entry.
#[derive(Debug, Default)]
pub struct State {
    pub query: String,
    /// The highlighted entry, counted among entry rows only.
    pub sel: usize,
    pub args: Option<ArgLine>,
    /// A chain's parameter form.
    pub form: Option<super::form::Form>,
    /// `export-transcript`'s path line.
    pub export: Option<ExportLine>,
}

/// Where to save a run's transcript, as typed.
#[derive(Debug)]
pub struct ExportLine {
    /// The run's id.
    pub run: u64,
    pub text: String,
}

/// The free-text argument line of a plugin or `set`/`delete`.
#[derive(Debug)]
pub struct ArgLine {
    pub entry: Entry,
    pub text: String,
    /// The usage, shown above the line when it couldn't pre-fill it.
    pub hint: Option<String>,
}

impl ArgLine {
    /// Pre-filled from the entry's usage with its leading `rpt <verb>` (or `rpt-<verb>`) taken
    /// off; a usage that starts any other way pre-fills nothing and becomes the hint.
    pub fn new(entry: Entry) -> ArgLine {
        let usage = entry.args.clone().unwrap_or_default();
        let usage = usage.trim();
        let rest = usage
            .strip_prefix(&format!("rpt {}", entry.name))
            .or_else(|| usage.strip_prefix(&format!("rpt-{}", entry.name)))
            .filter(|r| r.is_empty() || r.starts_with(' '));
        let (text, hint) = match rest {
            Some(r) => (r.trim().to_string(), None),
            None => (String::new(), (!usage.is_empty()).then(|| usage.to_string())),
        };
        ArgLine { entry, text, hint }
    }

    /// The shell line it runs: `rpt <verb> <text>`.
    pub fn command(&self) -> String {
        format!("rpt {} {}", self.entry.name, self.text).trim_end().to_string()
    }
}
