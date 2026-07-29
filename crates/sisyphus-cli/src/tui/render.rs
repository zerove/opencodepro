use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use super::app::{matching_palette_entries, App, Mode, View};

// ── UI: Routing by View ─────────────────────────────────

pub(crate) fn ui(frame: &mut ratatui::Frame, app: &App) {
    if app.palette_open {
        render_palette(frame, app);
        return;
    }

    let [content_area, status_area] = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Min(0),
        ratatui::layout::Constraint::Length(1),
    ])
    .areas(frame.area());

    // Route content by current view
    match app.view {
        View::Home => crate::tui::home::render_home(frame, content_area, app),
        View::Session => crate::tui::sessions::render_sessions(frame, content_area, app),
        View::Dashboard => crate::tui::dashboard::render_dashboard(frame, content_area, app),
    }

    // If a dialog is active, render it as overlay
    if app.dialog.is_some() {
        let overlay = ratatui::widgets::Clear;
        frame.render_widget(overlay, frame.area());
        crate::tui::dialog::render_dialog_overlay(frame, frame.area(), app);
    }

    frame.render_widget(
        ratatui::widgets::Paragraph::new(render_status(app)),
        status_area,
    );

    // Render toast notifications as floating overlay (bottom-right)
    render_toasts(frame, app);
}

fn render_palette(frame: &mut ratatui::Frame, app: &App) {
    let theme = app.theme();
    let area = frame.area();
    frame.render_widget(ratatui::widgets::Clear, area);

    let [_, vert_area, _] = Layout::vertical([
        Constraint::Percentage(25),
        Constraint::Length(20),
        Constraint::Percentage(25),
    ])
    .areas(area);
    let [_, pal_area, _] = Layout::horizontal([
        Constraint::Percentage(20),
        Constraint::Percentage(60),
        Constraint::Percentage(20),
    ])
    .areas(vert_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Command Palette ")
        .title_bottom(format!(
            " {} matches | Type to filter... ",
            matching_palette_entries(&app.palette_query).len()
        ))
        .border_style(Style::default().fg(theme.primary));

    let items: Vec<ListItem> = matching_palette_entries(&app.palette_query)
        .iter()
        .map(|e| {
            let shortcut_style = if e.shortcut.is_empty() {
                Style::default().fg(theme.muted)
            } else {
                Style::default().fg(theme.secondary)
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {:<16}", e.label),
                    Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
                ),
                Span::styled(e.desc, Style::default().fg(theme.muted)),
                Span::raw("  "),
                Span::styled(e.shortcut, shortcut_style),
            ]))
        })
        .collect();

    let sel = app.palette_sel.min(
        matching_palette_entries(&app.palette_query)
            .len()
            .saturating_sub(1),
    );
    let list = List::new(items).block(block).highlight_style(
        Style::default()
            .fg(theme.secondary)
            .add_modifier(Modifier::REVERSED),
    );
    frame.render_stateful_widget(
        list,
        pal_area,
        &mut ratatui::widgets::ListState::default().with_selected(Some(sel)),
    );
}

/// Count diagnostics by severity (LSP: 1=error, 2=warning, 3=info, 4=hint).
pub(crate) fn diag_counts(app: &App) -> (usize, usize, usize) {
    let (mut errs, mut warns, mut infos) = (0, 0, 0);
    for d in &app.lsp_diagnostics {
        match d.severity {
            Some(1) => errs += 1,
            Some(2) => warns += 1,
            Some(3) | Some(4) => infos += 1,
            _ => infos += 1,
        }
    }
    (errs, warns, infos)
}

/// Estimate context usage percentage from the active session's messages.
pub(crate) fn context_pct(app: &App) -> u8 {
    const MAX_TOKENS: u64 = 128_000;
    let total: u64 = app
        .session_detail
        .as_ref()
        .map(|s| s.messages.iter().map(|m| m.estimated_tokens).sum())
        .unwrap_or(0);
    if total == 0 {
        return 0;
    }
    let pct = (total as f64 / MAX_TOKENS as f64) * 100.0;
    pct.min(100.0) as u8
}

