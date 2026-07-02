use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use super::super::widgets;
use super::super::{App, DevFocus};

// ── Help Dialog ────────────────────────────────────────

pub(super) fn render_help_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();

    let lines = vec![
        Line::from(Span::styled("  Navigation", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD))),
        Line::from(vec![Span::styled("  Ctrl+K     ", Style::default().fg(theme.secondary)), Span::styled("Command palette", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  Ctrl+L     ", Style::default().fg(theme.secondary)), Span::styled("Session list", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  Ctrl+H     ", Style::default().fg(theme.secondary)), Span::styled("Go home", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  Ctrl+X h   ", Style::default().fg(theme.secondary)), Span::styled("Help (leader then h)", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  Ctrl+X p   ", Style::default().fg(theme.secondary)), Span::styled("Palette (leader then p)", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  Ctrl+Q     ", Style::default().fg(theme.secondary)), Span::styled("Exit (twice to confirm)", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  Tab        ", Style::default().fg(theme.secondary)), Span::styled("Toggle all thought blocks", Style::default().fg(theme.fg))]),
        Line::from(Span::raw("")),
        Line::from(Span::styled("  Session Input", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD))),
        Line::from(vec![Span::styled("  Enter      ", Style::default().fg(theme.secondary)), Span::styled("Send message", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  Esc        ", Style::default().fg(theme.secondary)), Span::styled("Cancel/back", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  ↑ ↓       ", Style::default().fg(theme.secondary)), Span::styled("Cycle input history", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  ← →       ", Style::default().fg(theme.secondary)), Span::styled("Move cursor", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  Home/End   ", Style::default().fg(theme.secondary)), Span::styled("Cursor start/end", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  Ctrl+A/E   ", Style::default().fg(theme.secondary)), Span::styled("Start/end of line", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  Ctrl+U     ", Style::default().fg(theme.secondary)), Span::styled("Clear line", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  Ctrl+W     ", Style::default().fg(theme.secondary)), Span::styled("Delete word backward", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  Ctrl+K     ", Style::default().fg(theme.secondary)), Span::styled("Delete to end of line", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  Backspace  ", Style::default().fg(theme.secondary)), Span::styled("Delete before cursor", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  Delete     ", Style::default().fg(theme.secondary)), Span::styled("Delete at cursor", Style::default().fg(theme.fg))]),
        Line::from(Span::raw("")),
        Line::from(Span::styled("  Slash Commands (in input)", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD))),
        Line::from(vec![Span::styled("  /help      ", Style::default().fg(theme.secondary)), Span::styled("Open this help", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  /dev       ", Style::default().fg(theme.secondary)), Span::styled("Open dev dialog", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  /home      ", Style::default().fg(theme.secondary)), Span::styled("Go to home", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  /sessions  ", Style::default().fg(theme.secondary)), Span::styled("Go to sessions", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  /clear     ", Style::default().fg(theme.secondary)), Span::styled("Clear input", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  /new       ", Style::default().fg(theme.secondary)), Span::styled("Create new session", Style::default().fg(theme.fg))]),
        Line::from(Span::raw("")),
        Line::from(Span::styled("  Session History", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD))),
        Line::from(vec![Span::styled("  ↑ ↓       ", Style::default().fg(theme.secondary)), Span::styled("Scroll messages", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  PgUp/PgDn  ", Style::default().fg(theme.secondary)), Span::styled("Scroll 10 lines", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  g / G      ", Style::default().fg(theme.secondary)), Span::styled("Top / bottom", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  t          ", Style::default().fg(theme.secondary)), Span::styled("Toggle last thought block", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  Ctrl+F     ", Style::default().fg(theme.secondary)), Span::styled("Search messages", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  y          ", Style::default().fg(theme.secondary)), Span::styled("Copy last assistant reply", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  Y          ", Style::default().fg(theme.secondary)), Span::styled("Copy selected message", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  C          ", Style::default().fg(theme.secondary)), Span::styled("Copy all messages", Style::default().fg(theme.fg))]),
        Line::from(Span::raw("")),
        Line::from(Span::styled("  Session List", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD))),
        Line::from(vec![Span::styled("  r          ", Style::default().fg(theme.secondary)), Span::styled("Refresh", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  d / Del    ", Style::default().fg(theme.secondary)), Span::styled("Delete session", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  R          ", Style::default().fg(theme.secondary)), Span::styled("Rename session", Style::default().fg(theme.fg))]),
        Line::from(vec![Span::styled("  c          ", Style::default().fg(theme.secondary)), Span::styled("Create new session", Style::default().fg(theme.fg))]),
        Line::from(Span::raw("")),
        Line::from(Span::styled("Pro tip: Type / in session input for slash commands", Style::default().fg(theme.info))),
    ];

    let line_count = lines.len() as u16;
    let visible_height = area.height.saturating_sub(2);
    let max_scroll = line_count.saturating_sub(visible_height);
    let scroll = app.help_scroll.min(max_scroll as usize);

    let title_bottom = if max_scroll > 0 {
        format!(" ↑↓/scroll: {} | Esc/q to close ", widgets::scroll_pct(scroll, max_scroll as usize))
    } else {
        " Esc/q to close ".into()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Help ")
        .title_bottom(title_bottom)
        .border_style(Style::default().fg(theme.primary));

    let para = Paragraph::new(Text::from(lines))
        .block(block)
        .scroll((scroll as u16, 0));
    frame.render_widget(para, area);
}

// ── Session List Dialog ───────────────────────────────

pub(super) fn render_session_list_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let items: Vec<ListItem> = app
        .sessions
        .iter()
        .map(|s| {
            ListItem::new(Line::from(vec![
                Span::styled(&s.name, Style::default().fg(theme.fg)),
                Span::styled(
                    format!(" ({} msgs)", s.message_count),
                    Style::default().fg(theme.muted),
                ),
            ]))
        })
        .collect();
    widgets::render_list(frame, area, items, app.session_sel, " Sessions ".into(), &theme);
}

// ── Todos Dialog ─────────────────────────────────────

pub(super) fn render_todos_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Todos ")
        .border_style(Style::default().fg(theme.primary));

    if app.todo_items.is_empty() {
        let text = Text::from(Line::from(Span::styled(
            "No todos yet. Press a to add one.",
            Style::default().fg(theme.muted),
        )));
        frame.render_widget(Paragraph::new(text).block(block), area);
        return;
    }

    let items: Vec<ListItem> = app.todo_items.iter().map(|item| {
        let checkbox = match item.status {
            sisyphus_tools::TodoStatus::Pending => "[ ]",
            sisyphus_tools::TodoStatus::InProgress => "[~]",
            sisyphus_tools::TodoStatus::Completed => "[✓]",
            sisyphus_tools::TodoStatus::Cancelled => "[✗]",
        };
        let style = match item.status {
            sisyphus_tools::TodoStatus::Completed => Style::default().fg(theme.success).add_modifier(Modifier::DIM),
            sisyphus_tools::TodoStatus::Cancelled => Style::default().fg(theme.error).add_modifier(Modifier::DIM),
            sisyphus_tools::TodoStatus::InProgress => Style::default().fg(theme.primary),
            sisyphus_tools::TodoStatus::Pending => Style::default().fg(theme.fg),
        };
        ListItem::new(Line::from(vec![
            Span::styled(format!("{} ", checkbox), style),
            Span::styled(&item.content, style),
        ]))
    }).collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().fg(theme.secondary).add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(
        list,
        area,
        &mut ratatui::widgets::ListState::default().with_selected(Some(app.todo_sel)),
    );
}

// ── LSP Dialog ───────────────────────────────────────

pub(super) fn render_lsp_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let filtered: Vec<&sisyphus_tools::lsp::LspDiagnostic> = if app.dialog_query.is_empty() {
        app.lsp_diagnostics.iter().collect()
    } else {
        let q = app.dialog_query.to_lowercase();
        app.lsp_diagnostics.iter().filter(|d| d.message.to_lowercase().contains(&q)).collect()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" LSP Diagnostics [{} {}] ", app.lsp_language, app.lsp_server))
        .title_bottom(format!(" {} / {} | Type to filter... ", filtered.len(), app.lsp_diagnostics.len()))
        .border_style(Style::default().fg(theme.primary));

    let items: Vec<ListItem> = filtered.iter().map(|d| {
        let sev = d.severity.map(sisyphus_tools::lsp::severity_name).unwrap_or("?");
        let style = match sev {
            "error" => Style::default().fg(theme.error),
            "warning" => Style::default().fg(theme.warning),
            "info" => Style::default().fg(theme.info),
            _ => Style::default().fg(theme.muted),
        };
        ListItem::new(Line::from(vec![
            Span::styled(format!("{:>6} ", sev), style),
            Span::styled(&d.message, Style::default().fg(theme.fg)),
        ]))
    }).collect();

    let list = List::new(items).block(block);
    frame.render_stateful_widget(
        list,
        area,
        &mut ratatui::widgets::ListState::default().with_selected(Some(app.lsp_scroll)),
    );
}

