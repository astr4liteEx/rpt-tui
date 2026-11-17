//! Layout A: Findings, Problems and Runs stacked on the left; the right side follows focus.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap},
};

use super::{App, Focus, Mark, term::TermView, theme::Theme};
use crate::rpt::{Finding, Problem};

/// Draws the home screen; returns the inside of the right side, where a run's pane goes.
pub fn draw(f: &mut Frame, app: &App) -> Option<Rect> {
    let t = app.theme;
    f.render_widget(Block::new().style(Style::new().bg(t.bg).fg(t.text)), f.area());

    let [header, body, footer] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).areas(f.area());
    draw_header(f, header, app);

    let [left, right] = Layout::horizontal([Constraint::Percentage(42), Constraint::Min(0)]).areas(body);
    let problems_height = (app.view.problems.len() as u16 + 2).clamp(3, 7);
    let runs_height = (app.runs.entries.len() as u16 + 2).clamp(5, 8);
    let [lf, lp, lr] =
        Layout::vertical([Constraint::Min(5), Constraint::Length(problems_height), Constraint::Length(runs_height)])
            .areas(left);
    draw_findings(f, lf, app);
    draw_problems(f, lp, app);
    draw_runs(f, lr, app);

    match app.focus {
        Focus::Findings => draw_finding_preview(f, right, app),
        Focus::Problems => draw_problem(f, right, app),
        Focus::Runs => draw_pane(f, right, app),
    }

    let hints = if app.runs.key_target().is_some() {
        " keys go to the run · ctrl-g leave · ctrl-t takeover"
    } else {
        match app.focus {
            Focus::Findings | Focus::Problems => " j/k move · g/G top/bottom · tab/1-3 focus · q quit",
            Focus::Runs => " enter keys · f takeover · x kill · j/k move · tab/1-3 focus · q quit",
        }
    };
    f.render_widget(Line::from(Span::styled(hints, Style::new().fg(t.muted))), footer);

    if let Some(msg) = &app.toast {
        draw_toast(f, &t, msg);
    }
    Some(Block::bordered().inner(right))
}

/// The selected run's screen on the right side, bordered in `border_run` while it has the keys.
fn draw_pane(f: &mut Frame, area: Rect, app: &App) {
    let t = app.theme;
    let Some(entry) = app.runs.selected() else {
        return draw_placeholder(f, area, &t, "Run", "No run selected.");
    };
    let has_keys = app.runs.key_target() == Some(app.runs.sel);
    let colour = if has_keys { t.border_run } else { t.border };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(colour))
        .title(Line::from(Span::styled(format!(" {} ", entry.label), Style::new().fg(t.text))))
        .title(
            Line::from(Span::styled(format!(" {} ", Mark::of(&entry.run).text()), Style::new().fg(t.muted)))
                .right_aligned(),
        );
    let inner = block.inner(area);
    f.render_widget(block, area);
    draw_screen(f, inner, app, &entry.run, has_keys);
}

/// A run's screen in `area`, with the cursor shown when the run has the keys.
fn draw_screen(f: &mut Frame, area: Rect, app: &App, run: &crate::run::Run, has_keys: bool) {
    let t = app.theme;
    let cursor = run.with_screen(|screen| {
        f.render_widget(TermView { screen, fg: t.text, bg: t.bg }, area);
        let (row, col) = screen.cursor_position();
        (!screen.hide_cursor()).then_some((area.x + col, area.y + row))
    });
    if has_keys && let Some((x, y)) = cursor.filter(|&(x, y)| x < area.right() && y < area.bottom()) {
        f.set_cursor_position((x, y));
    }
}

/// Emulated takeover: the run's screen full-size, with one status line at the bottom.
pub fn draw_takeover(f: &mut Frame, app: &App, i: usize) {
    let t = app.theme;
    let Some(entry) = app.runs.entries.get(i) else { return };
    let [screen, status] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());
    f.render_widget(Block::new().style(Style::new().bg(t.bg).fg(t.text)), f.area());
    draw_screen(f, screen, app, &entry.run, true);
    let line = Line::from(vec![
        Span::styled(format!(" {} ", entry.label), Style::new().fg(t.bg).bg(t.border_run).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {} ", Mark::of(&entry.run).text()), Style::new().fg(t.muted)),
        Span::styled(" ctrl-t pane · ctrl-g leave", Style::new().fg(t.muted)),
    ]);
    f.render_widget(line.style(Style::new().bg(t.surface)), status);
}

