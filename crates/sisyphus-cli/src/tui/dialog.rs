use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use super::background::{spawn_dev_agent, spawn_execute_sop, spawn_fetch_roadmap, spawn_list_sops};
use super::{App, DevFocus, LogEntry, ToastKind, View};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Dialog {
    SessionList,
    Todos,
    Lsp,
    Sop,
    Roadmap,
    Help,
    ConfirmDelete,
    Dev { goal: String },
    Rename { session_id: String, name: String },
}

/// Handle key events when a dialog is open.
pub fn handle_dialog_key(key: KeyEvent, app: &mut App) {
    let code = key.code;
    let Some(ref dialog) = app.dialog else { return };

    match dialog {
        Dialog::Help => {
            match code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                    app.dialog = None;
                    app.help_scroll = 0;
                }
                KeyCode::Up => {
                    app.help_scroll = app.help_scroll.saturating_sub(1);
                }
                KeyCode::Down => {
                    app.help_scroll = app.help_scroll.saturating_add(1);
                }
                KeyCode::PageUp => {
                    app.help_scroll = app.help_scroll.saturating_sub(10);
                }
                KeyCode::PageDown => {
                    app.help_scroll = app.help_scroll.saturating_add(10);
                }
                KeyCode::Home => {
                    app.help_scroll = 0;
                }
                KeyCode::End => {
                    app.help_scroll = usize::MAX; // will clamp at render
                }
                _ => {}
            }
        }
        Dialog::SessionList => handle_session_list_key(code, app),
        Dialog::Todos => handle_todos_key(code, app),
        Dialog::Lsp => handle_lsp_key(code, app),
        Dialog::Sop => handle_sop_key(code, app),
        Dialog::Roadmap => handle_roadmap_key(code, app),
        Dialog::ConfirmDelete => {
            match code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if let Some(id) = app.confirm_delete.take() {
                        let db = app.data_dir.join("sessions.json");
                        if sisyphus_tools::session_delete(&db, &id).is_ok() {
                            app.add_log(LogEntry::Success(format!("Deleted session {id}")));
                            app.session_dirty = true;
                        }
                    }
                    app.dialog = None;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    app.confirm_delete = None;
                    app.dialog = None;
                }
                _ => {}
            }
        }
        Dialog::Dev { goal: _ } => handle_dev_key(key, app),
        Dialog::Rename { session_id: _, name: _ } => handle_rename_key(code, app),
    }
}

// ── Session List Dialog ───────────────────────────────

fn handle_session_list_key(code: KeyCode, app: &mut App) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            if !app.session_filter.is_empty() {
                app.session_filter.clear();
            }
            app.dialog = None;
        }
        KeyCode::Up => app.session_sel = app.session_sel.saturating_sub(1),
        KeyCode::Down => {
            let visible = filtered_sessions(app).len();
            app.session_sel = (app.session_sel + 1).min(visible.saturating_sub(1));
        }
        KeyCode::PageUp => app.session_sel = app.session_sel.saturating_sub(10),
        KeyCode::PageDown => {
            let visible = filtered_sessions(app).len();
            app.session_sel = (app.session_sel + 10).min(visible.saturating_sub(1));
        }
        KeyCode::Home => app.session_sel = 0,
        KeyCode::End => {
            let visible = filtered_sessions(app).len();
            app.session_sel = visible.saturating_sub(1);
        }
        KeyCode::Enter => {
            select_session(app);
            app.dialog = None;
        }
        KeyCode::Backspace => {
            app.session_filter.pop();
            app.session_sel = 0;
        }
        KeyCode::Char('c') | KeyCode::Char('C') => {
            let db = app.data_dir.join("sessions.json");
            match sisyphus_tools::session_create(&db, "unnamed") {
                Ok(s) => {
                    app.add_toast(ToastKind::Success, format!("Created session: {}", s.name));
                    app.add_log(LogEntry::Success(format!("Created session {} ({})", s.id, s.name)));
                    app.session_dirty = true;
                    // Navigate to the new session
                    app.session_detail = sisyphus_tools::session_get(&db, &s.id).ok();
                    app.session_detail_scroll = usize::MAX;
                    app.view = View::Session;
                    app.dialog = None;
                }
                Err(e) => app.add_log(LogEntry::Error(format!("Create failed: {e}"))),
            }
        }
        KeyCode::Char('d') | KeyCode::Delete => {
            let filtered = filtered_sessions(app);
            if let Some(&idx) = filtered.get(app.session_sel.min(filtered.len().saturating_sub(1))) {
                if let Some(s) = app.sessions.get(idx) {
                    app.confirm_delete = Some(s.id.clone());
                    app.dialog = Some(Dialog::ConfirmDelete);
                }
            }
        }
        KeyCode::Char('r') | KeyCode::Char('R') => app.session_dirty = true,
        KeyCode::Char(c) if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' => {
            app.session_filter.push(c);
            app.session_sel = 0;
        }
        _ => {}
    }
}

