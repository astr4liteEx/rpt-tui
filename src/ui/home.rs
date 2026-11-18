//! Layout A: Findings, Problems and Runs stacked on the left; the right side follows focus.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap},
};

use super::{App, Focus, Mark, form::Form, palette, plural, term::TermView, theme::Theme};
use crate::config::ParamKind;
use crate::keys::Action;
use crate::rpt::{Finding, Problem};

/// Draws the home screen; returns the inside of the right side, where a run's pane goes.
pub fn draw(f: &mut Frame, app: &App) -> Option<Rect> {
    let t = &app.theme;
    f.render_widget(Block::new().style(Style::new().bg(t.bg).fg(t.text)), f.area());

    let [header, body, footer] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).areas(f.area());
    draw_header(f, header, app);

    let full = app.full;
    let right = if full {
        body
    } else {
        let [left, right] = Layout::horizontal([Constraint::Percentage(42), Constraint::Min(0)]).areas(body);
        let problems_height = (app.view.problems.len() as u16 + 2).clamp(3, 7);
        let runs_height = (app.runs.entries.len() as u16 + 2).clamp(5, 8);
        let [lf, lp, lr] = Layout::vertical([
            Constraint::Min(5),
            Constraint::Length(problems_height),
            Constraint::Length(runs_height),
        ])
        .areas(left);
        draw_findings(f, lf, app);
        draw_problems(f, lp, app);
        draw_runs(f, lr, app);
        right
    };

    match app.focus {
        Focus::Findings => draw_finding_preview(f, right, app),
        Focus::Problems => draw_problem(f, right, app),
        Focus::Runs if full => draw_transcript(f, right, app),
        Focus::Runs => draw_pane(f, right, app),
    }

    f.render_widget(Line::from(Span::styled(hints(app, full), Style::new().fg(t.muted))), footer);

    if let Some(state) = app.palette.as_ref().filter(|_| app.picking.is_none()) {
        draw_palette(f, app, state);
    }
    if let Some(lp) = &app.list_picker {
        draw_list_picker(f, t, lp);
    }
    if let Some(at) = app.help {
        draw_help(f, app, at);
    }
    if let Some(p) = &app.theme_picker {
        draw_theme_picker(f, app, p);
    }
    if let (Some(g), Some((trouble, left))) = (&app.guide, app.guide_trouble()) {
        draw_guide(f, t, g, &trouble, left);
    }
    if let Some(c) = &app.confirm {
        let msg = app.question(c);
        let rect = centred(f.area(), msg.chars().count() as u16 + 4, 3);
        f.render_widget(Clear, rect);
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(t.warn))
            .style(Style::new().bg(t.surface));
        f.render_widget(Paragraph::new(msg).block(block), rect);
    }
    draw_toasts(f, t, &app.toasts());
    // Full width, the right side is no run's pane: live runs keep the pane's size.
    (!full).then(|| Block::bordered().inner(right))
}

/// The footer's key hints, from the live bindings; an unbound action's hint is left out.
fn hints(app: &App, full: bool) -> String {
    let b = &app.bindings;
    let key = |a: Action| b.keys(a).first().map(|k| k.to_string());
    let hint = |a: Action, what: &str| key(a).map(|k| format!("{k} {what}"));
    let pair = |x: Action, y: Action, what: &str| match (key(x), key(y)) {
        (Some(x), Some(y)) => Some(format!("{x}/{y} {what}")),
        (x, y) => x.or(y).map(|k| format!("{k} {what}")),
    };
    let parts: Vec<Option<String>> = if app.runs.key_target().is_some() {
        vec![
            Some("keys go to the run".into()),
            hint(Action::LeaveRun, "leave"),
            hint(Action::ToggleTakeover, "takeover"),
        ]
    } else if full {
        vec![
            pair(Action::Down, Action::Up, "scroll"),
            hint(Action::Edit, "edit"),
            hint(Action::Back, "back"),
            hint(Action::Help, "help"),
            hint(Action::Quit, "quit"),
        ]
    } else {
        let mut v = vec![hint(Action::Palette, "commands")];
        match app.focus {
            Focus::Findings | Focus::Problems => {
                v.extend([hint(Action::Edit, "edit"), hint(Action::Open, "open"), hint(Action::Validate, "validate")])
            }
            Focus::Runs if app.runs.selected().is_none_or(|e| e.is_live()) => v.extend([
                hint(Action::Open, "keys"),
                hint(Action::Takeover, "takeover"),
                hint(Action::KillOrDelete, "kill"),
            ]),
            Focus::Runs => v.extend([
                hint(Action::Open, "open"),
                hint(Action::Rerun, "rerun"),
                hint(Action::KillOrDelete, "delete"),
            ]),
        }
        v.extend([
            pair(Action::Down, Action::Up, "move"),
            hint(Action::FocusNext, "focus"),
            hint(Action::Help, "help"),
            hint(Action::Quit, "quit"),
        ]);
        v
    };
    format!(" {}", parts.into_iter().flatten().collect::<Vec<_>>().join(" · "))
}

