use std::time::Instant;

use crossterm::event::KeyCode;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use super::background::spawn_fetch_roadmap;
use super::helpers::{copy_to_clipboard, scroll_pct};
use super::{App, LogEntry};

pub async fn handle_roadmap_key(code: KeyCode, app: &mut App) {
    match code {
        KeyCode::Char('r') | KeyCode::Char('R') => {
            app.roadmap_status = "connecting...".into();
            if app.pending_op.is_none() {
                app.pending_op = Some(spawn_fetch_roadmap(app.bridge_address.clone()));
            }
        }
        KeyCode::Char('p') | KeyCode::Char('P') => {
            app.roadmap_show_projection = !app.roadmap_show_projection;
        }
        KeyCode::Char('b') | KeyCode::Char('B') => {
            app.roadmap_show_bottlenecks = !app.roadmap_show_bottlenecks;
        }
        KeyCode::Up => app.roadmap_scroll = app.roadmap_scroll.saturating_sub(1),
        KeyCode::Down => app.roadmap_scroll = app.roadmap_scroll.saturating_add(1),
        KeyCode::PageUp => app.roadmap_scroll = app.roadmap_scroll.saturating_sub(10),
        KeyCode::PageDown => app.roadmap_scroll = app.roadmap_scroll.saturating_add(10),
        KeyCode::Home | KeyCode::Char('g') => app.roadmap_scroll = 0,
        KeyCode::End | KeyCode::Char('G') => app.roadmap_scroll = usize::MAX,
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let mut content = String::new();
            if let Some(ref p) = app.roadmap_projection {
                content.push_str("--- Roadmap Projection ---\n");
                content.push_str(p);
                content.push_str("\n\n");
            }
            if let Some(ref b) = app.roadmap_bottlenecks {
                content.push_str("--- Identified Bottlenecks ---\n");
                content.push_str(b);
            }
            if !content.is_empty() {
                copy_to_clipboard(&content);
                app.copy_notification = Some(Instant::now());
                app.add_log(LogEntry::Success("📋 Copied roadmap to clipboard".into()));
            } else {
                app.add_log(LogEntry::Warn("No roadmap data to copy".into()));
            }
        }
        _ => {}
    }
}

pub fn render_roadmap(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let [status_area, content_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);

    let status_style = match app.roadmap_status.as_str() {
        "connected" => Style::default().fg(theme.success),
        "connecting..." => Style::default().fg(theme.primary),
        _ => Style::default().fg(theme.error),
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(&app.roadmap_status, status_style))),
        status_area,
    );

    let proj_visible = app.roadmap_show_projection;
    let bot_visible = app.roadmap_show_bottlenecks;

    let (proj_height, bot_height) = match (proj_visible, bot_visible) {
        (true, true) => (Constraint::Percentage(50), Constraint::Percentage(50)),
        (true, false) => (Constraint::Min(0), Constraint::Length(0)),
        (false, true) => (Constraint::Length(0), Constraint::Min(0)),
        (false, false) => (Constraint::Length(0), Constraint::Length(0)),
    };

    let [proj_area, bot_area] =
        Layout::vertical([proj_height, bot_height]).areas(content_area);

    let proj_title = if proj_visible {
        " Roadmap Projections "
    } else {
        " Roadmap Projections [collapsed \u{2014} press p to show] "
    };
    let mut proj_total: usize = 0;
    if proj_visible {
        proj_total = match &app.roadmap_projection {
            Some(p) => p.lines().count(),
            None => 1,
        };
    }
    let proj_block = Block::default()
        .borders(Borders::ALL)
        .title(proj_title)
        .title_bottom(scroll_pct(
            if app.roadmap_scroll == usize::MAX { 0 } else { app.roadmap_scroll },
            proj_total,
        ))
         .border_style(Style::default().fg(theme.border));

    if proj_visible {
        let (proj_text, proj_height) = match &app.roadmap_projection {
            Some(p) => {
                let lines: Vec<Line> = p.lines().map(|l| Line::from(Span::styled(l, Style::default().fg(theme.fg)))).collect();
                let len = lines.len();
                (Text::from(lines), len)
            }
            None => (
                Text::from(Line::from(Span::styled(
                    "No projection data. Press r to refresh.",
                     Style::default().fg(theme.muted),
                ))),
                1,
            ),
        };
        let visible_height = proj_area.height.saturating_sub(2);
        let max_scroll = (proj_height as u16).saturating_sub(visible_height);
        let effective_scroll = if app.roadmap_scroll == usize::MAX {
            max_scroll as usize
        } else {
            app.roadmap_scroll.min(max_scroll as usize)
        };
        frame.render_widget(
            Paragraph::new(proj_text)
                .block(proj_block)
                .wrap(Wrap { trim: false })
                .scroll((effective_scroll as u16, 0)),
            proj_area,
        );
    } else {
        frame.render_widget(proj_block, proj_area);
    }

    let bot_title = if bot_visible {
        " Identified Bottlenecks "
    } else {
        " Identified Bottlenecks [collapsed \u{2014} press b to show] "
    };
    let bot_block = Block::default()
        .borders(Borders::ALL)
        .title(bot_title)
         .border_style(Style::default().fg(theme.warning));

    if bot_visible {
        let (bot_text, bot_height) = match &app.roadmap_bottlenecks {
            Some(b) => {
                let lines: Vec<Line> = b.lines().map(|l| Line::from(Span::styled(l, Style::default().fg(theme.fg)))).collect();
                let len = lines.len();
                (Text::from(lines), len)
            }
            None => (
                Text::from(Line::from(Span::styled(
                    "No bottlenecks detected.",
                     Style::default().fg(theme.muted),
                ))),
                1,
            ),
        };
        let visible_height = bot_area.height.saturating_sub(2);
        let max_scroll = (bot_height as u16).saturating_sub(visible_height);
        let effective_scroll = if app.roadmap_scroll == usize::MAX {
            max_scroll as usize
        } else {
            app.roadmap_scroll.min(max_scroll as usize)
        };
        frame.render_widget(
            Paragraph::new(bot_text)
                .block(bot_block)
                .wrap(Wrap { trim: false })
                .scroll((effective_scroll as u16, 0)),
            bot_area,
        );
    } else {
        frame.render_widget(bot_block, bot_area);
    }
}
