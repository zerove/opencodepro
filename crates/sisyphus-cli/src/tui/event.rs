use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, MouseButton, MouseEventKind};

use super::app::{matching_palette_entries, App, DevFocus, LogEntry, View};
use super::background::{spawn_fetch_available_skills, spawn_fetch_roadmap, spawn_list_sops};
use super::dialog::{handle_dialog_key, Dialog};
use super::helpers::paste_from_clipboard;
use super::home::handle_home_key;
use super::sessions::handle_sessions_key;
use crate::permission::CliPermission;

// ── Event handler ───────────────────────────────────────

pub(crate) fn handle_event(event: Event, app: &mut App, _perms: &CliPermission) -> Result<bool> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            app.notification = None; // Clear notification on any key press

            // Global: Ctrl+C / Ctrl+Q → quick exit (double-tap to confirm)
            if (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('q'))
                && key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
            {
                if app.exit_requested {
                    return Ok(true);
                }
                app.exit_requested = true;
                app.add_log(LogEntry::Warn(
                    "Press Ctrl+C / Ctrl+Q again to confirm exit.".into(),
                ));
                return Ok(false);
            }

            // Global: Ctrl+K → toggle command palette (opencode-style)
            if key.code == KeyCode::Char('k')
                && key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
            {
                app.palette_open = !app.palette_open;
                if app.palette_open {
                    app.palette_query.clear();
                    app.palette_sel = 0;
                }
                return Ok(false);
            }

            // Global: Ctrl+L → session list dialog
            if key.code == KeyCode::Char('l')
                && key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
            {
                app.dialog = Some(Dialog::SessionList);
                app.dialog_sel = 0;
                app.dialog_query.clear();
                app.session_dirty = true;
                return Ok(false);
            }

            // Global: Ctrl+M → model info (placeholder for model list)
            if key.code == KeyCode::Char('m')
                && key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
            {
                let name = if app.model_name.is_empty() {
                    "default"
                } else {
                    &app.model_name
                };
                app.add_log(LogEntry::Info(format!("Current model: {name}")));
                return Ok(false);
            }

            // Global: Ctrl+A → agent info (placeholder for agent list)
            if key.code == KeyCode::Char('a')
                && key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
            {
                app.add_log(LogEntry::Info(
                    "Agent routing managed by bridge. See Config for profiles.".into(),
                ));
                return Ok(false);
            }

            // Global: F1 → help dialog
            if key.code == KeyCode::F(1) {
                app.dialog = Some(Dialog::Help);
                return Ok(false);
            }

            // Global: Ctrl+H → go home
            if key.code == KeyCode::Char('h')
                && key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
            {
                app.view = View::Home;
                app.dialog = None;
                app.boulder_dirty = true;
                return Ok(false);
            }

            // Global: Ctrl+D → toggle dashboard
            if key.code == KeyCode::Char('d')
                && key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
            {
                app.view = View::Dashboard;
                app.dialog = None;
                app.boulder_dirty = true;
                return Ok(false);
            }

            // Global: Ctrl+N → new session
            if key.code == KeyCode::Char('n')
                && key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
            {
                let db = app.data_dir.join("sessions.json");
                match crate::tools::session_create(&db, "unnamed") {
                    Ok(s) => {
                        app.add_log(LogEntry::Success(format!("Created session {}", s.name)));
                        match crate::tools::session_get(&db, &s.id) {
                            Ok(session) => app.session_detail = Some(session),
                            Err(e) => app.add_log(LogEntry::Error(format!(
                                "Session created but load failed: {e}"
                            ))),
                        }
                        app.session_detail_scroll = usize::MAX;
                        app.view = View::Session;
                        app.boulder_dirty = true;
                    }
                    Err(e) => app.add_log(LogEntry::Error(format!("Create failed: {e}"))),
                }
                return Ok(false);
            }

            // Global: Ctrl+V → paste
            if key.code == KeyCode::Char('v')
                && key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
            {
                let pasted = paste_from_clipboard();
                if !pasted.is_empty() {
                    match app.view {
                        View::Home => {
                            let byte_pos = app
                                .home_input
                                .char_indices()
                                .nth(app.home_cursor)
                                .map(|(i, _)| i)
                                .unwrap_or(app.home_input.len());
                            app.home_input.insert_str(byte_pos, &pasted);
                            app.home_cursor += pasted.chars().count();
                        }
                        View::Dashboard => {}
                        View::Session if app.session_detail.is_some() => {
                            app.session_input.push_str(&pasted);
                        }
                        View::Session => {}
                    }
                }
                return Ok(false);
            }

            // Leader key: Ctrl+X → leader mode
            if key.code == KeyCode::Char('x')
                && key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
                && !app.leader_mode
            {
                app.leader_mode = true;
                return Ok(false);
            }

            if app.leader_mode {
                match key.code {
                    KeyCode::Char('h') => {
                        app.leader_mode = false;
                        app.dialog = Some(Dialog::Help);
                        return Ok(false);
                    }
                    KeyCode::Char('p') => {
                        app.leader_mode = false;
                        app.palette_open = true;
                        app.palette_query.clear();
                        app.palette_sel = 0;
                        return Ok(false);
                    }
                    KeyCode::Esc => {
                        app.leader_mode = false;
                        return Ok(false);
                    }
                    _ => {
                        app.leader_mode = false;
                    }
                }
            }

            // Dialog takes priority over view routing
            if app.dialog.is_some() {
                handle_dialog_key(key, app);
                return Ok(false);
            }

            // When palette is open, route all keys to palette handler
            if app.palette_open {
                handle_palette_key(key, app);
                return Ok(false);
            }

            // Route by current view
            match app.view {
                View::Home => handle_home_key(key, app),
                View::Session => handle_sessions_key(key, app),
                View::Dashboard => crate::tui::dashboard::handle_dashboard_key(key, app),
            }

            if app.exit_immediately {
                return Ok(true);
            }
        }
        Event::Mouse(m) => match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if app.view == View::Session && app.session_detail.is_some() {
                    if let Some(msg_idx) = app.find_message_at_row(m.row) {
                        app.selected_msg = Some(msg_idx);
                        app.toggle_message_thoughts(msg_idx);
                    }
                }
            }
            MouseEventKind::ScrollDown => {
                if app.dialog.is_some() {
                    if matches!(app.dialog, Some(Dialog::Help)) {
                        app.help_scroll = app.help_scroll.saturating_add(3);
                    } else {
                        app.dialog_sel = app.dialog_sel.saturating_add(1);
                    }
                } else if app.view == View::Session {
                    if app.session_detail.is_some() {
                        app.adjust_session_scroll(3);
                        app.boulder_dirty = true;
                    } else {
                        let visible = app.filtered_sessions().len();
                        app.session_sel = (app.session_sel + 1).min(visible.saturating_sub(1));
                    }
                }
            }
            MouseEventKind::ScrollUp => {
                if app.dialog.is_some() {
                    if matches!(app.dialog, Some(Dialog::Help)) {
                        app.help_scroll = app.help_scroll.saturating_sub(3);
                    } else {
                        app.dialog_sel = app.dialog_sel.saturating_sub(1);
                    }
                } else if app.view == View::Session {
                    if app.session_detail.is_some() {
                        app.adjust_session_scroll(-3);
                        app.boulder_dirty = true;
                    } else {
                        app.session_sel = app.session_sel.saturating_sub(1);
                    }
                }
            }
            _ => {}
        },
        Event::Resize(_, _) => {} // ratatui handles redraw automatically
        _ => {}
    }
    Ok(false)
}