/// The theme picker: the themes, the previewed one highlighted.
fn draw_theme_picker(f: &mut Frame, app: &App, p: &super::theme_picker::ThemePicker) {
    let t = &app.theme;
    let rect = centred(f.area(), 44, p.names.len() as u16 + 2);
    f.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(t.border_focus))
        .title(Line::from(Span::styled(" theme ", Style::new().fg(t.accent).add_modifier(Modifier::BOLD))))
        .title_bottom(Line::from(Span::styled(" j/k preview · enter keep · esc back ", Style::new().fg(t.muted))))
        .padding(Padding::horizontal(1))
        .style(Style::new().bg(t.surface).fg(t.text));
    let items: Vec<ListItem> = p.names.iter().map(|n| ListItem::new(n.clone())).collect();
    let list = List::new(items).block(block).highlight_style(Style::new().bg(t.bg)).highlight_symbol("▌");
    f.render_stateful_widget(list, rect, &mut ListState::default().with_selected(Some(p.sel)));
}

/// Help: the live bindings by context, then chain hotkeys; `*` marks a changed binding.
fn draw_help(f: &mut Frame, app: &App, at: u16) {
    let t = &app.theme;
    let b = &app.bindings;
    let row = |mark: &str, name: &str, keys: String, what: &str| {
        Line::from(vec![
            Span::styled(format!(" {mark:1}"), Style::new().fg(t.warn)),
            Span::styled(format!("{name:<18} "), Style::new().fg(t.text)),
            Span::styled(format!("{keys:<16} "), Style::new().fg(t.warn)),
            Span::styled(what.to_string(), Style::new().fg(t.muted)),
        ])
    };
    let heading =
        |s: &str| Line::from(Span::styled(s.to_string(), Style::new().fg(t.accent).add_modifier(Modifier::BOLD)));
    let mut lines = vec![];
    for scope in ["global", "lists", "findings, problems", "runs", "run"] {
        let actions: Vec<&Action> = Action::ALL.iter().filter(|a| a.works() && a.scope().label() == scope).collect();
        if actions.is_empty() {
            continue;
        }
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        lines.push(heading(scope));
        for a in actions {
            let keys = match b.written(*a) {
                k if k.is_empty() => "unbound".to_string(),
                k => k,
            };
            lines.push(row(if b.changed(*a) { "*" } else { "" }, a.name(), keys, a.summary()));
        }
    }
    let hotkeys: Vec<Line> = app
        .chains
        .iter()
        .filter_map(|c| b.chain_key(&c.name).map(|k| row("", &c.name, k.to_string(), &c.description)))
        .collect();
    if !hotkeys.is_empty() {
        lines.push(Line::from(""));
        lines.push(heading("chain hotkeys"));
        lines.extend(hotkeys);
    }
    let area = centred(f.area(), 84, f.area().height);
    f.render_widget(Clear, area);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(t.border_focus))
        .title(Line::from(Span::styled(" Keys ", Style::new().fg(t.accent).add_modifier(Modifier::BOLD))))
        .title_bottom(Line::from(Span::styled(
            " * changed from the default · j/k scroll · esc close ",
            Style::new().fg(t.muted),
        )))
        .padding(Padding::horizontal(1))
        .style(Style::new().bg(t.surface));
    let inner = block.inner(area);
    let max = (lines.len() as u16).saturating_sub(inner.height);
    app.help_max.set(max);
    let at = at.min(max);
    f.render_widget(block, area);
    f.render_widget(Paragraph::new(Text::from(lines)).scroll((at, 0)), inner);
}