fn filtered_sessions(app: &App) -> Vec<usize> {
    if app.session_filter.is_empty() {
        return (0..app.sessions.len()).collect();
    }
    let q = app.session_filter.to_lowercase();
    app.sessions
        .iter()
        .enumerate()
        .filter(|(_, s)| s.name.to_lowercase().contains(&q) || s.id.contains(&q))
        .map(|(i, _)| i)
        .collect()
}

fn select_session(app: &mut App) {
    if app.sessions.is_empty() {
        return;
    }
    let filtered = filtered_sessions(app);
    if filtered.is_empty() {
        return;
    }
    let idx = filtered[app.session_sel.min(filtered.len().saturating_sub(1))];
    let db = app.data_dir.join("sessions.json");
    let id = &app.sessions[idx].id;
    app.session_detail = sisyphus_tools::session_get(&db, id).ok();
    app.session_detail_scroll = usize::MAX;
    app.view = View::Session;
}

// ── Todos Dialog ─────────────────────────────────────

fn handle_todos_key(code: KeyCode, app: &mut App) {
    if !app.todo_adding.is_empty() {
        match code {
            KeyCode::Enter => {
                let task = app.todo_adding.trim().to_string();
                if !task.is_empty() {
                    let db = app.data_dir.join("todos.json");
                    match sisyphus_tools::todo_add(&db, &task, sisyphus_tools::TodoPriority::Medium) {
                        Ok(item) => {
                            app.add_toast(ToastKind::Success, format!("Added todo: {}", item.content));
                            app.add_log(LogEntry::Success(format!("Added todo: {}", item.content)));
                            app.todo_dirty = true;
                        }
                        Err(e) => app.add_log(LogEntry::Error(format!("Add todo failed: {e}"))),
                    }
                }
                app.todo_adding.clear();
            }
            KeyCode::Esc => app.todo_adding.clear(),
            KeyCode::Backspace => { app.todo_adding.pop(); }
            KeyCode::Char(c) => { app.todo_adding.push(c); }
            _ => {}
        }
        return;
    }

    match code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.dialog = None;
        }
        KeyCode::Up => app.todo_sel = app.todo_sel.saturating_sub(1),
        KeyCode::Down => {
            app.todo_sel = (app.todo_sel + 1).min(app.todo_items.len().saturating_sub(1));
        }
        KeyCode::Char(' ') | KeyCode::Enter => toggle_todo(app),
        KeyCode::Char('a') => app.todo_adding = String::new(),
        KeyCode::Char('d') | KeyCode::Delete => delete_todo(app),
        KeyCode::Char('r') | KeyCode::Char('R') => {
            app.todo_dirty = true;
            app.add_toast(ToastKind::Info, "Refreshed todos");
        }
        _ => {}
    }
}

fn toggle_todo(app: &mut App) {
    if app.todo_items.is_empty() {
        return;
    }
    let db = app.data_dir.join("todos.json");
    let item = &app.todo_items[app.todo_sel];
    let new_status = match item.status {
        sisyphus_tools::TodoStatus::Pending => sisyphus_tools::TodoStatus::InProgress,
        sisyphus_tools::TodoStatus::InProgress => sisyphus_tools::TodoStatus::Completed,
        sisyphus_tools::TodoStatus::Completed => sisyphus_tools::TodoStatus::Pending,
        sisyphus_tools::TodoStatus::Cancelled => sisyphus_tools::TodoStatus::Pending,
    };
    if sisyphus_tools::todo_update(&db, &item.id, None, Some(new_status.clone()), None).is_ok() {
        app.todo_dirty = true;
        app.add_toast(ToastKind::Success, format!("Todo: {:?}", new_status));
    }
}

