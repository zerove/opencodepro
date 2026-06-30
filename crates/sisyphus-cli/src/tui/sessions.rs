use std::time::Instant;

use crossterm::event::KeyCode;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use super::background::spawn_session_add_message;
use super::helpers::{copy_to_clipboard, render_message_content, scroll_pct};
use super::{App, LogEntry, Mode};

pub fn handle_sessions_key(code: KeyCode, app: &mut App) {
    app.refresh_sessions();

    // Handle inline rename/create modes
    if app.rename_session_id.is_some() {
        match code {
            KeyCode::Enter => {
                let id = app.rename_session_id.take().unwrap_or_default();
                let name = app.rename_buf.trim().to_string();
                if !name.is_empty() {
                    let db = app.data_dir.join("sessions.json");
                    match sisyphus_tools::session_rename(&db, &id, &name) {
                        Ok(_) => {
                            app.add_log(LogEntry::Success(format!("Renamed session {id} → {name}")));
                            app.session_dirty = true;
                        }
                        Err(e) => app.add_log(LogEntry::Error(format!("Rename failed: {e}"))),
                    }
                }
                app.rename_buf.clear();
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                app.rename_session_id = None;
                app.rename_buf.clear();
            }
            KeyCode::Backspace => { app.rename_buf.pop(); }
            KeyCode::Char(c) => { app.rename_buf.push(c); }
            _ => {}
        }
        return;
    }

    if app.session_detail.is_some() {
        match app.session_mode {
            Mode::Input => match code {
                KeyCode::Enter => {
                    if let Some(ref mut session) = app.session_detail {
                        let content = app.session_input.trim().to_string();
                        if !content.is_empty() {
                            // Immediately add message to local state so user sees it right away
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs().to_string())
                                .unwrap_or_default();
                            session.messages.push(sisyphus_tools::Message {
                                role: "user".into(),
                                content: content.clone(),
                                created_at: now,
                                estimated_tokens: (content.len() as u64).div_ceil(4),
                                is_compressed: false,
                            });
                            session.message_count = session.messages.len() as u64;
                            app.session_detail_scroll = usize::MAX;
                            app.pending_op = Some(spawn_session_add_message(
                                app.data_dir.clone(),
                                session.id.clone(),
                                "user".to_string(),
                                content,
                                Some(app.bridge_address.clone()),
                            ));
                            app.session_input.clear();
                        }
                    }
                    app.session_mode = Mode::Select;
                }
                KeyCode::Esc => {
                    app.session_mode = Mode::Select;
                    app.session_input.clear();
                }
                KeyCode::Backspace => {
                    app.session_input.pop();
                }
                KeyCode::Char(c) => {
                    app.session_input.push(c);
                }
                _ => {}
            },
            _ => match code {
                KeyCode::Enter | KeyCode::Char('i') => {
                    app.session_mode = Mode::Input;
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    app.session_detail = None;
                    app.session_mode = Mode::Select;
                }
                KeyCode::Up => {
                    app.session_detail_scroll = app.session_detail_scroll.saturating_sub(1);
                }
                KeyCode::Down => {
                    app.session_detail_scroll = app.session_detail_scroll.saturating_add(1);
                }
                KeyCode::PageUp => {
                    app.session_detail_scroll = app.session_detail_scroll.saturating_sub(10);
                }
                KeyCode::PageDown => {
                    app.session_detail_scroll = app.session_detail_scroll.saturating_add(10);
                }
                KeyCode::Home | KeyCode::Char('g') => {
                    app.session_detail_scroll = 0;
                }
                KeyCode::End | KeyCode::Char('G') => {
                    app.session_detail_scroll = usize::MAX;
                }
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    // Copy last assistant message to clipboard
                    if let Some(ref detail) = app.session_detail {
                        let last_assistant = detail.messages
                            .iter()
                            .rev()
                            .find(|m| m.role == "assistant")
                            .map(|m| m.content.as_str());
                        if let Some(content) = last_assistant {
                            copy_to_clipboard(content);
                            app.copy_notification = Some(Instant::now());
                            app.add_log(LogEntry::Success("📋 Copied last assistant message to clipboard".into()));
                        } else {
                            app.add_log(LogEntry::Warn("No assistant message to copy".into()));
                        }
                    }
                }
                KeyCode::Char('C') => {
                    // Copy entire session detail text to clipboard
                    if let Some(ref detail) = app.session_detail {
                        let mut all_text = String::new();
                        for msg in &detail.messages {
                            let role = match msg.role.as_str() {
                                "user" => "You",
                                "assistant" => "Sisyphus",
                                r => r,
                            };
                            all_text.push_str(&format!("[{}] {}\n{}\n\n", role, msg.created_at, msg.content));
                        }
                        copy_to_clipboard(&all_text);
                        app.copy_notification = Some(Instant::now());
                        app.add_log(LogEntry::Success("📋 Copied entire session to clipboard".into()));
                    }
                }
                _ => {}
            },
        }
        return;
    }

    match code {
        KeyCode::Up => app.session_sel = app.session_sel.saturating_sub(1),
        KeyCode::Down => {
            let visible = app.filtered_sessions().len();
            app.session_sel = (app.session_sel + 1).min(visible.saturating_sub(1));
        }
        KeyCode::PageUp => app.session_sel = app.session_sel.saturating_sub(10),
        KeyCode::PageDown => {
            let visible = app.filtered_sessions().len();
            app.session_sel = (app.session_sel + 10).min(visible.saturating_sub(1));
        }
        KeyCode::Home => app.session_sel = 0,
        KeyCode::End => {
            let visible = app.filtered_sessions().len();
            app.session_sel = visible.saturating_sub(1);
        }
        KeyCode::Enter => app.select_session(),
        KeyCode::Esc | KeyCode::Char('q') => {
            if !app.session_filter.is_empty() {
                app.session_filter.clear();
            }
            app.confirm_delete = None;
        }
        KeyCode::Backspace => {
            app.session_filter.pop();
            app.session_sel = 0;
        }
        KeyCode::Char('r') => app.session_dirty = true,
        KeyCode::Char('d') | KeyCode::Delete => app.delete_session(),
        KeyCode::Char('c') => {
            app.create_buf.clear();
            app.session_dirty = true;
            // Immediately create with empty name → server-assigned ID
            let db = app.data_dir.join("sessions.json");
            match sisyphus_tools::session_create(&db, "unnamed") {
                Ok(s) => {
                    app.add_log(LogEntry::Success(format!("Created session {} ({})", s.id, s.name)));
                    app.session_dirty = true;
                }
                Err(e) => app.add_log(LogEntry::Error(format!("Create failed: {e}"))),
            }
        }
        KeyCode::Char('R') => {
            let filtered = app.filtered_sessions();
            if filtered.is_empty() {
                return;
            }
            let idx = filtered[app.session_sel.min(filtered.len().saturating_sub(1))];
            app.rename_session_id = Some(app.sessions[idx].id.clone());
            app.rename_buf = app.sessions[idx].name.clone();
        }
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let filtered = app.filtered_sessions();
            if let Some(&idx) = filtered.get(app.session_sel.min(filtered.len().saturating_sub(1))) {
                if let Some(s) = app.sessions.get(idx) {
                    let text = format!("{} ({}) — {} msgs", s.name, s.id, s.message_count);
                    copy_to_clipboard(&text);
                    app.copy_notification = Some(Instant::now());
                    app.add_log(LogEntry::Success("📋 Copied session info to clipboard".into()));
                }
            }
        }
        KeyCode::Char(c) if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' => {
            app.session_filter.push(c);
            app.session_sel = 0;
        }
        _ => {}
    }
}

