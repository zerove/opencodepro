use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use unicode_width::UnicodeWidthStr;
use super::background::spawn_session_add_message;
use super::dialog::Dialog;
use super::helpers::{copy_to_clipboard, render_message_with_thought_section, scroll_pct};
use super::{App, DevFocus, LogEntry, Mode, ToastKind};

// === Cursor-aware input editing helpers ===

fn cursor_char_to_byte(s: &str, char_idx: usize) -> usize {
    s.chars().take(char_idx).map(|c| c.len_utf8()).sum()
}

fn cursor_insert(s: &mut String, cursor: &mut usize, c: char) {
    let byte_idx = cursor_char_to_byte(s, *cursor);
    s.insert(byte_idx, c);
    *cursor += 1;
}

fn cursor_backspace(s: &mut String, cursor: &mut usize) {
    if *cursor > 0 {
        *cursor -= 1;
        let byte_idx = cursor_char_to_byte(s, *cursor);
        s.remove(byte_idx);
    }
}

fn cursor_delete(s: &mut String, cursor: usize) {
    if let Some((byte_idx, _)) = s.char_indices().nth(cursor) {
        s.remove(byte_idx);
    }
}

fn cursor_delete_word_backward(s: &mut String, cursor: &mut usize) {
    if *cursor == 0 { return; }
    let chars: Vec<char> = s.chars().collect();
    let mut end = *cursor;
    while end > 0 && chars[end - 1].is_whitespace() { end -= 1; }
    while end > 0 && !chars[end - 1].is_whitespace() { end -= 1; }
    let byte_start: usize = chars[..end].iter().map(|c| c.len_utf8()).sum();
    let byte_end: usize = chars[..*cursor].iter().map(|c| c.len_utf8()).sum();
    s.drain(byte_start..byte_end);
    *cursor = end;
}

fn cursor_delete_to_end(s: &mut String, cursor: usize) {
    let byte_start = cursor_char_to_byte(s, cursor);
    s.truncate(byte_start);
}

// === Slash command dispatch ===
fn update_slash_suggestions(app: &mut App) {
    let input = app.session_input.trim();
    if input.starts_with('/') {
        let rest = input.strip_prefix('/').unwrap_or("").to_lowercase();
        let all_commands = [
            ("/help", "Open help dialog"),
            ("/dev [goal]", "Open dev dialog"),
            ("/home", "Go to home"),
            ("/sessions", "Go to sessions"),
            ("/clear", "Clear input"),
            ("/new", "Create new session"),
        ];
        let matches: Vec<String> = all_commands.iter()
            .filter(|(cmd, _)| cmd.starts_with(&format!("/{rest}")) || rest.is_empty())
            .map(|(cmd, desc)| format!("{cmd:<12} {desc}"))
            .collect();
        app.session_suggestions = matches;
    } else {
        // Restore default suggestions
        app.session_suggestions = vec![
            "Type /help for keyboard shortcuts".into(),
            "Type /dev to open dev dialog".into(),
            "Try Ctrl+U/W/A/E for input editing".into(),
        ];
    }
}

fn dispatch_slash_command(app: &mut App, cmd: &str) -> bool {
    let (cmd_name, args) = cmd.split_once(' ').unwrap_or((cmd, ""));
    match cmd_name {
        "/help" | "/h" => {
            app.dialog = Some(Dialog::Help);
            true
        }
        "/dev" | "/d" => {
            let goal = if args.is_empty() { String::new() } else { args.trim().to_string() };
            app.dev_goal = goal.clone();
            app.dev_context.clear();
            app.dev_parallel = false;
            app.dev_resume = false;
            app.dev_report = None;
            app.dev_status = "idle".into();
            app.dev_focus = DevFocus::Goal;
            app.dialog = Some(Dialog::Dev { goal });
            true
        }
        "/home" => {
            app.view = super::View::Home;
            true
        }
        "/sessions" => {
            app.view = super::View::Session;
            true
        }
        "/clear" | "/c" => {
            app.session_input.clear();
            app.session_cursor = 0;
            true
        }
        "/new" | "/n" => {
            let db = app.data_dir.join("sessions.json");
            match sisyphus_tools::session_create(&db, "unnamed") {
                Ok(s) => {
                    app.add_log(LogEntry::Success(format!("Created session {} ({})", s.id, s.name)));
                    app.session_dirty = true;
                }
                Err(e) => app.add_log(LogEntry::Error(format!("Create failed: {e}"))),
            }
            true
        }
        _ => false,
    }
}

