use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, MouseButton, MouseEventKind};

use super::app::{matching_palette_entries, App, DevFocus, LogEntry, View};
use super::background::{spawn_fetch_available_skills, spawn_fetch_roadmap, spawn_list_sops};
use super::dialog::{handle_dialog_key, Dialog};
use super::helpers::paste_from_clipboard;
use super::home::handle_home_key;
use super::sessions::handle_sessions_key;
use crate::permission::CliPermission;

// ── 事件处理器 ───────────────────────────────────────

pub(crate) fn handle_event(event: Event, app: &mut App, _perms: &CliPermission) -> Result<bool> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            app.notification = None; // Clear notification on any key press

            // 全局：Ctrl+C / Ctrl+Q → 快速退出（双击确认）
            if (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('q'))
                && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
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

            // 全局：Ctrl+K → 切换命令面板（opencode 风格）
            if key.code == KeyCode::Char('k')
                && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
            {
                app.palette_open = !app.palette_open;
                if app.palette_open {
                    app.palette_query.clear();
                    app.palette_sel = 0;
                }
                return Ok(false);
            }

            // 全局：Ctrl+L → 会话列表对话框
            if key.code == KeyCode::Char('l')
                && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
            {
                app.dialog = Some(Dialog::SessionList);
                app.dialog_sel = 0;
                app.dialog_query.clear();
                app.session_dirty = true;
                return Ok(false);
            }

            // 全局：Ctrl+M → 模型信息（模型列表占位）
            if key.code == KeyCode::Char('m')
                && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
            {
                let name = if app.model_name.is_empty() {
                    "default"
                } else {
                    &app.model_name
                };
                app.add_log(LogEntry::Info(format!("Current model: {name}")));
                return Ok(false);
            }

            // 全局：Ctrl+A → 代理信息（代理列表占位）
            if key.code == KeyCode::Char('a')
                && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
            {
                app.add_log(LogEntry::Info(
                    "Agent routing managed by bridge. See Config for profiles.".into(),
                ));
                return Ok(false);
            }

            // 全局：F1 → 帮助对话框
            if key.code == KeyCode::F(1) {
                app.dialog = Some(Dialog::Help);
                return Ok(false);
            }

            // 全局：Ctrl+H → 返回主页
            if key.code == KeyCode::Char('h')
                && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
            {
                app.view = View::Home;
                app.dialog = None;
                app.boulder_dirty = true;
                return Ok(false);
            }

            // 全局：Ctrl+D → 切换仪表盘
            if key.code == KeyCode::Char('d')
                && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
            {
                app.view = View::Dashboard;
                app.dialog = None;
                app.boulder_dirty = true;
                // 进入仪表盘时拉取一次真实异常明细（REST /api/anomaly）。
                app.fetch_dashboard_anomalies();
                return Ok(false);
            }

            // 全局：Ctrl+N → 新建会话
            if key.code == KeyCode::Char('n')
                && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
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
                    },
                    Err(e) => app.add_log(LogEntry::Error(format!("Create failed: {e}"))),
                }
                return Ok(false);
            }

            // 全局：Ctrl+V → 粘贴
            if key.code == KeyCode::Char('v')
                && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
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
                        },
                        View::Dashboard => {},
                        View::Session if app.session_detail.is_some() => {
                            app.session_input.push_str(&pasted);
                        },
                        View::Session => {},
                    }
                }
                return Ok(false);
            }

            // 引导键：Ctrl+X → leader 模式
            if key.code == KeyCode::Char('x')
                && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
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
                    },
                    KeyCode::Char('p') => {
                        app.leader_mode = false;
                        app.palette_open = true;
                        app.palette_query.clear();
                        app.palette_sel = 0;
                        return Ok(false);
                    },
                    KeyCode::Esc => {
                        app.leader_mode = false;
                        return Ok(false);
                    },
                    _ => {
                        app.leader_mode = false;
                    },
                }
            }

            // 对话框优先于视图路由
            if app.dialog.is_some() {
                handle_dialog_key(key, app);
                return Ok(false);
            }

            // 面板打开时，所有按键路由到面板处理器
            if app.palette_open {
                handle_palette_key(key, app);
                return Ok(false);
            }

            // 按当前视图路由
            match app.view {
                View::Home => handle_home_key(key, app),
                View::Session => handle_sessions_key(key, app),
                View::Dashboard => crate::tui::dashboard::handle_dashboard_key(key, app),
            }

            if app.exit_immediately {
                return Ok(true);
            }
        },
        Event::Mouse(m) => match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if app.view == View::Session && app.session_detail.is_some() {
                    if let Some(msg_idx) = app.find_message_at_row(m.row) {
                        app.selected_msg = Some(msg_idx);
                        app.toggle_message_thoughts(msg_idx);
                    }
                }
            },
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
            },
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
            },
            _ => {},
        },
        Event::Resize(_, _) => {}, // ratatui handles redraw automatically
        _ => {},
    }
    Ok(false)
}