fn panel<'a>(t: &Theme, title: &'a str, focused: bool) -> Block<'a> {
    let colour = if focused { t.border_focus } else { t.border };
    let title_style =
        if focused { Style::new().fg(t.accent).add_modifier(Modifier::BOLD) } else { Style::new().fg(t.muted) };
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(colour))
        .title(Line::from(Span::styled(format!(" {title} "), title_style)))
        .padding(Padding::horizontal(1))
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let t = app.theme;
    let errors = app.view.problems.iter().filter(|p| p.is_error()).count();
    let warnings = app.view.problems.len() - errors;
    let mut spans = vec![
        Span::styled(" rpt-tui ", Style::new().fg(t.bg).bg(t.accent).add_modifier(Modifier::BOLD)),
        Span::styled(format!("  {}   ", app.view.root.display()), Style::new().fg(t.muted)),
        Span::styled(format!("{} findings  ", app.view.findings.len()), Style::new().fg(t.text)),
        Span::styled(
            format!("{errors} {}  ", plural(errors, "error")),
            Style::new().fg(if errors > 0 { t.err } else { t.muted }),
        ),
        Span::styled(
            format!("{warnings} {}", plural(warnings, "warning")),
            Style::new().fg(if warnings > 0 { t.warn } else { t.muted }),
        ),
    ];
    if let Some(banner) = app.view.banner() {
        spans.push(Span::styled(format!("   ▲ {banner}"), Style::new().fg(t.warn)));
    }
    f.render_widget(Line::from(spans), area);
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 { word.to_string() } else { format!("{word}s") }
}

fn draw_findings(f: &mut Frame, area: Rect, app: &App) {
    let t = app.theme;
    let width = app.view.findings.iter().map(|x| x.severity.chars().count()).max().unwrap_or(0);
    let items: Vec<ListItem> = app
        .view
        .findings
        .iter()
        .map(|x| {
            let sev = t.severity(&x.severity, app.view.severity_position(&x.severity));
            ListItem::new(Line::from(vec![
                Span::styled(format!("{} ", x.id), Style::new().fg(t.muted)),
                Span::styled(format!("● {:<width$} ", x.severity), Style::new().fg(sev)),
                Span::styled(x.title.clone(), Style::new().fg(t.text)),
            ]))
        })
        .collect();
    let counts: Vec<Span> = app
        .view
        .severity_counts()
        .iter()
        .map(|(s, n)| Span::styled(format!(" {n} {s} "), Style::new().fg(t.severity(s, app.view.severity_position(s)))))
        .collect();
    let block = panel(&t, "1 Findings", app.focus == Focus::Findings).title_bottom(Line::from(counts));
    let list = List::new(items).block(block).highlight_style(Style::new().bg(t.surface)).highlight_symbol("▌");
    let mut state = ListState::default().with_selected((!app.view.findings.is_empty()).then_some(app.findings_sel));
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_problems(f: &mut Frame, area: Rect, app: &App) {
    let t = app.theme;
    // Inside: borders (2), padding (2) and the highlight mark (1).
    let inner = (area.width as usize).saturating_sub(5);
    let items: Vec<ListItem> = app
        .view
        .problems
        .iter()
        .map(|p| {
            let (mark, colour) = level_mark(&t, p);
            let loc = location(p);
            // The file:line always shows; the message gives way.
            let room = inner.saturating_sub(mark.chars().count() + 1 + 2 + loc.chars().count());
            ListItem::new(Line::from(vec![
                Span::styled(format!("{mark} "), Style::new().fg(colour)),
                Span::styled(truncate(&p.message, room), Style::new().fg(t.text)),
                Span::styled(format!("  {loc}"), Style::new().fg(t.muted)),
            ]))
        })
        .collect();
    let block = panel(&t, "2 Problems", app.focus == Focus::Problems);
    if items.is_empty() {
        f.render_widget(Paragraph::new(Span::styled("No problems.", Style::new().fg(t.ok))).block(block), area);
        return;
    }
    let list = List::new(items).block(block).highlight_style(Style::new().bg(t.surface)).highlight_symbol("▌");
    let mut state = ListState::default().with_selected(Some(app.problems_sel));
    f.render_stateful_widget(list, area, &mut state);
}

/// The problem's level as a mark and word (`✘ error`, `▲ warning`) and its colour.
fn level_mark(t: &Theme, p: &Problem) -> (String, ratatui::style::Color) {
    if p.is_error() { ("✘ error".into(), t.err) } else { (format!("▲ {}", p.level), t.warn) }
}

/// `s` cut to `max` characters, ending in `…` when cut.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn location(p: &Problem) -> String {
    match p.line {
        Some(line) => format!("{}:{line}", p.path),
        None => p.path.clone(),
    }
}