pub fn handle_sessions_key(key: KeyEvent, app: &mut App) {
    let code = key.code;
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    app.refresh_sessions();

    if app.session_detail.is_some() {
        match app.session_mode {
            Mode::Input => match code {
                KeyCode::Enter => {
                    let content = app.session_input.trim().to_string();
                    let was_slash = content.starts_with('/');

                    if was_slash {
                        let handled = dispatch_slash_command(app, &content);
                        if handled {
                            app.session_input.clear();
                            app.session_cursor = 0;
                            app.session_mode = Mode::Select;
                            return;
                        }
                    }

                    if let Some(ref mut session) = app.session_detail {
                        if !content.is_empty() {
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs().to_string())
                                .unwrap_or_default();
                            app.history.push(content.clone());
                            app.history_idx = None;
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
                            app.session_cursor = 0;
                        }
                    }
                    app.session_mode = Mode::Select;
                }
                // === Cancel / Exit input ===
                KeyCode::Esc => {
                    app.session_mode = Mode::Select;
                    app.session_input.clear();
                    app.session_cursor = 0;
                }
                // === Basic editing ===
                KeyCode::Backspace => {
                    cursor_backspace(&mut app.session_input, &mut app.session_cursor);
                    update_slash_suggestions(app);
                }
                KeyCode::Delete => {
                    cursor_delete(&mut app.session_input, app.session_cursor);
                    update_slash_suggestions(app);
                }
                // === Arrow navigation ===
                KeyCode::Left => app.session_cursor = app.session_cursor.saturating_sub(1),
                KeyCode::Right => app.session_cursor = (app.session_cursor + 1).min(app.session_input.len()),
                KeyCode::Home => app.session_cursor = 0,
                KeyCode::End => app.session_cursor = app.session_input.len(),
                // === History cycling (↑/↓) ===
                KeyCode::Up => {
                    if !app.history.is_empty() {
                        let idx = match app.history_idx {
                            Some(i) if i > 0 => i - 1,
                            None => app.history.len() - 1,
                            _ => return,
                        };
                        app.history_idx = Some(idx);
                        app.session_input = app.history[idx].clone();
                        app.session_cursor = app.session_input.len();
                        update_slash_suggestions(app);
                    }
                }
                KeyCode::Down => {
                    if let Some(idx) = app.history_idx {
                        if idx + 1 < app.history.len() {
                            app.history_idx = Some(idx + 1);
                            app.session_input = app.history[idx + 1].clone();
                        } else {
                            app.history_idx = None;
                            app.session_input.clear();
                        }
                        app.session_cursor = app.session_input.len();
                        update_slash_suggestions(app);
                    }
                }
                // === Ctrl+ combos ===
                KeyCode::Char('a') if ctrl => app.session_cursor = 0,
                KeyCode::Char('e') if ctrl => app.session_cursor = app.session_input.len(),
                KeyCode::Char('u') if ctrl => {
                    app.session_input.clear();
                    app.session_cursor = 0;
                    update_slash_suggestions(app);
                }
                KeyCode::Char('w') if ctrl => {
                    cursor_delete_word_backward(&mut app.session_input, &mut app.session_cursor);
                    update_slash_suggestions(app);
                }
                KeyCode::Char('k') if ctrl => {
                    cursor_delete_to_end(&mut app.session_input, app.session_cursor);
                    update_slash_suggestions(app);
                }
                // === Regular char input ===
                KeyCode::Char(c) => {
                    cursor_insert(&mut app.session_input, &mut app.session_cursor, c);
                    update_slash_suggestions(app);
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
                    app.adjust_session_scroll(-1);
                }
                KeyCode::Down => {
                    app.adjust_session_scroll(1);
                }
                KeyCode::PageUp => {
                    app.adjust_session_scroll(-10);
                }
                KeyCode::PageDown => {
                    app.adjust_session_scroll(10);
                }
                KeyCode::Tab => {
                    app.toggle_all_thoughts();
                    let expanded = app.expanded_thoughts.len();
                    let total = app.session_detail.as_ref().map_or(0, |s| s.messages.len());
                    app.add_log(LogEntry::Info(
                        format!("Thought blocks: {expanded}/{total} expanded"),
                    ));
                }
                KeyCode::Char('t') => {
                    if let Some(ref detail) = app.session_detail {
                        if let Some(idx) = detail.messages.iter().rposition(|m| m.role == "assistant") {
                            app.selected_msg = Some(idx);
                            app.toggle_message_thoughts(idx);
                            let state = if app.is_thought_expanded(idx) { "expanded" } else { "collapsed" };
                            app.add_log(LogEntry::Info(
                                format!("Toggled thought blocks for message {idx} ({state})"),
                            ));
                        } else {
                            app.add_log(LogEntry::Info("No assistant messages to toggle".into()));
                        }
                    }
                }
                KeyCode::Home | KeyCode::Char('g') => {
                    app.session_detail_scroll = 0;
                }
                KeyCode::End | KeyCode::Char('G') => {
                    app.session_detail_scroll = usize::MAX;
                }
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    // Copy selected message; fall back to last assistant if no selection
                    if let Some(ref detail) = app.session_detail {
                        let content = app.selected_msg
                            .and_then(|i| detail.messages.get(i))
                            .map(|m| m.content.as_str())
                            .or_else(|| {
                                detail.messages.iter().rev().find(|m| m.role == "assistant").map(|m| m.content.as_str())
                            });
                        if let Some(content) = content {
                            match copy_to_clipboard(content) {
                                Ok(msg) => {
                                    app.add_toast(ToastKind::Success, &msg);
                                    app.add_log(LogEntry::Success(msg));
                                }
                                Err(e) => app.add_log(LogEntry::Error(e)),
                            }
                        } else {
                            app.add_log(LogEntry::Warn("No message to copy".into()));
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
                        match copy_to_clipboard(&all_text) {
                            Ok(msg) => {
                                app.add_toast(ToastKind::Success, &msg);
                                app.add_log(LogEntry::Success(msg));
                            }
                            Err(e) => app.add_log(LogEntry::Error(e)),
                        }
                    }
                }
                // ── Search (Ctrl+F) ──
                KeyCode::Char('f') if ctrl => {
                    if app.search_active {
                        app.search_active = false;
                        app.search_query.clear();
                        app.search_results.clear();
                    } else {
                        app.search_active = true;
                    }
                }
                // ── Search active: char input, navigate, exit ──
                _ if app.search_active => {
                    app.search_active = false; // will re-set below if still wanted
                    match key.code {
                        KeyCode::Esc => {
                            app.search_query.clear();
                            app.search_results.clear();
                        }
                        KeyCode::Char(c) => {
                            app.search_query.push(c);
                            app.search_active = true;
                            app.perform_search();
                        }
                        KeyCode::Backspace => {
                            app.search_query.pop();
                            app.search_active = !app.search_query.is_empty();
                            app.perform_search();
                        }
                        KeyCode::Enter => {
                            app.search_active = true;
                            app.next_search_match();
                        }
                        KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => {
                            app.search_active = true;
                            app.prev_search_match();
                        }
                        KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => {
                            app.search_active = true;
                            app.next_search_match();
                        }
                        _ => {
                            // deactivate search on any other key
                            app.search_query.clear();
                            app.search_results.clear();
                        }
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
            let session_id = app.sessions[idx].id.clone();
            let name = app.sessions[idx].name.clone();
            app.dialog = Some(Dialog::Rename { session_id, name });
        }
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let filtered = app.filtered_sessions();
            if let Some(&idx) = filtered.get(app.session_sel.min(filtered.len().saturating_sub(1))) {
                if let Some(s) = app.sessions.get(idx) {
                    let text = format!("{} ({}) — {} msgs", s.name, s.id, s.message_count);
                    match copy_to_clipboard(&text) {
                        Ok(msg) => {
                            app.add_toast(ToastKind::Success, &msg);
                            app.add_log(LogEntry::Success(msg));
                        }
                        Err(e) => app.add_log(LogEntry::Error(e)),
                    }
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
        suggestion_count
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

    let time_str = if session.created_at.len() >= 16 {
        &session.created_at[11..16]
    } else {
        &session.created_at
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} — {} msgs (created {}) ", session.name, session.message_count, time_str))
            .border_style(Style::default().fg(theme.border));

    let mut lines = Vec::new();

    for (idx, msg) in session.messages.iter().enumerate() {
        let is_sel = app.selected_msg == Some(idx);
        let sel_prefix = if is_sel { "▸ " } else { "  " };
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
        let sel_style = if is_sel {
            Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.muted)
        };
        lines.push(Line::from(vec![
            Span::styled(sel_prefix, sel_style),
            Span::styled(format!("[{}] ", role_label), role_style),
            Span::styled(ts, ts_style),
        ]));
        // Render thought block (collapsible) + real answer (always visible)
        render_message_with_thought_section(&mut lines, &msg.content, app.is_thought_expanded(idx));
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

    let visible_height = history_area.height.saturating_sub(2);

    // Estimate display rows accounting for word-wrapping.
    // Without this, max_scroll undercounts when wrapping creates more display rows than logical lines.
    let text_area_width = history_area.width.saturating_sub(2) as usize;
    let estimated_display_rows: u16 = if text_area_width == 0 {
        lines.len() as u16
    } else {
        lines.iter().map(|l| {
            let w = l.width();
            if w == 0 {
                1u16
            } else {
                w.div_ceil(text_area_width) as u16
            }
        }).sum()
    };

    let max_scroll = estimated_display_rows.saturating_sub(visible_height);
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

    // Store layout info for click-to-message mapping and scroll clamping
    app.last_detail_area.set(history_area);
    app.last_effective_scroll.set(effective_scroll);
    app.last_detail_max_scroll.set(max_scroll as usize);

    let mode_tag = match app.session_mode {
        Mode::Input => " [INPUT]",
        Mode::Select => " [NORMAL]",
    };
    let input_title: String = if app.pending_op.is_some() {
        " ⏳ 等待回复... ".into()
    } else {
        format!(" Chat{mode_tag} ")
    };
    let input_block = Block::default()
        .borders(Borders::ALL)
        .title(input_title)
        .border_style(if app.session_mode == Mode::Input {
            Style::default().fg(theme.emphasized)
        } else if app.pending_op.is_some() {
            Style::default().fg(theme.error)
        } else {
            Style::default().fg(theme.muted)
        });

    let input_text: &str = if !app.session_input.is_empty() {
        &app.session_input
    } else if app.session_mode == Mode::Input {
        "Type your message and press Enter to send..."
    } else {
        "Press I or Enter to type, Esc to go back"
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

    if app.session_mode == Mode::Input && !app.session_input.is_empty() {
        let prefix_width = 7;
        let char_count = app.session_input.chars().count();
        let text_before: String = app.session_input.chars().take(app.session_cursor.min(char_count)).collect();
        let cursor_display = text_before.width() as u16;
        frame.set_cursor_position((
            input_area.x + 1 + prefix_width + cursor_display,
            input_area.y + 1,
        ));
    }

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

    // ── Search overlay ──
    if app.search_active {
        let search_area = Rect {
            x: history_area.x + 1,
            y: history_area.y + history_area.height.saturating_sub(2),
            width: history_area.width.saturating_sub(2),
            height: 2,
        };
        let match_info = if app.search_results.is_empty() {
            if app.search_query.is_empty() {
                " Type to search...".to_string()
            } else {
                " No matches".to_string()
            }
        } else {
            format!(" {} / {} ", app.search_idx + 1, app.search_results.len())
        };
        let query_display = if app.search_query.is_empty() {
            Span::raw("")
        } else {
            Span::styled(
                &app.search_query,
                Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
            )
        };
        let search_fg = if app.search_results.is_empty() && !app.search_query.is_empty() {
            theme.error
        } else {
            theme.info
        };
        let search_bar = Paragraph::new(Line::from(vec![
            Span::styled(" 🔍 ", Style::default().fg(search_fg)),
            query_display,
            Span::styled(match_info, Style::default().fg(theme.muted)),
        ]))
        .style(Style::default().bg(theme.bg_darker));
        frame.render_widget(search_bar, search_area);
    }
}
