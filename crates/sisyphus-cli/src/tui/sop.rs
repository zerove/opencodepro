use std::time::Instant;

use crossterm::event::KeyCode;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use super::background::{spawn_execute_sop, spawn_list_sops};
use super::helpers::{copy_to_clipboard, scroll_pct};
use super::{App, LogEntry};

pub async fn handle_sop_key(code: KeyCode, app: &mut App) {
    match code {
        KeyCode::Esc => {
            if !app.sop_query.is_empty() {
                app.sop_query.clear();
            }
        }
        KeyCode::Backspace => {
            app.sop_query.pop();
        }
        KeyCode::Char(c) => {
            if c == 'r' || c == 'R' {
                app.sop_status = "connecting...".into();
                app.sop_result = None;
                if app.pending_op.is_none() {
                    app.pending_op = Some(spawn_list_sops(app.bridge_address.clone()));
                }
                return;
            }
            if c == 'e' {
                let sop_id = app.filtered_sops().get(app.sop_sel).map(|s| s.sop_id.clone());
                let Some(sop_id) = sop_id else { return };
                app.sop_status = format!("executing {sop_id}...");
                app.sop_result = None;
                if app.pending_op.is_none() {
                    app.pending_op = Some(spawn_execute_sop(app.bridge_address.clone(), sop_id));
                }
                return;
            }
            if c == 'y' || c == 'Y' {
                let filtered = app.filtered_sops();
                if let Some(sop) = filtered.get(app.sop_sel.min(filtered.len().saturating_sub(1))) {
                    let text = format!("SOP: {} — nodes: {} | created: {}",
                        sop.sop_id, sop.node_count, sop.created_at);
                    copy_to_clipboard(&text);
                    app.copy_notification = Some(Instant::now());
                    app.add_log(LogEntry::Success("📋 Copied SOP info to clipboard".into()));
                }
                return;
            }
            app.sop_query.push(c);
        }
        KeyCode::Enter => {
            let sop_id = app.filtered_sops().get(app.sop_sel).map(|s| s.sop_id.clone());
            let Some(sop_id) = sop_id else { return };
            app.sop_status = format!("executing {sop_id}...");
            app.sop_result = None;
            if app.pending_op.is_none() {
                app.pending_op = Some(spawn_execute_sop(app.bridge_address.clone(), sop_id));
            }
        }
        KeyCode::Up => {
            app.sop_sel = app.sop_sel.saturating_sub(1);
        }
        KeyCode::Down => {
            let filtered = app.filtered_sops();
            if !filtered.is_empty() {
                app.sop_sel = (app.sop_sel + 1).min(filtered.len().saturating_sub(1));
            }
        }
        KeyCode::PageUp => {
            app.sop_sel = app.sop_sel.saturating_sub(10);
        }
        KeyCode::PageDown => {
            let filtered = app.filtered_sops();
            if !filtered.is_empty() {
                app.sop_sel = (app.sop_sel + 10).min(filtered.len().saturating_sub(1));
            }
        }
        KeyCode::Home => app.sop_sel = 0,
        KeyCode::End => {
            let filtered = app.filtered_sops();
            app.sop_sel = filtered.len().saturating_sub(1);
        }
        _ => {}
    }
}

pub fn render_sop(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let has_filter = !app.sop_query.is_empty();
    let filter_height: u16 = if has_filter { 1 } else { 0 };
    let [status_area, filter_area, list_area, result_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(filter_height),
        Constraint::Min(3),
        Constraint::Min(5),
    ])
    .areas(area);

    let status_style = match true {
        _ if app.sop_status.starts_with("connect") => Style::default().fg(theme.error),
        _ if app.sop_status.starts_with("executing") => Style::default().fg(theme.primary),
        _ if app.sop_status.starts_with("executed") => Style::default().fg(theme.success),
        _ if app.sop_status.starts_with("connected") => Style::default().fg(theme.success),
        _ => Style::default().fg(theme.muted),
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(&app.sop_status, status_style))),
        status_area,
    );

    if has_filter {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                 Span::styled("🔍 ", Style::default().fg(theme.info)),
                 Span::styled(&app.sop_query, Style::default().fg(theme.fg)),
            ])),
            filter_area,
        );
    }

    let filtered = app.filtered_sops();
    let list_block = Block::default()
        .borders(Borders::ALL)
        .title(" Standard Operating Procedures ")
        .title_bottom(scroll_pct(app.sop_sel, filtered.len()))
         .border_style(Style::default().fg(theme.border));

    let items: Vec<ListItem> = if filtered.is_empty() && !has_filter {
        vec![ListItem::new(Line::from(Span::styled(
            "Press r to refresh / connect to bridge",
            Style::default().fg(Color::DarkGray),
        )))]
    } else if filtered.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            format!("No SOPs match '{}'", app.sop_query),
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        filtered
            .iter()
            .map(|sop| {
                let line = Line::from(vec![
                     Span::styled(&sop.sop_id, Style::default().fg(theme.fg)),
                    Span::styled(
                        format!(" ({} nodes)", sop.node_count),
                        Style::default().fg(theme.muted),
                    ),
                ]);
                ListItem::new(line)
            })
            .collect()
    };

    let list = List::new(items)
        .block(list_block)
         .highlight_style(Style::default().fg(theme.secondary).add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(
        list,
        list_area,
        &mut ratatui::widgets::ListState::default().with_selected(Some(app.sop_sel)),
    );

    let result_block = Block::default()
        .borders(Borders::ALL)
        .title(" Result ")
         .border_style(Style::default().fg(theme.muted));
    let result_text = match app.sop_result {
        Some(ref r) => Text::from(Line::from(Span::styled(
             r,
             Style::default().fg(theme.fg),
        ))),
        None => Text::from(Line::from(Span::styled(
            "Select an SOP and press Enter to execute",
            Style::default().fg(Color::DarkGray),
        ))),
    };
    frame.render_widget(
        Paragraph::new(result_text)
            .block(result_block)
            .wrap(Wrap { trim: false }),
        result_area,
    );
}