pub fn render_sessions(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    if app.sessions.is_empty() {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Sessions ")
            .border_style(Style::default().fg(theme.border));
        let text = Text::from(Line::from(Span::styled(
            "No sessions. Use `sisyphus session create` in shell tab.",
            Style::default().fg(theme.muted),
        )));
        frame.render_widget(Paragraph::new(text).block(block), area);
        return;
    }

    // Detail view
    if let Some(ref detail) = app.session_detail {
        render_session_detail(frame, area, detail, app.session_detail_scroll, app);
        return;
    }

    // Layout: filter bar on top, session list below
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
    ])
    .split(area);

    // Filter bar
    let filter_style = if app.session_filter.is_empty() {
        Style::default().fg(theme.muted)
    } else {
        Style::default().fg(theme.emphasized)
    };
    let filter_text = if app.session_filter.is_empty() {
        " 🔍 Type to filter sessions..."
    } else {
        &app.session_filter
    };
    let filter_block = Block::default()
        .borders(Borders::ALL)
        .title(" Search ")
            .border_style(Style::default().fg(theme.border));
    let filter_widget = Paragraph::new(Text::from(Line::from(Span::styled(
        filter_text,
        filter_style,
    ))))
    .block(filter_block);
    frame.render_widget(filter_widget, chunks[0]);

    // Session list (filtered)
    let filtered_indices = app.filtered_sessions();
    let visible_count = filtered_indices.len();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Sessions ({visible_count}) "))
        .title_bottom(scroll_pct(app.session_sel, visible_count))
            .border_style(Style::default().fg(theme.border));

    let items: Vec<ListItem> = filtered_indices
        .iter()
        .map(|&idx| {
            let s = &app.sessions[idx];
            let mut spans = vec![
                Span::raw("  "),
                Span::styled(&s.name, Style::default().fg(theme.fg)),
                Span::styled(
                    format!(" ({} msgs)", s.message_count),
                    Style::default().fg(theme.muted),
                ),
            ];
            // Show matching reason when filtered
            if !app.session_filter.is_empty() {
                let q = app.session_filter.to_lowercase();
                if s.name.to_lowercase().contains(&q) {
                    spans.push(Span::styled(" [name]", Style::default().fg(theme.success)));
                } else if s.id.contains(&q) {
                    spans.push(Span::styled(" [id]", Style::default().fg(theme.success)));
                }
            }
            let line = Line::from(spans);
            ListItem::new(line)
        })
        .collect();

    let adj_sel = app.session_sel.min(visible_count.saturating_sub(1));
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().fg(theme.secondary).add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(
        list,
        chunks[1],
        &mut ratatui::widgets::ListState::default().with_selected(Some(adj_sel)),
    );
}