fn draw_runs(f: &mut Frame, area: Rect, app: &App) {
    let t = app.theme;
    let block = panel(&t, "3 Runs", app.focus == Focus::Runs);
    if app.runs.entries.is_empty() {
        f.render_widget(Paragraph::new(Span::styled("No runs yet.", Style::new().fg(t.muted))).block(block), area);
        return;
    }
    let items: Vec<ListItem> = app
        .runs
        .entries
        .iter()
        .map(|e| {
            let mark = Mark::of(&e.run);
            let colour = mark.colour(&t);
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<12}", mark.text()), Style::new().fg(colour)),
                Span::styled(e.label.clone(), Style::new().fg(t.text)),
            ]))
        })
        .collect();
    let list = List::new(items).block(block).highlight_style(Style::new().bg(t.surface)).highlight_symbol("▌");
    let mut state = ListState::default().with_selected(Some(app.runs.sel));
    f.render_stateful_widget(list, area, &mut state);
}

/// Body sections in reading order: the usual three first, then any others by name.
fn ordered_sections(x: &Finding) -> Vec<(&str, &str)> {
    let known = ["description", "remediation", "poc"];
    let mut out: Vec<(&str, &str)> =
        known.iter().filter_map(|k| x.sections.get_key_value(*k)).map(|(k, v)| (k.as_str(), v.as_str())).collect();
    out.extend(x.sections.iter().filter(|(k, _)| !known.contains(&k.as_str())).map(|(k, v)| (k.as_str(), v.as_str())));
    out
}

fn section_title(key: &str) -> String {
    match key {
        "description" => "Description".into(),
        "remediation" => "Remediation".into(),
        "poc" => "Proof of concept".into(),
        other => other.to_string(),
    }
}

fn draw_finding_preview(f: &mut Frame, area: Rect, app: &App) {
    let t = app.theme;
    let Some(x) = app.selected_finding() else {
        return draw_placeholder(f, area, &t, "Preview", "No findings yet.");
    };
    let sev = t.severity(&x.severity, app.view.severity_position(&x.severity));
    let mut meta = vec![Span::styled(format!("● {}", x.severity), Style::new().fg(sev))];
    if !x.scope.is_empty() {
        meta.push(Span::styled(format!("   scope {}", x.scope.join(", ")), Style::new().fg(t.muted)));
    }
    meta.push(Span::styled(format!("   evidence {}", x.evidence.len()), Style::new().fg(t.muted)));
    let mut lines = vec![
        Line::from(vec![
            Span::styled(format!("{}  ", x.id), Style::new().fg(t.muted)),
            Span::styled(x.title.clone(), Style::new().fg(t.text).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(meta),
        Line::from(Span::styled(x.path.clone(), Style::new().fg(t.muted))),
    ];
    for (key, body) in ordered_sections(x) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("## {}", section_title(key)),
            Style::new().fg(t.accent).add_modifier(Modifier::BOLD),
        )));
        lines.extend(body.lines().map(|l| Line::from(Span::styled(l.to_string(), Style::new().fg(t.text)))));
    }
    let block = panel(&t, "Preview", false);
    f.render_widget(Paragraph::new(Text::from(lines)).block(block).wrap(Wrap { trim: false }), area);
}

fn draw_problem(f: &mut Frame, area: Rect, app: &App) {
    let t = app.theme;
    let Some(p) = app.selected_problem() else {
        return draw_placeholder(f, area, &t, "Problem", "No problems.");
    };
    let level = if p.is_error() {
        Span::styled("✘ error", Style::new().fg(t.err))
    } else {
        Span::styled("▲ warning", Style::new().fg(t.warn))
    };
    let lines = vec![
        Line::from(vec![
            level,
            Span::styled(format!("   {}", p.rule), Style::new().fg(t.accent).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(Span::styled(location(p), Style::new().fg(t.muted))),
        Line::from(""),
        Line::from(Span::styled(p.message.clone(), Style::new().fg(t.text))),
    ];
    let block = panel(&t, "Problem", false);
    f.render_widget(Paragraph::new(Text::from(lines)).block(block).wrap(Wrap { trim: false }), area);
}

fn draw_placeholder(f: &mut Frame, area: Rect, t: &Theme, title: &str, text: &str) {
    let block = panel(t, title, false);
    f.render_widget(Paragraph::new(Span::styled(text.to_string(), Style::new().fg(t.muted))).block(block), area);
}

fn draw_toast(f: &mut Frame, t: &Theme, msg: &str) {
    let area = f.area();
    let width = (msg.chars().count() as u16 + 4).min(area.width.saturating_sub(2));
    let height = 3;
    let rect = Rect { x: area.right().saturating_sub(width + 1), y: area.y + 1, width, height };
    f.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(t.err))
        .style(Style::new().bg(t.surface));
    f.render_widget(Paragraph::new(msg.to_string()).block(block), rect);
}