// ── SOP Dialog ────────────────────────────────────────

pub(super) fn render_sop_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let filtered: Vec<&crate::bridge::SopInfo> = if app.sop_query.is_empty() {
        app.sop_sops.iter().collect()
    } else {
        let q = app.sop_query.to_lowercase();
        app.sop_sops.iter().filter(|s| s.sop_id.to_lowercase().contains(&q)).collect()
    };

    let items: Vec<ListItem> = if filtered.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "Press r to refresh / connect to bridge",
            Style::default().fg(theme.muted),
        )))]
    } else {
        filtered.iter().map(|sop| {
            ListItem::new(Line::from(vec![
                Span::styled(&sop.sop_id, Style::default().fg(theme.fg)),
                Span::styled(format!(" ({} nodes)", sop.node_count), Style::default().fg(theme.muted)),
            ]))
        }).collect()
    };
    widgets::render_list(frame, area, items, app.sop_sel, " SOP List ".into(), &theme);
}

// ── Roadmap Dialog ────────────────────────────────────

pub(super) fn render_roadmap_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Roadmap ")
        .border_style(Style::default().fg(theme.primary));

    let content = match &app.roadmap_projection {
        Some(p) => Text::from(Line::from(Span::styled(p, Style::default().fg(theme.fg)))),
        None => Text::from(Line::from(Span::styled(
            "No roadmap data. Press r to refresh.",
            Style::default().fg(theme.muted),
        ))),
    };

    let para = Paragraph::new(content).block(block).wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

