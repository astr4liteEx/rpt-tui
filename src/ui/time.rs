//! Times as the Runs panel shows them, in local time.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The local time at `t`.
fn local(t: SystemTime) -> Option<libc::tm> {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as libc::time_t;
    // SAFETY: localtime_r only writes the tm it is given.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    (!unsafe { libc::localtime_r(&secs, &mut tm) }.is_null()).then_some(tm)
}

/// When a run started, as the local clock read it: `14:03:22` today, `11-17 14:03` another day.
pub fn when(t: SystemTime, now: SystemTime) -> String {
    match (local(t), local(now)) {
        (Some(a), Some(b)) if (a.tm_year, a.tm_yday) == (b.tm_year, b.tm_yday) => {
            format!("{:02}:{:02}:{:02}", a.tm_hour, a.tm_min, a.tm_sec)
        }
        (Some(a), _) => format!("{:02}-{:02} {:02}:{:02}", a.tm_mon + 1, a.tm_mday, a.tm_hour, a.tm_min),
        (None, _) => "--:--:--".into(),
    }
}

/// A file-name time stamp: `20261118-140322`.
pub fn stamp(t: SystemTime) -> String {
    local(t).map_or_else(String::new, |a| {
        format!(
            "{}{:02}{:02}-{:02}{:02}{:02}",
            a.tm_year + 1900,
            a.tm_mon + 1,
            a.tm_mday,
            a.tm_hour,
            a.tm_min,
            a.tm_sec
        )
    })
}

/// How long a run took: `2s`, `3m04s`, `1h02m`.
pub fn took(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m{:02}s", s / 60, s % 60),
        _ => format!("{}h{:02}m", s / 3600, s % 3600 / 60),
    }
}

/// How long ago something was: `just now`, `3m ago`, `2h ago`, `4d ago`.
pub fn ago(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..60 => "just now".into(),
        60..3600 => format!("{}m ago", s / 60),
        3600..86400 => format!("{}h ago", s / 3600),
        _ => format!("{}d ago", s / 86400),
    }
}