pub(crate) fn render_status(app: &App) -> Line<'static> {
    let theme = app.theme();

    if app.leader_mode {
        return Line::from(Span::styled(
            " [leader] Press h: help | p: palette | Esc: cancel ",
            Style::default().fg(Color::Black).bg(theme.warning),
        ));
    }

    let view_label = match app.view {
        View::Home => " HOME ",
        View::Session => " SESSION ",
        View::Dashboard => " DASHBOARD ",
    };

    let help: String = match app.view {
        View::Home => {
            "Enter: start session | Ctrl+K: palette | Ctrl+L: sessions | type prompt".into()
        }
        View::Dashboard => "Ctrl+H home | Ctrl+D toggle dashboard".into(),
        View::Session => {
            if app.session_detail.is_some() {
                let mode_label = if app.session_mode == Mode::Input {
                    "INPUT"
                } else {
                    "NORMAL"
                };
                let mode_extra = if app.session_mode == Mode::Input {
                    " | Ctrl+U clear | Ctrl+W del word | Ctrl+A/E home/end | /command"
                } else {
                    ""
                };
                format!("[{mode_label}] Enter send | Esc back{mode_extra}")
            } else {
                "↑↓ select | Enter open | Ctrl+N new | Ctrl+H home | type filter".to_string()
            }
        }
    };

    let spinner_chars = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏";
    let spinner = if app.pending_op.is_some() {
        let idx = (app.pending_spin as usize) % spinner_chars.chars().count();
        let c = spinner_chars.chars().nth(idx).unwrap_or(' ');
        format!(" {c} ")
    } else {
        String::new()
    };

    let ctx = context_pct(app);
    let ctx_style = if ctx > 80 {
        Style::default().fg(Color::Black).bg(theme.warning)
    } else {
        Style::default().fg(theme.muted)
    };

    let (errs, warns, infos) = diag_counts(app);

    let mut spans: Vec<Span<'static>> = vec![
        Span::styled(
            view_label,
            Style::default().fg(Color::Black).bg(theme.primary),
        ),
        Span::raw(" "),
        Span::styled(help, Style::default().fg(theme.muted)),
    ];

    if !spinner.is_empty() {
        spans.push(Span::styled(spinner, Style::default().fg(theme.emphasized)));
    }

    spans.push(Span::raw("  "));
    spans.push(Span::styled(format!("Context: {ctx}%"), ctx_style));

    // Diagnostics
    if errs > 0 || warns > 0 || infos > 0 {
        spans.push(Span::raw("  "));
        if errs > 0 {
            spans.push(Span::styled(
                format!("E{} ", errs),
                Style::default().fg(theme.error),
            ));
        }
        if warns > 0 {
            spans.push(Span::styled(
                format!("W{} ", warns),
                Style::default().fg(theme.warning),
            ));
        }
        if infos > 0 {
            spans.push(Span::styled(
                format!("I{} ", infos),
                Style::default().fg(theme.info),
            ));
        }
    }

    spans.push(Span::raw("  "));
    if app.ralph_enabled {
        spans.push(Span::styled(
            " RALPH ",
            Style::default().fg(Color::Black).bg(theme.warning),
        ));
        spans.push(Span::raw(" "));
    }
    if app.dev_status == "running" {
        spans.push(Span::styled(
            " ⚡Dev ",
            Style::default().fg(Color::Black).bg(theme.success),
        ));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(
        app.model_name.clone(),
        Style::default().fg(theme.muted),
    ));

    if let Some((msg, ts)) = &app.notification {
        if ts.elapsed() < std::time::Duration::from_secs(4) {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                format!("⚠ {msg}"),
                Style::default().fg(theme.warning),
            ));
        }
    }

    Line::from(spans)
}

fn render_toasts(frame: &mut ratatui::Frame, app: &App) {
    if app.toasts.is_empty() {
        return;
    }
    let theme = app.theme();
    let area = frame.area();

    let stacked_height = (app.toasts.len() * 3 + 2) as u16;
    let [_, toast_vertical] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(stacked_height)]).areas(area);
    let [_, toast_area] =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
            .areas(toast_vertical);

    let inner = Layout::vertical(
        std::iter::repeat_n(Constraint::Length(3), app.toasts.len()).collect::<Vec<_>>(),
    )
    .split(toast_area);

    for (i, toast) in app.toasts.iter().enumerate() {
        if i >= inner.len() {
            break;
        }
        let (fg, bg) = toast.style(&theme);
        let label = Paragraph::new(Line::from(Span::styled(
            &toast.message,
            Style::default().fg(fg).bg(bg),
        )))
        .block(
            Block::default()
                .borders(Borders::NONE)
                .style(Style::default().bg(bg)),
        );
        frame.render_widget(ratatui::widgets::Clear, inner[i]);
        frame.render_widget(label, inner[i]);
    }
}