/// A finding or problem's text in its panel; full width, it scrolls.
fn draw_view(f: &mut Frame, area: Rect, app: &App, lines: Vec<Line>, title: &str) {
    let t = &app.theme;
    let mut para = Paragraph::new(Text::from(lines)).block(panel(t, title, app.full)).wrap(Wrap { trim: false });
    if app.full {
        let max = (para.line_count(area.width) as u16).saturating_sub(area.height);
        app.scroll_max.set(max);
        para = para.scroll((app.scroll.min(max), 0));
    }
    f.render_widget(para, area);
}

/// The selected run's screen on the right side, bordered in `border_run` while it has the keys.
fn draw_pane(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let Some(entry) = app.runs.selected() else {
        return draw_placeholder(f, area, t, "Run", "No run selected.");
    };
    let has_keys = app.runs.key_target() == Some(app.runs.sel);
    let block = run_block(app, entry, if has_keys { t.border_run } else { t.border });
    let inner = block.inner(area);
    f.render_widget(block, area);
    match &entry.run {
        Some(run) => run.with_screen(|screen| draw_screen(f, inner, app, screen, has_keys)),
        // Read back from history: the end of its transcript.
        None => app.with_replay(app.runs.sel, inner, false, |p| draw_screen(f, inner, app, p.screen(), false)),
    }
}

/// A run's border: its label, its mark, and when it started, how long it took and how long ago.
fn run_block<'a>(app: &App, entry: &super::runs::RunEntry, colour: ratatui::style::Color) -> Block<'a> {
    let t = &app.theme;
    let now = std::time::SystemTime::now();
    let mut about = format!("started {}", crate::time::when(entry.started, now));
    if let Some((_, took)) = &entry.ended {
        let since = now.duration_since(entry.started + *took).unwrap_or_default();
        about.push_str(&format!(" · {} · {}", crate::time::took(*took), crate::time::ago(since)));
    }
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(colour))
        .title(Line::from(Span::styled(format!(" {} ", entry.label), Style::new().fg(t.text))))
        .title(
            Line::from(Span::styled(format!(" {} ", Mark::of(entry).text()), Style::new().fg(t.muted))).right_aligned(),
        )
        .title_bottom(Line::from(Span::styled(format!(" {about} "), Style::new().fg(t.muted))))
}

/// A finished run's transcript, full width; it scrolls.
fn draw_transcript(f: &mut Frame, area: Rect, app: &App) {
    let Some(entry) = app.runs.selected() else { return };
    let block = run_block(app, entry, app.theme.border_focus);
    let inner = block.inner(area);
    f.render_widget(block, area);
    app.with_replay(app.runs.sel, inner, true, |p| {
        p.screen_mut().set_scrollback(usize::MAX);
        let max = p.screen().scrollback();
        app.scroll_max.set(max.min(u16::MAX as usize) as u16);
        p.screen_mut().set_scrollback(max - (app.scroll as usize).min(max));
        draw_screen(f, inner, app, p.screen(), false);
    });
}

/// A run's screen in `area`, with the cursor shown when the run has the keys.
fn draw_screen(f: &mut Frame, area: Rect, app: &App, screen: &vt100::Screen, has_keys: bool) {
    let t = &app.theme;
    f.render_widget(TermView { screen, fg: t.text, bg: t.bg }, area);
    let (row, col) = screen.cursor_position();
    let cursor = (!screen.hide_cursor()).then_some((area.x + col, area.y + row));
    if has_keys && let Some((x, y)) = cursor.filter(|&(x, y)| x < area.right() && y < area.bottom()) {
        f.set_cursor_position((x, y));
    }
}