fn delete_todo(app: &mut App) {
    if app.todo_items.is_empty() {
        return;
    }
    let id = app.todo_items[app.todo_sel].id.clone();
    let db = app.data_dir.join("todos.json");
    if sisyphus_tools::todo_delete(&db, &id).is_ok() {
        app.todo_dirty = true;
        app.add_toast(ToastKind::Info, "Deleted todo");
        app.add_log(LogEntry::Info(format!("Deleted todo {id}")));
    }
}

// ── LSP Dialog ───────────────────────────────────────

fn handle_lsp_key(code: KeyCode, app: &mut App) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.dialog = None;
        }
        KeyCode::Up => app.lsp_scroll = app.lsp_scroll.saturating_sub(1),
        KeyCode::Down => app.lsp_scroll = app.lsp_scroll.saturating_add(1),
        KeyCode::PageUp => app.lsp_scroll = app.lsp_scroll.saturating_sub(10),
        KeyCode::PageDown => app.lsp_scroll = app.lsp_scroll.saturating_add(10),
        KeyCode::Char('1') => {
            app.lsp_language = "rust".into();
            app.lsp_server = "rust-analyzer".into();
            app.add_toast(ToastKind::Info, "LSP: Rust (rust-analyzer)");
        }
        KeyCode::Char('2') => {
            app.lsp_language = "typescript".into();
            app.lsp_server = "typescript-language-server".into();
            app.add_toast(ToastKind::Info, "LSP: TypeScript");
        }
        KeyCode::Char('3') => {
            app.lsp_language = "python".into();
            app.lsp_server = "pylsp".into();
            app.add_toast(ToastKind::Info, "LSP: Python (pylsp)");
        }
        KeyCode::Enter => {
            app.lsp_status = "running".into();
            app.add_toast(ToastKind::Info, "LSP analysis started...");
        }
        KeyCode::Backspace => { app.dialog_query.pop(); }
        KeyCode::Char(c) if !c.is_control() => { app.dialog_query.push(c); }
        _ => {}
    }
}

// ── SOP Dialog ────────────────────────────────────────

fn handle_sop_key(code: KeyCode, app: &mut App) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.dialog = None;
        }
        KeyCode::Up => app.sop_sel = app.sop_sel.saturating_sub(1),
        KeyCode::Down => {
            let filtered = filtered_sops(app);
            if !filtered.is_empty() {
                app.sop_sel = (app.sop_sel + 1).min(filtered.len().saturating_sub(1));
            }
        }
        KeyCode::Enter => {
            let sop_id = filtered_sops(app).get(app.sop_sel).map(|s| s.sop_id.clone());
            let Some(sop_id) = sop_id else { return };
            app.sop_status = format!("executing {sop_id}...");
            app.sop_result = None;
            app.add_toast(ToastKind::Info, format!("Executing SOP: {sop_id}"));
            if app.pending_op.is_none() {
                app.pending_op = Some(spawn_execute_sop(app.bridge_address.clone(), sop_id));
            }
        }
        KeyCode::Char('r') | KeyCode::Char('R') => {
            app.sop_status = "connecting...".into();
            app.sop_result = None;
            app.add_toast(ToastKind::Info, "Refreshing SOP list...");
            if app.pending_op.is_none() {
                app.pending_op = Some(spawn_list_sops(app.bridge_address.clone()));
            }
        }
        KeyCode::Backspace => { app.sop_query.pop(); }
        KeyCode::Char(c) if !c.is_control() => { app.sop_query.push(c); }
        _ => {}
    }
}