fn render_session_detail(
    frame: &mut ratatui::Frame,
    area: Rect,
    session: &sisyphus_tools::Session,
    scroll: usize,
    app: &App,
) {
    let suggestion_count = app.session_suggestions.len() as u16;
    let theme = app.theme();
    let show_suggestions = !session.messages.is_empty()
        && app.pending_op.is_none()
        && suggestion_count > 0;
    let suggestion_height = if show_suggestions {
        2 + suggestion_count
    } else {
        0
    };
    let chunks = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(3),
        Constraint::Length(suggestion_height),
    ])
    .split(area);
    let history_area = chunks[0];
    let input_area = chunks[1];
    let suggestion_area = chunks[2];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Session: {} ", session.name))
            .border_style(Style::default().fg(theme.border));

    let mut lines = Vec::new();

    for msg in &session.messages {
        let role_label = match msg.role.as_str() {
            "user" => "You",
            "assistant" => "Sisyphus",
            _ => &msg.role,
        };
        let role_style = match msg.role.as_str() {
            "user" => Style::default().fg(theme.info).add_modifier(Modifier::BOLD),
            "assistant" => Style::default().fg(theme.success).add_modifier(Modifier::BOLD),
            _ => Style::default().fg(theme.muted),
        };
        let ts_style = Style::default().fg(theme.muted);
        let ts = if msg.created_at.len() >= 16 {
            msg.created_at[11..16].to_string()
        } else {
            msg.created_at.clone()
        };
        lines.push(Line::from(vec![
            Span::styled(format!("[{role_label}] "), role_style),
            Span::styled(ts, ts_style),
        ]));
        // Simple inline rendering: detect **bold** and `code`
        render_message_content(&mut lines, &msg.content);
        lines.push(Line::from(Span::raw("")));
    }

    // Thinking indicator when waiting for agent response
    if app.pending_op.is_some() && !session.messages.is_empty() {
        let spinner_frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let frame_idx = (app.pending_spin as usize / 3) % spinner_frames.len();
        let spinner = spinner_frames[frame_idx];
        lines.push(Line::from(vec![
            Span::styled(format!("[{spinner}] "), Style::default().fg(theme.emphasized)),
            Span::styled("Sisyphus 思考中...", Style::default().fg(theme.emphasized)),
        ]));
    }

    let text_height = lines.len() as u16;
    let visible_height = history_area.height.saturating_sub(2);
    let max_scroll = text_height.saturating_sub(visible_height);
    let auto_scroll = scroll == usize::MAX;
    let effective_scroll = if auto_scroll {
        max_scroll as usize
    } else {
        scroll.min(max_scroll as usize)
    };

    let paragraph = Paragraph::new(Text::from(lines))
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((effective_scroll as u16, 0));
    frame.render_widget(paragraph, history_area);

    let input_block = Block::default()
        .borders(Borders::ALL)
        .title(if app.pending_op.is_some() { " ⏳ 等待回复... " } else { " Chat " })
        .border_style(if app.session_mode == Mode::Input {
            Style::default().fg(theme.emphasized)
        } else if app.pending_op.is_some() {
            Style::default().fg(theme.error)
        } else {
            Style::default().fg(theme.muted)
        });

    let input_text = if app.session_input.is_empty() {
        "Type a message and press Enter to send..."
    } else {
        &app.session_input
    };

    let input_style = if app.session_input.is_empty() {
        Style::default().fg(theme.muted)
    } else {
        Style::default().fg(theme.fg)
    };

    let input_widget = Paragraph::new(Line::from(vec![
        Span::styled("User > ", Style::default().fg(theme.info)),
        Span::styled(input_text, input_style),
    ]))
    .block(input_block);

    frame.render_widget(input_widget, input_area);

    if suggestion_height > 0 && !app.session_suggestions.is_empty() {
        let suggestion_lines: Vec<Line> = app.session_suggestions.iter()
            .enumerate()
            .map(|(i, s)| {
                let prefix = if i == 0 { "💡 " } else { "   " };
                Line::from(Span::styled(
                    format!("{prefix}{s}"),
                    Style::default().fg(theme.muted),
                ))
            })
            .collect();
        let suggestion_widget = Paragraph::new(Text::from(suggestion_lines))
            .style(Style::default().bg(theme.bg_darker));
        frame.render_widget(suggestion_widget, suggestion_area);
    }
}