/// Emulated takeover: the run's screen full-size, with one status line at the bottom.
pub fn draw_takeover(f: &mut Frame, app: &App, i: usize) {
    let t = &app.theme;
    let Some(entry) = app.runs.entries.get(i) else { return };
    let [screen, status] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());
    f.render_widget(Block::new().style(Style::new().bg(t.bg).fg(t.text)), f.area());
    if let Some(run) = &entry.run {
        run.with_screen(|s| draw_screen(f, screen, app, s, true));
    }
    let line = Line::from(vec![
        Span::styled(format!(" {} ", entry.label), Style::new().fg(t.bg).bg(t.border_run).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {} ", Mark::of(entry).text()), Style::new().fg(t.muted)),
        Span::styled(
            format!(
                " {} pane · {} leave",
                app.bindings.written(Action::ToggleTakeover),
                app.bindings.written(Action::LeaveRun)
            ),
            Style::new().fg(t.muted),
        ),
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
    let t = &app.theme;
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

fn draw_findings(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
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
    let block = panel(t, "1 Findings", app.focus == Focus::Findings).title_bottom(Line::from(counts));
    let list = List::new(items).block(block).highlight_style(Style::new().bg(t.surface)).highlight_symbol("▌");
    let mut state = ListState::default().with_selected((!app.view.findings.is_empty()).then_some(app.findings_sel));
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_problems(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    // Inside: borders (2), padding (2) and the highlight mark (1).
    let inner = (area.width as usize).saturating_sub(5);
    let items: Vec<ListItem> = app
        .view
        .problems
        .iter()
        .map(|p| {
            let (mark, colour) = level_mark(t, p);
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
    let block = panel(t, "2 Problems", app.focus == Focus::Problems);
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
    let t = &app.theme;
    let title = app.runs_title();
    let block = panel(t, &title, app.focus == Focus::Runs);
    if app.runs.entries.is_empty() {
        f.render_widget(Paragraph::new(Span::styled("No runs yet.", Style::new().fg(t.muted))).block(block), area);
        return;
    }
    // Inside: borders (2), padding (2) and the highlight mark (1).
    let width = (area.width as usize).saturating_sub(5);
    let now = std::time::SystemTime::now();
    let order = app.runs.order();
    // Marks line up in a column as wide as the widest shown.
    let marks: Vec<Mark> = order.iter().map(|&i| Mark::of(&app.runs.entries[i])).collect();
    let mark_width = marks.iter().map(|m| m.text().chars().count()).max().unwrap_or(0) + 2;
    // A column for "new" dots while any run has one.
    let dots = app.runs.entries.iter().any(|e| e.unseen.is_some());
    let width = if dots { width.saturating_sub(2) } else { width };
    let items: Vec<ListItem> = order
        .iter()
        .zip(marks)
        .map(|(&i, mark)| {
            let e = &app.runs.entries[i];
            let when = format!("{} ", crate::time::when(e.started, now));
            let took = e.ended.as_ref().map(|(_, d)| format!(" {}", crate::time::took(*d))).unwrap_or_default();
            let room = width.saturating_sub(mark_width + when.chars().count() + took.chars().count());
            let label = fit(&e.label, room);
            let dim = if e.earlier { Modifier::DIM } else { Modifier::empty() };
            let label_style = if e.ephemeral {
                Style::new().fg(t.muted).add_modifier(Modifier::ITALIC)
            } else {
                Style::new().fg(t.text)
            };
            let dot = match e.unseen {
                Some(failed) => Span::styled("● ", Style::new().fg(if failed { t.err } else { t.accent })),
                None => Span::raw(if dots { "  " } else { "" }),
            };
            ListItem::new(Line::from(vec![
                dot,
                Span::styled(
                    format!("{:<mark_width$}", mark.text()),
                    Style::new().fg(mark.colour(t)).add_modifier(dim),
                ),
                Span::styled(when, Style::new().fg(t.muted).add_modifier(dim)),
                Span::styled(format!("{label:<room$}"), label_style.add_modifier(dim)),
                Span::styled(took, Style::new().fg(t.muted).add_modifier(dim)),
            ]))
        })
        .collect();
    let list = List::new(items).block(block).highlight_style(Style::new().bg(t.surface)).highlight_symbol("▌");
    let mut state = ListState::default().with_selected(order.iter().position(|&i| i == app.runs.sel));
    f.render_stateful_widget(list, area, &mut state);
}

/// `text` cut to `width` characters, ending in `…` when cut.
fn fit(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
    if width > 0 {
        out.push('…');
    }
    out
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
    let t = &app.theme;
    let Some(x) = app.selected_finding() else {
        return draw_placeholder(f, area, t, "Preview", "No findings yet.");
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
    draw_view(f, area, app, lines, "Preview");
}

fn draw_problem(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let Some(p) = app.selected_problem() else {
        return draw_placeholder(f, area, t, "Problem", "No problems.");
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
    draw_view(f, area, app, lines, "Problem");
}

fn draw_placeholder(f: &mut Frame, area: Rect, t: &Theme, title: &str, text: &str) {
    let block = panel(t, title, false);
    f.render_widget(Paragraph::new(Span::styled(text.to_string(), Style::new().fg(t.muted))).block(block), area);
}

/// The toasts, stacked down from the top right, the newest lowest. `ok` is a run's toast:
/// green when it went well; others (`None`) are errors.
fn draw_toasts(f: &mut Frame, t: &Theme, toasts: &[(&str, Option<bool>)]) {
    let area = f.area();
    let mut y = area.y + 1;
    for (msg, ok) in toasts {
        let colour = if *ok == Some(true) { t.ok } else { t.err };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(colour))
            .style(Style::new().bg(t.surface));
        let para = Paragraph::new(msg.to_string()).block(block).wrap(Wrap { trim: true });
        // Up to 64 columns wide, wrapping onto more lines.
        let width = (msg.chars().count() as u16 + 4).min(64).min(area.width.saturating_sub(2));
        let height = (para.line_count(width) as u16).min(area.bottom().saturating_sub(y + 1));
        if height == 0 {
            break;
        }
        let rect = Rect { x: area.right().saturating_sub(width + 1), y, width, height };
        f.render_widget(Clear, rect);
        f.render_widget(para, rect);
        y += height;
    }
}

/// A rectangle `width` × `height` centred in `area`, shrunk to fit.
fn centred(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(4));
    let height = height.min(area.height.saturating_sub(2));
    Rect { x: area.x + (area.width - width) / 2, y: area.y + (area.height - height) / 2, width, height }
}

/// The command palette: a query line over the ranked list, or a chosen entry's argument line.
fn draw_palette(f: &mut Frame, app: &App, state: &palette::State) {
    let t = &app.theme;
    let block = |title: String| {
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(t.border_focus))
            .title(Line::from(Span::styled(title, Style::new().fg(t.accent).add_modifier(Modifier::BOLD))))
            .style(Style::new().bg(t.surface).fg(t.text))
            .padding(Padding::horizontal(1))
    };
    if let Some(form) = &state.form {
        return draw_form(f, t, form, block(format!(" chain {} ", form.chain.name)));
    }
    if let Some(line) = &state.export {
        let rect = centred(f.area(), 84, 5);
        f.render_widget(Clear, rect);
        let b = block(" export-transcript ".into());
        let inner = b.inner(rect);
        f.render_widget(b, rect);
        let prompt = "save to ";
        let lines = vec![
            Line::from(vec![
                Span::styled(prompt, Style::new().fg(t.accent)),
                Span::styled(line.text.clone(), Style::new().fg(t.text)),
            ]),
            Line::from(Span::styled("enter save · ctrl-u clear · esc back", Style::new().fg(t.muted))),
        ];
        f.render_widget(Paragraph::new(lines), inner);
        let x = inner.x + (prompt.chars().count() + line.text.chars().count()) as u16;
        f.set_cursor_position((x.min(inner.right().saturating_sub(1)), inner.y));
        return;
    }
    if let Some(args) = &state.args {
        let rect = centred(f.area(), 84, 6);
        f.render_widget(Clear, rect);
        let b = block(format!(" {} {} ", args.entry.kind.tag(), args.entry.name));
        let inner = b.inner(rect);
        f.render_widget(b, rect);
        let prompt = format!("rpt {} ", args.entry.name);
        let hint = args.hint.as_ref().map(|h| format!("usage: {h}")).unwrap_or_default();
        let lines = vec![
            Line::from(Span::styled(hint, Style::new().fg(t.muted))),
            Line::from(vec![
                Span::styled(prompt.clone(), Style::new().fg(t.accent)),
                Span::styled(args.text.clone(), Style::new().fg(t.text)),
            ]),
            Line::from(Span::styled("enter run · ctrl-u clear · esc back", Style::new().fg(t.muted))),
        ];
        f.render_widget(Paragraph::new(lines), inner);
        let x = inner.x + (prompt.chars().count() + args.text.chars().count()) as u16;
        f.set_cursor_position((x.min(inner.right().saturating_sub(1)), inner.y + 1));
        return;
    }

    let entries = app.entries();
    let rows = app.palette_rows(&entries);
    let selected = app.palette_selected(&rows);
    let rect = centred(f.area(), 84, rows.len() as u16 + 4);
    f.render_widget(Clear, rect);
    let b = block(" commands ".into());
    let inner = b.inner(rect);
    f.render_widget(b, rect);
    let [query, list] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(inner);
    f.render_widget(
        Line::from(vec![
            Span::styled("> ", Style::new().fg(t.accent)),
            Span::styled(state.query.clone(), Style::new().fg(t.text)),
        ]),
        query,
    );
    f.set_cursor_position((query.x + 2 + state.query.chars().count() as u16, query.y));

    let width = list.width as usize;
    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| match row {
            palette::Row::Header(h) => {
                ListItem::new(Span::styled(h.to_string(), Style::new().fg(t.accent).add_modifier(Modifier::BOLD)))
            }
            palette::Row::Loading => ListItem::new(Span::styled("  loading plugins…", Style::new().fg(t.muted))),
            palette::Row::Entry(i) => ListItem::new(entry_line(t, &entries[*i], width)),
        })
        .collect();
    let at = selected.and_then(|s| rows.iter().position(|r| *r == palette::Row::Entry(s)));
    let list_widget = List::new(items).highlight_style(Style::new().bg(t.bg)).highlight_symbol("▌");
    let mut list_state = ListState::default().with_selected(at);
    f.render_stateful_widget(list_widget, list, &mut list_state);
}

/// One palette row: kind tag, name, summary (or why it's greyed out), `input → output`, and
/// its key on the right.
fn entry_line<'a>(t: &Theme, e: &palette::Entry, width: usize) -> Line<'a> {
    let dim = e.disabled.is_some();
    let fg = |c| Style::new().fg(if dim { t.muted } else { c });
    let key = e.key.clone().unwrap_or_default();
    let io = e.io.as_ref().map(|io| format!("  {io}")).unwrap_or_default();
    let summary = match &e.disabled {
        Some(why) => format!("{} ({why})", e.summary),
        None => e.summary.clone(),
    };
    let head = format!(" {:<7}{:<18} ", e.kind.tag(), truncate(&e.name, 18));
    // Room for the summary once the tag, name, io and key are placed (and the highlight mark).
    let room = width.saturating_sub(1 + head.chars().count() + io.chars().count() + key.chars().count() + 1);
    let summary = truncate(&summary, room);
    let pad = room.saturating_sub(summary.chars().count());
    Line::from(vec![
        Span::styled(head, fg(t.text)),
        Span::styled(summary, fg(t.muted)),
        Span::styled(io, fg(t.accent)),
        Span::raw(" ".repeat(pad + 1)),
        Span::styled(key, fg(t.warn)),
    ])
}

/// A chain's parameter form: one row per parameter (name, kind, value), then hints.
fn draw_form(f: &mut Frame, t: &Theme, form: &Form, block: Block) {
    let n = form.values.len() as u16;
    let rect = centred(f.area(), 84, n + 6);
    f.render_widget(Clear, rect);
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    let width = form.chain.params.iter().map(|p| p.name.chars().count() + 1).max().unwrap_or(0);
    let mut lines = vec![Line::from(Span::styled(form.chain.description.clone(), Style::new().fg(t.muted)))];
    let mut cursor = None;
    for (i, (p, v)) in form.chain.params.iter().zip(&form.values).enumerate() {
        let here = i == form.field;
        // A flag always has a value, so it never shows as required.
        let required = p.required() && p.kind != ParamKind::Flag;
        let name = format!("{}{}", p.name, if required { "*" } else { "" });
        let (kind, shown) = match &p.kind {
            ParamKind::Text => ("text", v.clone()),
            ParamKind::File { .. } => ("file", v.clone()),
            ParamKind::Choice { .. } => ("choice", format!("‹ {v} ›")),
            ParamKind::Flag => ("flag", if v == "yes" { "[x] yes".into() } else { "[ ] no".into() }),
        };
        let mark = if here { "▌" } else { " " };
        let head = format!("{mark}{name:<width$}  {kind:<7}", width = width);
        if here && matches!(p.kind, ParamKind::Text | ParamKind::File { .. }) {
            cursor = Some((head.chars().count() + v.chars().count()) as u16);
        }
        let style = if here { Style::new().fg(t.text).bg(t.bg) } else { Style::new().fg(t.text) };
        lines.push(Line::from(vec![
            Span::styled(head, Style::new().fg(if here { t.accent } else { t.muted })),
            Span::styled(shown, style),
        ]));
    }
    lines.push(Line::from(Span::styled(form.error.clone().unwrap_or_default(), Style::new().fg(t.err))));
    let mut hints = "tab next · enter run · esc back".to_string();
    match form.param().kind {
        ParamKind::File { .. } => hints.push_str(" · ctrl-f pick a file"),
        ParamKind::Choice { .. } => hints.push_str(" · ←/→ choose"),
        ParamKind::Flag => hints.push_str(" · space toggles"),
        ParamKind::Text => {}
    }
    lines.push(Line::from(Span::styled(hints, Style::new().fg(t.muted))));
    f.render_widget(Paragraph::new(lines), inner);
    if let Some(x) = cursor {
        f.set_cursor_position(((inner.x + x).min(inner.right().saturating_sub(1)), inner.y + 1 + form.field as u16));
    }
}

/// The built-in file list: a filter line over the matching files.
fn draw_list_picker(f: &mut Frame, t: &Theme, lp: &super::picker::ListPicker) {
    let shown = lp.shown();
    let rect = centred(f.area(), 70, shown.len() as u16 + 5);
    f.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(t.border_focus))
        .title(Line::from(Span::styled(" pick a file ", Style::new().fg(t.accent).add_modifier(Modifier::BOLD))))
        .title_bottom(Line::from(Span::styled(
            " enter pick (or the typed path) · esc cancel ",
            Style::new().fg(t.muted),
        )))
        .style(Style::new().bg(t.surface).fg(t.text))
        .padding(Padding::horizontal(1));
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    let [query, list] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(inner);
    f.render_widget(
        Line::from(vec![Span::styled("> ", Style::new().fg(t.accent)), Span::raw(lp.query.clone())]),
        query,
    );
    f.set_cursor_position((query.x + 2 + lp.query.chars().count() as u16, query.y));
    let items: Vec<ListItem> = shown.iter().map(|p| ListItem::new(Span::raw(p.to_string()))).collect();
    let widget = List::new(items).highlight_style(Style::new().bg(t.bg)).highlight_symbol("▌");
    let mut state = ListState::default().with_selected((!shown.is_empty()).then_some(lp.sel));
    f.render_stateful_widget(widget, list, &mut state);
}

/// The key guide: the trouble, then its choices or the wait for a key press.
fn draw_guide(f: &mut Frame, t: &Theme, g: &super::guide::Guide, trouble: &crate::keys::Trouble, left: usize) {
    let mut lines = vec![
        Line::from(Span::styled(format!("{}: {}", trouble.at(), trouble.reason()), Style::new().fg(t.text))),
        Line::from(""),
    ];
    match &g.pressing {
        Some(fix) => {
            let side = match fix {
                super::guide::Fix::Rebind(side, _) | super::guide::Fix::Unbind(side, _) => side,
            };
            let who = side.who();
            lines.push(Line::from(Span::styled(format!("press the new key for {who}"), Style::new().fg(t.accent))));
        }
        None => {
            for (i, (label, _)) in super::guide::options(trouble).iter().enumerate() {
                let (mark, style) = if i == g.sel {
                    ("▌", Style::new().fg(t.accent).add_modifier(Modifier::BOLD))
                } else {
                    (" ", Style::new().fg(t.text))
                };
                lines.push(Line::from(Span::styled(format!("{mark}{label}"), style)));
            }
        }
    }
    if let Some(why) = &g.refused {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(why.clone(), Style::new().fg(t.err))));
    }
    let hint = if g.pressing.is_some() { " esc cancels " } else { " ↑/↓ choose · enter fix · esc skip " };
    let height = lines.len() as u16 + 2;
    let area = centred(f.area(), 76, height);
    f.render_widget(Clear, area);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(t.warn))
        .title(Line::from(Span::styled(
            format!(" Fix keys · 1 of {left} "),
            Style::new().fg(t.warn).add_modifier(Modifier::BOLD),
        )))
        .title_bottom(Line::from(Span::styled(hint, Style::new().fg(t.muted))))
        .padding(Padding::horizontal(1))
        .style(Style::new().bg(t.surface));
    f.render_widget(Paragraph::new(Text::from(lines)).block(block), area);
}