// ── Confirm Dialog ─────────────────────────────────────

pub(super) fn render_confirm_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Confirm ")
        .border_style(Style::default().fg(theme.warning));

    let text = Text::from(vec![
        Line::from(Span::styled("Delete this session?", Style::default().fg(theme.fg))),
        Line::from(""),
        Line::from(Span::styled("Press y to confirm, n or Esc to cancel", Style::default().fg(theme.muted))),
    ]);

    let para = Paragraph::new(text).block(block);
    frame.render_widget(para, area);
}

// ── Rename Dialog ──────────────────────────────────────

pub(super) fn render_rename_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App, name: &str) {
    let theme = app.theme();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Rename Session ")
        .border_style(Style::default().fg(theme.primary));
    let [_, inner, _] = Layout::vertical([Constraint::Percentage(50), Constraint::Length(3), Constraint::Percentage(50)]).areas(area);

    let display = if name.is_empty() { " (empty — cancels) " } else { name };
    let input = Paragraph::new(Line::from(Span::styled(
        format!(" {} ", display),
        Style::default().fg(theme.fg),
    )))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.secondary)),
    );
    let hint = Paragraph::new(Line::from(Span::styled(
        " Enter to confirm | Esc to cancel ",
        Style::default().fg(theme.muted),
    )));

    let layout = Layout::vertical([Constraint::Length(3), Constraint::Length(1)]).split(inner);
    frame.render_widget(block, area);
    frame.render_widget(input, layout[0]);
    frame.render_widget(hint, layout[1]);
}