fn handle_palette_key(key: KeyEvent, app: &mut App) {
    let code = key.code;
    let matches = matching_palette_entries(&app.palette_query);
    match code {
        KeyCode::Esc => {
            app.palette_open = false;
        }
        KeyCode::Char(c) => {
            app.palette_query.push(c);
            app.palette_sel = 0;
        }
        KeyCode::Backspace => {
            app.palette_query.pop();
            app.palette_sel = 0;
        }
        KeyCode::Up => {
            if !matches.is_empty() {
                app.palette_sel = app.palette_sel.saturating_sub(1);
            }
        }
        KeyCode::Down => {
            if !matches.is_empty() {
                app.palette_sel = (app.palette_sel + 1).min(matches.len().saturating_sub(1));
            }
        }
        KeyCode::Enter => {
            if matches.is_empty() {
                return;
            }
            let sel = app.palette_sel.min(matches.len().saturating_sub(1));
            let entry = matches[sel];
            app.palette_open = false;

            match entry.command {
                "help" | ":help" => {
                    app.dialog = Some(Dialog::Help);
                }
                "home" | ":home" => {
                    app.view = View::Home;
                    app.dialog = None;
                    app.boulder_dirty = true;
                }
                "sessions" | "session list" | ":sessions" => {
                    app.dialog = Some(Dialog::SessionList);
                    app.dialog_sel = 0;
                    app.dialog_query.clear();
                    app.session_dirty = true;
                }
                "new session" | ":new" | ":create" => {
                    let db = app.data_dir.join("sessions.json");
                    match crate::tools::session_create(&db, "unnamed") {
                        Ok(s) => {
                            app.add_log(LogEntry::Success(format!("Created session {}", s.name)));
                            match crate::tools::session_get(&db, &s.id) {
                                Ok(session) => app.session_detail = Some(session),
                                Err(e) => app.add_log(LogEntry::Error(format!(
                                    "Session created but load failed: {e}"
                                ))),
                            }
                            app.session_detail_scroll = usize::MAX;
                            app.view = View::Session;
                            app.boulder_dirty = true;
                        }
                        Err(e) => app.add_log(LogEntry::Error(format!("Create failed: {e}"))),
                    }
                }
                "todos" | ":todos" => {
                    app.dialog = Some(Dialog::Todos);
                    app.refresh_todos();
                    app.todo_sel = 0;
                }
                "lsp" | ":lsp" => {
                    app.dialog = Some(Dialog::Lsp);
                    app.dialog_query.clear();
                    app.lsp_scroll = 0;
                }
                "sops" | ":sops" | "sop" | ":sop" => {
                    app.dialog = Some(Dialog::Sop);
                    app.sop_status = "connecting...".into();
                    app.sop_result = None;
                    if app.pending_op.is_none() {
                        app.pending_op = Some(spawn_list_sops(
                            app.bridge_client.clone(),
                            app.bridge_address.clone(),
                        ));
                    }
                }
                "roadmap" | ":roadmap" => {
                    app.dialog = Some(Dialog::Roadmap);
                    app.roadmap_status = "connecting...".into();
                    if app.pending_op.is_none() {
                        app.pending_op = Some(spawn_fetch_roadmap(
                            app.bridge_client.clone(),
                            app.bridge_address.clone(),
                        ));
                    }
                }
                "skills" | ":skills" => {
                    app.dialog = Some(Dialog::SkillMarket);
                    app.skill_market_status = "connecting...".into();
                    if app.pending_op.is_none() {
                        app.pending_op =
                            Some(spawn_fetch_available_skills(app.bridge_address.clone()));
                    }
                }
                "workspaces" | ":workspaces" => {
                    app.dialog = Some(Dialog::WorkspaceList);
                    app.workspace_status = "connecting...".into();
                    if app.pending_op.is_none() {
                        app.pending_op = Some(super::background::spawn_list_workspaces(
                            app.bridge_address.clone(),
                        ));
                    }
                }
                "clear" | ":clear" => {
                    app.log.clear();
                    app.scroll = 0;
                }
                "exit" | ":exit" | "quit" | ":quit" => {
                    app.exit_requested = true;
                    app.add_log(LogEntry::Warn(
                        "Type `exit` again to confirm, or `clear` to dismiss.".into(),
                    ));
                }
                cmd if cmd.starts_with(":bridge") => {
                    let db = app.data_dir.join("sessions.json");
                    app.session_input = cmd.to_string();
                    app.view = View::Session;
                    if let Err(e) = crate::tools::session_create(&db, "unnamed") {
                        app.add_log(LogEntry::Error(format!(
                            "Failed to create bridge session: {e}"
                        )));
                    }
                    app.boulder_dirty = true;
                }
                cmd if cmd.starts_with(":refresh") => {
                    app.session_dirty = true;
                    app.todo_dirty = true;
                    app.add_log(LogEntry::Info("Refreshed".into()));
                }
                "model" | ":model" => {
                    let name = if app.model_name.is_empty() {
                        "default"
                    } else {
                        &app.model_name
                    };
                    app.add_log(LogEntry::Info(format!("Current model: {name}")));
                    app.add_log(LogEntry::Info(
                        "Use Config or bridge to switch models.".into(),
                    ));
                }
                "agent" | ":agent" => {
                    app.add_log(LogEntry::Info(
                        "Agent routing is managed by the bridge.".into(),
                    ));
                    app.add_log(LogEntry::Info(
                        "Use Config to configure agent profiles.".into(),
                    ));
                }
                "theme" | ":theme" => {
                    app.toggle_theme();
                    app.add_log(LogEntry::Success(format!(
                        "Theme switched to {}",
                        match app.theme_mode {
                            super::theme::ThemeMode::Dark => "Dark",
                            super::theme::ThemeMode::Light => "Light",
                        }
                    )));
                }
                "dev" | ":dev" => {
                    app.dev_goal.clear();
                    app.dev_context.clear();
                    app.dev_parallel = false;
                    app.dev_resume = false;
                    app.dev_report = None;
                    app.dev_status = "idle".into();
                    app.dev_focus = DevFocus::Goal;
                    app.dialog = Some(Dialog::Dev {
                        goal: String::new(),
                    });
                }
                _ => {}
            }
        }
        _ => {}
    }
}