fn filtered_sops(app: &App) -> Vec<&crate::bridge::SopInfo> {
    if app.sop_query.is_empty() {
        return app.sop_sops.iter().collect();
    }
    let q = app.sop_query.to_lowercase();
    app.sop_sops.iter().filter(|s| s.sop_id.to_lowercase().contains(&q)).collect()
}

// ── Roadmap Dialog ────────────────────────────────────

fn handle_roadmap_key(code: KeyCode, app: &mut App) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.dialog = None;
        }
        KeyCode::Up => app.roadmap_scroll = app.roadmap_scroll.saturating_sub(1),
        KeyCode::Down => app.roadmap_scroll = app.roadmap_scroll.saturating_add(1),
        KeyCode::PageUp => app.roadmap_scroll = app.roadmap_scroll.saturating_sub(10),
        KeyCode::PageDown => app.roadmap_scroll = app.roadmap_scroll.saturating_add(10),
        KeyCode::Home | KeyCode::Char('g') => app.roadmap_scroll = 0,
        KeyCode::End | KeyCode::Char('G') => app.roadmap_scroll = usize::MAX,
        KeyCode::Char('r') | KeyCode::Char('R') => {
            app.roadmap_status = "connecting...".into();
            if app.pending_op.is_none() {
                app.pending_op = Some(spawn_fetch_roadmap(app.bridge_address.clone()));
            }
        }
        _ => {}
    }
}

// ── Rename Session Dialog ────────────────────────────

fn handle_rename_key(code: KeyCode, app: &mut App) {
    let Some(Dialog::Rename { session_id, name }) = &app.dialog else { return };
    let session_id = session_id.clone();
    match code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.dialog = None;
        }
        KeyCode::Enter => {
            let new_name = name.trim().to_string();
            if !new_name.is_empty() {
                let db = app.data_dir.join("sessions.json");
                match sisyphus_tools::session_rename(&db, &session_id, &new_name) {
                    Ok(_) => {
                        app.add_toast(ToastKind::Success, format!("Renamed → {new_name}"));
                        app.add_log(LogEntry::Success(format!("Renamed session {session_id} → {new_name}")));
                        app.session_dirty = true;
                    }
                    Err(e) => app.add_log(LogEntry::Error(format!("Rename failed: {e}")),
                    )
                }
            }
            app.dialog = None;
        }
        KeyCode::Backspace => {
            if let Some(Dialog::Rename { name, .. }) = &mut app.dialog {
                name.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Some(Dialog::Rename { name, .. }) = &mut app.dialog {
                name.push(c);
            }
        }
        _ => {}
    }
}

// ── Dev Dialog ────────────────────────────────────────