// ── Dev Dialog ────────────────────────────────────────

pub(super) fn render_dev_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Dev Agent ")
        .border_style(Style::default().fg(theme.primary));

    let focus_style = |focused: bool| {
        if focused {
            Style::default().fg(theme.secondary).add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(theme.fg)
        }
    };

    // Show report if available
    if let Some(report) = &app.dev_report {
        let lines: Vec<Line> = report.lines().map(|l| {
            let style = if l.starts_with("✅") || l.starts_with("✔") {
                Style::default().fg(theme.success)
            } else if l.starts_with("❌") || l.starts_with("⚠") {
                Style::default().fg(theme.error)
            } else {
                Style::default().fg(theme.fg)
            };
            Line::from(Span::styled(l.to_string(), style))
        }).collect();

        let para = Paragraph::new(Text::from(lines))
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(para, area);
        return;
    }

    // Show running state
    if app.dev_status == "running" {
        let text = Text::from(vec![
            Line::from(Span::styled(
                "  Running developer agent...",
                Style::default().fg(theme.primary),
            )),
            Line::from(Span::styled(
                "  This may take a few minutes.",
                Style::default().fg(theme.muted),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Press Esc to cancel (agent continues in background)",
                Style::default().fg(theme.muted),
            )),
        ]);
        let para = Paragraph::new(text).block(block);
        frame.render_widget(para, area);
        return;
    }

    // Show input form
    let mut lines = Vec::new();

    // Goal
    let goal_label = Span::styled("Goal:", focus_style(app.dev_focus == DevFocus::Goal));
    let goal_val = if app.dev_goal.is_empty() {
        Span::styled(" <type your dev goal>", Style::default().fg(theme.muted))
    } else {
        Span::styled(&app.dev_goal, Style::default().fg(theme.fg))
    };
    lines.push(Line::from(vec![goal_label, goal_val]));
    lines.push(Line::from(""));

    // Context
    let ctx_label = Span::styled("Context:", focus_style(app.dev_focus == DevFocus::Context));
    let ctx_val = if app.dev_context.is_empty() {
        Span::styled(" <optional project context>", Style::default().fg(theme.muted))
    } else {
        Span::styled(&app.dev_context, Style::default().fg(theme.fg))
    };
    lines.push(Line::from(vec![ctx_label, ctx_val]));
    lines.push(Line::from(""));

    // Parallel
    let par_checked = if app.dev_parallel { "[x]" } else { "[ ]" };
    let par_label = Span::styled(
        format!("Parallel: {par_checked}  (run steps concurrently, max 4)"),
        focus_style(app.dev_focus == DevFocus::Parallel),
    );
    lines.push(Line::from(par_label));
    lines.push(Line::from(""));

    // Resume
    let res_checked = if app.dev_resume { "[x]" } else { "[ ]" };
    let res_label = Span::styled(
        format!("Resume: {res_checked}  (skip completed steps from checkpoint)"),
        focus_style(app.dev_focus == DevFocus::Resume),
    );
    lines.push(Line::from(res_label));
    lines.push(Line::from(""));

    // Run button
    let run_style = if app.dev_focus == DevFocus::Run {
        Style::default().fg(theme.secondary).add_modifier(Modifier::REVERSED)
    } else {
        Style::default().fg(theme.success)
    };
    lines.push(Line::from(Span::styled(" [ Run Dev Agent ] ", run_style)));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " Tab/Shift+Tab: navigate  Enter: run  Space: toggle  Esc: close",
        Style::default().fg(theme.muted),
    )));

    let para = Paragraph::new(Text::from(lines)).block(block);
    frame.render_widget(para, area);
}