fn handle_palette_key(key: KeyEvent, app: &mut App) {
    let code = key.code;
    let matches = matching_palette_entries(&app.palette_query);
    match code {
        KeyCode::Esc => {
            app.palette_open = false;
        },
        KeyCode::Char(c) => {
            app.palette_query.push(c);
            app.palette_sel = 0;
        },
        KeyCode::Backspace => {
            app.palette_query.pop();
            app.palette_sel = 0;
        },
        KeyCode::Up => {
            if !matches.is_empty() {
                app.palette_sel = app.palette_sel.saturating_sub(1);
            }
        },
        KeyCode::Down => {
            if !matches.is_empty() {
                app.palette_sel = (app.palette_sel + 1).min(matches.len().saturating_sub(1));
            }
        },
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
                },
                "home" | ":home" => {
                    app.view = View::Home;
                    app.dialog = None;
                    app.boulder_dirty = true;
                },
                "sessions" | "session list" | ":sessions" => {
                    app.dialog = Some(Dialog::SessionList);
                    app.dialog_sel = 0;
                    app.dialog_query.clear();
                    app.session_dirty = true;
                },
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
                        },
                        Err(e) => app.add_log(LogEntry::Error(format!("Create failed: {e}"))),
                    }
                },
                "todos" | ":todos" => {
                    app.dialog = Some(Dialog::Todos);
                    app.refresh_todos();
                    app.todo_sel = 0;
                },
                "lsp" | ":lsp" => {
                    app.dialog = Some(Dialog::Lsp);
                    app.dialog_query.clear();
                    app.lsp_scroll = 0;
                },
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
                },
                "cogtrace" | ":cogtrace" | "ct" | ":ct" => {
                    app.dialog = Some(Dialog::CognitiveTrace);
                    app.fetch_cognitive_trace();
                },
                "cogsum" | ":cogsum" | "cs" | ":cs" => {
                    app.dialog = Some(Dialog::CognitiveSummary);
                    app.fetch_cognitive_summary();
                },
                "health" | ":health" | "h" | ":h" => {
                    app.dialog = Some(Dialog::Health);
                    app.fetch_health_deep();
                },
                "roadmap" | ":roadmap" => {
                    app.dialog = Some(Dialog::Roadmap);
                    app.roadmap_status = "connecting...".into();
                    if app.pending_op.is_none() {
                        app.pending_op = Some(spawn_fetch_roadmap(
                            app.bridge_client.clone(),
                            app.bridge_address.clone(),
                        ));
                    }
                },
                "skills" | ":skills" => {
                    app.dialog = Some(Dialog::SkillMarket);
                    app.skill_market_status = "connecting...".into();
                    if app.pending_op.is_none() {
                        app.pending_op =
                            Some(spawn_fetch_available_skills(app.bridge_address.clone()));
                    }
                },
                "workspaces" | ":workspaces" => {
                    app.dialog = Some(Dialog::WorkspaceList);
                    app.workspace_status = "connecting...".into();
                    if app.pending_op.is_none() {
                        app.pending_op = Some(super::background::spawn_list_workspaces(
                            app.bridge_address.clone(),
                        ));
                    }
                },
                "clear" | ":clear" => {
                    app.log.clear();
                    app.scroll = 0;
                },
                "exit" | ":exit" | "quit" | ":quit" => {
                    app.exit_requested = true;
                    app.add_log(LogEntry::Warn(
                        "Type `exit` again to confirm, or `clear` to dismiss.".into(),
                    ));
                },
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
                },
                cmd if cmd.starts_with(":refresh") => {
                    app.session_dirty = true;
                    app.todo_dirty = true;
                    app.add_log(LogEntry::Info("Refreshed".into()));
                },
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
                },
                "agent" | ":agent" => {
                    app.add_log(LogEntry::Info(
                        "Agent routing is managed by the bridge.".into(),
                    ));
                    app.add_log(LogEntry::Info(
                        "Use Config to configure agent profiles.".into(),
                    ));
                },
                "theme" | ":theme" => {
                    app.toggle_theme();
                    app.add_log(LogEntry::Success(format!(
                        "Theme switched to {}",
                        match app.theme_mode {
                            super::theme::ThemeMode::Dark => "Dark",
                            super::theme::ThemeMode::Light => "Light",
                        }
                    )));
                },
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
                },
                _ => {},
            }
        },
        _ => {},
    }
}