fn handle_dev_key(key: KeyEvent, app: &mut App) {
    let code = key.code;
    // If report is available, any key dismisses it (user can Esc to close)
    if app.dev_report.is_some() {
        match code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                app.dialog = None;
                app.dev_report = None;
                app.dev_status = "idle".into();
            }
            _ => {}
        }
        return;
    }

    // If running, only Esc cancels (sets status to idle)
    if app.dev_status == "running" {
        if code == KeyCode::Esc {
            app.dev_status = "idle".into();
        }
        return;
    }

    match code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.dialog = None;
            app.dev_goal.clear();
            app.dev_context.clear();
            app.dev_parallel = false;
            app.dev_resume = false;
            app.dev_report = None;
            app.dev_status = "idle".into();
            app.dev_focus = DevFocus::Goal;
        }
        KeyCode::Tab => {
            app.dev_focus = match app.dev_focus {
                DevFocus::Goal => DevFocus::Context,
                DevFocus::Context => DevFocus::Parallel,
                DevFocus::Parallel => DevFocus::Resume,
                DevFocus::Resume => DevFocus::Run,
                DevFocus::Run => DevFocus::Goal,
            };
        }
        KeyCode::BackTab => {
            app.dev_focus = match app.dev_focus {
                DevFocus::Goal => DevFocus::Run,
                DevFocus::Context => DevFocus::Goal,
                DevFocus::Parallel => DevFocus::Context,
                DevFocus::Resume => DevFocus::Parallel,
                DevFocus::Run => DevFocus::Resume,
            };
        }
        KeyCode::Enter => {
            if app.dev_focus == DevFocus::Run {
                let goal = app.dev_goal.trim().to_string();
                if goal.is_empty() {
                    app.add_log(LogEntry::Warn("Dev goal cannot be empty".into()));
                    return;
                }
                app.dev_status = "running".into();
                app.add_toast(ToastKind::Info, format!("Dev agent started: {goal}"));
                let context = app.dev_context.clone();
                let parallel = app.dev_parallel;
                let resume = app.dev_resume;
                if app.pending_op.is_none() {
                    let mut config = sisyphus_types::Config::default();
                    config.apply_env_overrides();
                    app.pending_op = Some(spawn_dev_agent(
                        config, goal, context, parallel, resume,
                    ));
                }
            }
        }
        KeyCode::Backspace => {
            match app.dev_focus {
                DevFocus::Goal => { app.dev_goal.pop(); }
                DevFocus::Context => { app.dev_context.pop(); }
                _ => {}
            }
        }
        KeyCode::Char(' ') => {
            match app.dev_focus {
                DevFocus::Parallel => app.dev_parallel = !app.dev_parallel,
                DevFocus::Resume => app.dev_resume = !app.dev_resume,
                DevFocus::Goal => app.dev_goal.push(' '),
                DevFocus::Context => app.dev_context.push(' '),
                DevFocus::Run => {} // space on Run is no-op
            }
        }
        KeyCode::Char(c) => {
            match app.dev_focus {
                DevFocus::Goal => app.dev_goal.push(c),
                DevFocus::Context => app.dev_context.push(c),
                _ => {}
            }
        }
        _ => {}
    }
}

// ── Render ────────────────────────────────────────────

pub fn render_dialog_overlay(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let Some(ref dialog) = app.dialog else { return };

    match dialog {
        Dialog::Help => render_help_dialog(frame, area, app),
        Dialog::SessionList => render_session_list_dialog(frame, area, app),
        Dialog::Todos => render_todos_dialog(frame, area, app),
        Dialog::Lsp => render_lsp_dialog(frame, area, app),
        Dialog::Sop => render_sop_dialog(frame, area, app),
        Dialog::Roadmap => render_roadmap_dialog(frame, area, app),
        Dialog::ConfirmDelete => render_confirm_dialog(frame, area, app),
        Dialog::Dev { goal: _goal } => render_dev_dialog(frame, area, app),
        Dialog::Rename { session_id: _, name } => render_rename_dialog(frame, area, app, name),
    }
}

fn render_help_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
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
        format!(" ↑↓/scroll: {} | Esc/q to close ", helpers_scroll_pct(scroll, max_scroll as usize))
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

/// Minimal scroll-pos formatter (inline to avoid cross-module dep on helpers).
fn helpers_scroll_pct(pos: usize, total: usize) -> String {
    if total <= 1 { String::new() } else {
        let pct = (pos as f64 / (total.saturating_sub(1) as f64) * 100.0).round() as u8;
        format!("{pos}/{total} ({pct}%)")
    }
}

fn render_session_list_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Sessions ")
        .border_style(Style::default().fg(theme.primary));

    let items: Vec<ListItem> = app
        .sessions
        .iter()
        .map(|s| {
            let line = Line::from(vec![
                Span::styled(&s.name, Style::default().fg(theme.fg)),
                Span::styled(
                    format!(" ({} msgs)", s.message_count),
                    Style::default().fg(theme.muted),
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().fg(theme.secondary).add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(
        list,
        area,
        &mut ratatui::widgets::ListState::default().with_selected(Some(app.session_sel)),
    );
}

fn render_todos_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
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

fn render_lsp_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
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

fn render_sop_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let filtered = filtered_sops(app);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" SOP List ")
        .border_style(Style::default().fg(theme.primary));

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

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().fg(theme.secondary).add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(
        list,
        area,
        &mut ratatui::widgets::ListState::default().with_selected(Some(app.sop_sel)),
    );
}

fn render_roadmap_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
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

fn render_confirm_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
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

fn render_rename_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App, name: &str) {
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

fn render_dev_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
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
