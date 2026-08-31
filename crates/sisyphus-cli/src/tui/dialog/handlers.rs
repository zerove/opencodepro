use crossterm::event::{KeyCode, KeyEvent};

use super::super::background::{
    spawn_delete_workspace, spawn_dev_agent, spawn_execute_sop, spawn_fetch_available_skills,
    spawn_fetch_installed_skills, spawn_install_skill, spawn_list_sops, spawn_list_workspaces,
    spawn_switch_workspace, spawn_uninstall_skill,
};
use super::super::{App, DevFocus, LogEntry, ToastKind, View};
use super::Dialog;

// ── 工作区列表对话框 ─────────────────────────────────

pub(super) fn handle_workspace_list_key(code: KeyCode, app: &mut App) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.dialog = None;
        },
        KeyCode::Up => app.workspace_sel = app.workspace_sel.saturating_sub(1),
        KeyCode::Down => {
            app.workspace_sel = (app.workspace_sel + 1).min(app.workspaces.len().saturating_sub(1));
        },
        KeyCode::Enter => {
            if let Some(ws) = app.workspaces.get(app.workspace_sel) {
                let name = ws.name.clone();
                let id = ws.id.clone();
                app.add_toast(
                    ToastKind::Info,
                    format!("Switching to workspace: {}...", name),
                );
                if app.pending_op.is_none() {
                    app.pending_op = Some(spawn_switch_workspace(app.bridge_address.clone(), id));
                }
                app.dialog = None;
            }
        },
        KeyCode::Char('r') | KeyCode::Char('R') => {
            app.workspace_status = "refreshing...".into();
            if app.pending_op.is_none() {
                app.pending_op = Some(spawn_list_workspaces(app.bridge_address.clone()));
            }
        },
        KeyCode::Char('d') | KeyCode::Delete => {
            if let Some(ws) = app.workspaces.get(app.workspace_sel) {
                let name = ws.name.clone();
                let id = ws.id.clone();
                app.add_toast(ToastKind::Info, format!("Deleting workspace: {}...", name));
                if app.pending_op.is_none() {
                    app.pending_op = Some(spawn_delete_workspace(app.bridge_address.clone(), id));
                }
            }
        },
        _ => {},
    }
}

// ── 会话列表对话框 ───────────────────────────────

pub(super) fn handle_session_list_key(code: KeyCode, app: &mut App) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            if !app.session_filter.is_empty() {
                app.session_filter.clear();
            }
            app.dialog = None;
        },
        KeyCode::Up => app.session_sel = app.session_sel.saturating_sub(1),
        KeyCode::Down => {
            let visible = filtered_sessions(app).len();
            app.session_sel = (app.session_sel + 1).min(visible.saturating_sub(1));
        },
        KeyCode::PageUp => app.session_sel = app.session_sel.saturating_sub(10),
        KeyCode::PageDown => {
            let visible = filtered_sessions(app).len();
            app.session_sel = (app.session_sel + 10).min(visible.saturating_sub(1));
        },
        KeyCode::Home => app.session_sel = 0,
        KeyCode::End => {
            let visible = filtered_sessions(app).len();
            app.session_sel = visible.saturating_sub(1);
        },
        KeyCode::Enter => {
            select_session(app);
            app.dialog = None;
        },
        KeyCode::Backspace => {
            app.session_filter.pop();
            app.session_sel = 0;
        },
        KeyCode::Char('c') | KeyCode::Char('C') => {
            let db = app.data_dir.join("sessions.json");
            match crate::tools::session_create(&db, "unnamed") {
                Ok(s) => {
                    app.add_log(LogEntry::Success(format!(
                        "Created session {} ({})",
                        s.id, s.name
                    )));
                    app.session_dirty = true;
                    // 导航到新会话
                    app.session_detail = crate::tools::session_get(&db, &s.id).ok();
                    app.session_detail_scroll = usize::MAX;
                    app.view = View::Session;
                    app.dialog = None;
                },
                Err(e) => app.add_log(LogEntry::Error(format!("Create failed: {e}"))),
            }
        },
        KeyCode::Char('d') | KeyCode::Delete => {
            let filtered = filtered_sessions(app);
            if let Some(&idx) = filtered.get(app.session_sel.min(filtered.len().saturating_sub(1)))
            {
                if let Some(s) = app.sessions.get(idx) {
                    app.confirm_delete = Some(s.id.clone());
                    app.dialog = Some(Dialog::ConfirmDelete);
                }
            }
        },
        KeyCode::Char('r') | KeyCode::Char('R') => app.session_dirty = true,
        KeyCode::Char(c) if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' => {
            app.session_filter.push(c);
            app.session_sel = 0;
        },
        _ => {},
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
    app.session_detail = crate::tools::session_get(&db, id).ok();
    app.session_detail_scroll = usize::MAX;
    app.view = View::Session;
}

// ── 待办对话框 ─────────────────────────────────────

pub(super) fn handle_todos_key(code: KeyCode, app: &mut App) {
    if !app.todo_adding.is_empty() {
        match code {
            KeyCode::Enter => {
                let task = app.todo_adding.trim().to_string();
                if !task.is_empty() {
                    let db = app.data_dir.join("todos.json");
                    match crate::tools::todo_add(&db, &task, crate::tools::TodoPriority::Medium) {
                        Ok(item) => {
                            app.add_toast(
                                ToastKind::Success,
                                format!("Added todo: {}", item.content),
                            );
                            app.add_log(LogEntry::Success(format!("Added todo: {}", item.content)));
                            app.todo_dirty = true;
                        },
                        Err(e) => app.add_log(LogEntry::Error(format!("Add todo failed: {e}"))),
                    }
                }
                app.todo_adding.clear();
            },
            KeyCode::Esc => app.todo_adding.clear(),
            KeyCode::Backspace => {
                app.todo_adding.pop();
            },
            KeyCode::Char(c) => {
                app.todo_adding.push(c);
            },
            _ => {},
        }
        return;
    }

    match code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.dialog = None;
        },
        KeyCode::Up => app.todo_sel = app.todo_sel.saturating_sub(1),
        KeyCode::Down => {
            app.todo_sel = (app.todo_sel + 1).min(app.todo_items.len().saturating_sub(1));
        },
        KeyCode::Char(' ') | KeyCode::Enter => toggle_todo(app),
        KeyCode::Char('a') => app.todo_adding = String::new(),
        KeyCode::Char('d') | KeyCode::Delete => delete_todo(app),
        KeyCode::Char('r') | KeyCode::Char('R') => {
            app.todo_dirty = true;
            app.add_toast(ToastKind::Info, "Refreshed todos");
        },
        _ => {},
    }
}

fn toggle_todo(app: &mut App) {
    if app.todo_items.is_empty() {
        return;
    }
    let db = app.data_dir.join("todos.json");
    let item = &app.todo_items[app.todo_sel];
    let new_status = match item.status {
        crate::tools::TodoStatus::Pending => crate::tools::TodoStatus::InProgress,
        crate::tools::TodoStatus::InProgress => crate::tools::TodoStatus::Completed,
        crate::tools::TodoStatus::Completed => crate::tools::TodoStatus::Pending,
        crate::tools::TodoStatus::Cancelled => crate::tools::TodoStatus::Pending,
    };
    if crate::tools::todo_update(&db, &item.id, None, Some(new_status.clone()), None).is_ok() {
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
    if crate::tools::todo_delete(&db, &id).is_ok() {
        app.todo_dirty = true;
        app.add_toast(ToastKind::Info, "Deleted todo");
        app.add_log(LogEntry::Info(format!("Deleted todo {id}")));
    }
}

// ── LSP 对话框 ───────────────────────────────────────

pub(super) fn handle_lsp_key(code: KeyCode, app: &mut App) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.dialog = None;
        },
        KeyCode::Up => app.lsp_scroll = app.lsp_scroll.saturating_sub(1),
        KeyCode::Down => app.lsp_scroll = app.lsp_scroll.saturating_add(1),
        KeyCode::PageUp => app.lsp_scroll = app.lsp_scroll.saturating_sub(10),
        KeyCode::PageDown => app.lsp_scroll = app.lsp_scroll.saturating_add(10),
        KeyCode::Char('1') => {
            app.lsp_language = "rust".into();
            app.lsp_server = "rust-analyzer".into();
            app.add_toast(ToastKind::Info, "LSP: Rust (rust-analyzer)");
        },
        KeyCode::Char('2') => {
            app.lsp_language = "typescript".into();
            app.lsp_server = "typescript-language-server".into();
            app.add_toast(ToastKind::Info, "LSP: TypeScript");
        },
        KeyCode::Char('3') => {
            app.lsp_language = "python".into();
            app.lsp_server = "pylsp".into();
            app.add_toast(ToastKind::Info, "LSP: Python (pylsp)");
        },
        KeyCode::Enter => {
            app.lsp_status = "running".into();
            app.add_toast(ToastKind::Info, "LSP analysis started...");
        },
        KeyCode::Backspace => {
            app.dialog_query.pop();
        },
        KeyCode::Char(c) if !c.is_control() => {
            app.dialog_query.push(c);
        },
        _ => {},
    }
}

// ── SOP 对话框 ────────────────────────────────────────

pub(super) fn handle_sop_key(code: KeyCode, app: &mut App) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.dialog = None;
        },
        KeyCode::Up => app.sop_sel = app.sop_sel.saturating_sub(1),
        KeyCode::Down => {
            let filtered = filtered_sops(app);
            if !filtered.is_empty() {
                app.sop_sel = (app.sop_sel + 1).min(filtered.len().saturating_sub(1));
            }
        },
        KeyCode::Enter => {
            let sop_id = filtered_sops(app).get(app.sop_sel).map(|s| s.sop_id.clone());
            let Some(sop_id) = sop_id else { return };
            app.sop_status = format!("executing {sop_id}...");
            app.sop_result = None;
            app.add_toast(ToastKind::Info, format!("Executing SOP: {sop_id}"));
            if app.pending_op.is_none() {
                app.pending_op = Some(spawn_execute_sop(
                    app.bridge_client.clone(),
                    app.bridge_address.clone(),
                    sop_id,
                ));
            }
        },
        KeyCode::Char('r') | KeyCode::Char('R') => {
            app.sop_status = "connecting...".into();
            app.sop_result = None;
            app.add_toast(ToastKind::Info, "Refreshing SOP list...");
            if app.pending_op.is_none() {
                app.pending_op = Some(spawn_list_sops(
                    app.bridge_client.clone(),
                    app.bridge_address.clone(),
                ));
            }
        },
        KeyCode::Char('t') | KeyCode::Char('T') => {
            let sop_id = filtered_sops(app).get(app.sop_sel).map(|s| s.sop_id.clone());
            let Some(id) = sop_id else { return };
            app.add_toast(ToastKind::Info, format!("Fetching trace for {id}..."));
            app.fetch_sop_trace(id.clone());
            app.dialog = Some(crate::tui::dialog::Dialog::Trace { sop_id: id });
        },
        KeyCode::Backspace => {
            app.sop_query.pop();
        },
        KeyCode::Char(c) if !c.is_control() => {
            app.sop_query.push(c);
        },
        _ => {},
    }
}

fn filtered_sops(app: &App) -> Vec<&crate::bridge::SopInfo> {
    if app.sop_query.is_empty() {
        return app.sop_sops.iter().collect();
    }
    let q = app.sop_query.to_lowercase();
    app.sop_sops.iter().filter(|s| s.sop_id.to_lowercase().contains(&q)).collect()
}

// ── 路线图对话框 ────────────────────────────────────

pub(super) fn handle_roadmap_key(code: KeyCode, app: &mut App) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.dialog = None;
        },
        KeyCode::Up => app.roadmap_scroll = app.roadmap_scroll.saturating_sub(1),
        KeyCode::Down => app.roadmap_scroll = app.roadmap_scroll.saturating_add(1),
        KeyCode::PageUp => app.roadmap_scroll = app.roadmap_scroll.saturating_sub(10),
        KeyCode::PageDown => app.roadmap_scroll = app.roadmap_scroll.saturating_add(10),
        KeyCode::Home | KeyCode::Char('g') => app.roadmap_scroll = 0,
        KeyCode::End | KeyCode::Char('G') => app.roadmap_scroll = usize::MAX,
        KeyCode::Enter => {
            // 批准选中的提案
            let proposal_id = app.roadmap_status.clone(); // This is a placeholder; in a real system, we'd use roadmap_sel
            if !proposal_id.is_empty() {
                app.add_toast(
                    ToastKind::Info,
                    format!("Approving proposal {proposal_id}..."),
                );
                if app.pending_op.is_none() {
                    app.pending_op = Some(super::super::background::spawn_approve_proposal(
                        app.bridge_client.clone(),
                        app.bridge_address.clone(),
                        proposal_id,
                        "TUI_User".into(),
                    ));
                }
            }
        },
        KeyCode::Char('r') | KeyCode::Char('R') => {
            app.roadmap_status = "connecting...".into();
            if app.pending_op.is_none() {
                app.pending_op = Some(super::super::background::spawn_fetch_roadmap(
                    app.bridge_client.clone(),
                    app.bridge_address.clone(),
                ));
            }
        },
        _ => {},
    }
}

// ── 重命名会话对话框 ────────────────────────────

pub(super) fn handle_rename_key(code: KeyCode, app: &mut App) {
    let Some(Dialog::Rename { session_id, name }) = &app.dialog else {
        return;
    };
    let session_id = session_id.clone();
    match code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.dialog = None;
        },
        KeyCode::Enter => {
            let new_name = name.trim().to_string();
            if !new_name.is_empty() {
                let db = app.data_dir.join("sessions.json");
                match crate::tools::session_rename(&db, &session_id, &new_name) {
                    Ok(_) => {
                        app.add_toast(ToastKind::Success, format!("Renamed → {new_name}"));
                        app.add_log(LogEntry::Success(format!(
                            "Renamed session {session_id} → {new_name}"
                        )));
                        app.session_dirty = true;
                    },
                    Err(e) => app.add_log(LogEntry::Error(format!("Rename failed: {e}"))),
                }
            }
            app.dialog = None;
        },
        KeyCode::Backspace => {
            if let Some(Dialog::Rename { name, .. }) = &mut app.dialog {
                name.pop();
            }
        },
        KeyCode::Char(c) => {
            if let Some(Dialog::Rename { name, .. }) = &mut app.dialog {
                name.push(c);
            }
        },
        _ => {},
    }
}

// ── 技能市场对话框 ──────────────────────────────────
pub(super) fn handle_skill_market_key(code: KeyCode, app: &mut App) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.dialog = None;
        },
        KeyCode::Up => {
            app.skill_market_sel = app.skill_market_sel.saturating_sub(1);
        },
        KeyCode::Down => {
            let max = app.skill_market_available.len().max(app.skill_market_installed.len());
            app.skill_market_sel = (app.skill_market_sel + 1).min(max.saturating_sub(1));
        },
        KeyCode::Enter => {
            // 若光标在可用技能上，安装它。
            if app.skill_market_sel < app.skill_market_available.len() {
                let skill = app.skill_market_available[app.skill_market_sel].clone();
                app.add_toast(
                    ToastKind::Info,
                    format!("Installing skill: {}...", skill.name),
                );
                if app.pending_op.is_none() {
                    app.pending_op = Some(spawn_install_skill(
                        app.bridge_address.clone(),
                        skill.id.clone(),
                    ));
                }
            } else {
                // 若光标在已安装技能上，卸载它。
                let installed_idx =
                    app.skill_market_sel.saturating_sub(app.skill_market_available.len());
                if installed_idx < app.skill_market_installed.len() {
                    let skill = app.skill_market_installed[installed_idx].clone();
                    app.add_toast(
                        ToastKind::Info,
                        format!("Uninstalling skill: {}...", skill.name),
                    );
                    if app.pending_op.is_none() {
                        app.pending_op = Some(spawn_uninstall_skill(
                            app.bridge_address.clone(),
                            skill.id.clone(),
                        ));
                    }
                }
            }
        },
        KeyCode::Char('r') | KeyCode::Char('R') => {
            app.skill_market_status = "fetching available...".into();
            if app.pending_op.is_none() {
                app.pending_op = Some(spawn_fetch_available_skills(app.bridge_address.clone()));
            }
        },
        KeyCode::Char('i') | KeyCode::Char('I') => {
            app.skill_market_status = "fetching installed...".into();
            if app.pending_op.is_none() {
                app.pending_op = Some(spawn_fetch_installed_skills(app.bridge_address.clone()));
            }
        },
        _ => {},
    }
}

pub(super) fn handle_dev_key(key: KeyEvent, app: &mut App) {
    let code = key.code;
    // 若报告可用，任意键关闭它（用户可按 Esc 关闭）
    if app.dev_report.is_some() {
        match code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                app.dialog = None;
                app.dev_report = None;
                app.dev_status = "idle".into();
            },
            _ => {},
        }
        return;
    }

    // 运行中时，仅 Esc 取消（将状态设为空闲）
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
        },
        KeyCode::Tab => {
            app.dev_focus = match app.dev_focus {
                DevFocus::Goal => DevFocus::Context,
                DevFocus::Context => DevFocus::Parallel,
                DevFocus::Parallel => DevFocus::Resume,
                DevFocus::Resume => DevFocus::Run,
                DevFocus::Run => DevFocus::Goal,
            };
        },
        KeyCode::BackTab => {
            app.dev_focus = match app.dev_focus {
                DevFocus::Goal => DevFocus::Run,
                DevFocus::Context => DevFocus::Goal,
                DevFocus::Parallel => DevFocus::Context,
                DevFocus::Resume => DevFocus::Parallel,
                DevFocus::Run => DevFocus::Resume,
            };
        },
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
                    app.pending_op = Some(spawn_dev_agent(config, goal, context, parallel, resume));
                }
            }
        },
        KeyCode::Backspace => match app.dev_focus {
            DevFocus::Goal => {
                app.dev_goal.pop();
            },
            DevFocus::Context => {
                app.dev_context.pop();
            },
            _ => {},
        },
        KeyCode::Char(' ') => {
            match app.dev_focus {
                DevFocus::Parallel => app.dev_parallel = !app.dev_parallel,
                DevFocus::Resume => app.dev_resume = !app.dev_resume,
                DevFocus::Goal => app.dev_goal.push(' '),
                DevFocus::Context => app.dev_context.push(' '),
                DevFocus::Run => {}, // space on Run is no-op
            }
        },
        KeyCode::Char(c) => match app.dev_focus {
            DevFocus::Goal => app.dev_goal.push(c),
            DevFocus::Context => app.dev_context.push(c),
            _ => {},
        },
        _ => {},
    }
}

pub(crate) fn handle_cognitive_trace_key(code: KeyCode, app: &mut App) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.dialog = None;
            app.cog_trace_detail = None;
        },
        KeyCode::Up => {
            app.cog_trace_scroll = app.cog_trace_scroll.saturating_sub(1);
            app.cog_trace_detail = None;
        },
        KeyCode::Down => {
            app.cog_trace_scroll = app.cog_trace_scroll.saturating_add(1);
            app.cog_trace_detail = None;
        },
        KeyCode::PageUp => {
            app.cog_trace_scroll = app.cog_trace_scroll.saturating_sub(10);
            app.cog_trace_detail = None;
        },
        KeyCode::PageDown => {
            app.cog_trace_scroll = app.cog_trace_scroll.saturating_add(10);
            app.cog_trace_detail = None;
        },
        KeyCode::Home | KeyCode::Char('g') => {
            app.cog_trace_scroll = 0;
            app.cog_trace_detail = None;
        },
        KeyCode::End | KeyCode::Char('G') => {
            app.cog_trace_scroll = usize::MAX;
            app.cog_trace_detail = None;
        },
        KeyCode::Enter => {
            // 显示选中决策的完整详情（预测/实际/理由/指标）
            if app.cog_trace_detail.is_some() {
                app.cog_trace_detail = None;
            } else if let Some(row) = app.cog_trace.get(app.cog_trace_scroll) {
                app.cog_trace_detail = Some(format_cognitive_event(&row.event));
            }
        },
        KeyCode::Char('r') | KeyCode::Char('R') => {
            // 重新拉取决策链
            app.fetch_cognitive_trace();
        },
        KeyCode::Char('h') | KeyCode::Char('H') => {
            // 切换热缓冲 ⇄ SQL 历史
            app.toggle_cog_trace_history();
        },
        _ => {},
    }
}

/// 将认知事件格式化为可读的详情文本（Enter 查看）。
pub(crate) fn format_cognitive_event(event: &sisyphus_types::ExecutionEvent) -> String {
    match event {
        sisyphus_types::ExecutionEvent::NodeStarted { node_name } => {
            format!("▶ Node started: {node_name}")
        }
        sisyphus_types::ExecutionEvent::NodeCompleted {
            node_name,
            output,
            fuel_consumed,
            duration_ms,
        } => {
            let out = serde_json::to_string_pretty(&output.0)
                .unwrap_or_else(|_| output.0.to_string());
            format!(
                "✔ Node completed: {node_name}\nfuel: {fuel_consumed} | duration: {duration_ms}ms\n\noutput:\n{out}"
            )
        }
        sisyphus_types::ExecutionEvent::NodeFailed {
            node_name,
            error,
            fuel_consumed,
            duration_ms,
        } => format!(
            "✖ Node failed: {node_name}\nerror: {error}\nfuel: {fuel_consumed} | duration: {duration_ms}ms"
        ),
        sisyphus_types::ExecutionEvent::ExecutionComplete { result } => {
            format!("✓ Execution complete\nresult: {}", result.0)
        }
        sisyphus_types::ExecutionEvent::CognitivePlan { plan } => {
            format!(
                "🧠 Cognitive plan: {}\nsteps: {}",
                plan.goal,
                plan.steps.len()
            )
        }
        sisyphus_types::ExecutionEvent::CognitiveStep {
            step,
            message,
            metrics,
        } => {
            let mut out = format!("🧠 Step[{step}]: {message}");
            if let Some(m) = metrics {
                out.push_str(&format!(
                    "\niteration: {} | deviation: {:.2} | confidence: {:.2} | progress: {:.1}%",
                    m.iteration, m.deviation, m.confidence, m.progress * 100.0
                ));
                out.push_str(&format!(
                    "\nstability: {:.2} | info_gain: {:.2} | token_cost: {}",
                    m.stability, m.info_gain, m.token_cost
                ));
            }
            out
        }
        sisyphus_types::ExecutionEvent::MicroReflection {
            node_name,
            prediction,
            actual,
            decision,
            updates,
            reason,
        } => {
            let mut out = format!(
                "🔍 Micro-reflection @ {node_name}\nprediction: {prediction}\nactual: {actual}\ndecision: {decision}"
            );
            if let Some(ref r) = reason {
                out.push_str(&format!("\nreason: {r}"));
            }
            if let Some(ref ups) = updates {
                for (k, v) in ups {
                    out.push_str(&format!("\nupdate {k}: {v}"));
                }
            }
            out
        }
        sisyphus_types::ExecutionEvent::CognitiveComplete { report } => {
            format!("🎯 Cognitive loop complete\nreport:\n{report}")
        }
    }
}

pub(crate) fn handle_trace_key(code: KeyCode, app: &mut App) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.dialog = None;
            app.sop_trace_detail = None;
        },
        KeyCode::Up => {
            app.sop_trace_scroll = app.sop_trace_scroll.saturating_sub(1);
            app.sop_trace_detail = None;
        },
        KeyCode::Down => {
            app.sop_trace_scroll = app.sop_trace_scroll.saturating_add(1);
            app.sop_trace_detail = None;
        },
        KeyCode::PageUp => {
            app.sop_trace_scroll = app.sop_trace_scroll.saturating_sub(10);
            app.sop_trace_detail = None;
        },
        KeyCode::PageDown => {
            app.sop_trace_scroll = app.sop_trace_scroll.saturating_add(10);
            app.sop_trace_detail = None;
        },
        KeyCode::Home | KeyCode::Char('g') => {
            app.sop_trace_scroll = 0;
            app.sop_trace_detail = None;
        },
        KeyCode::End | KeyCode::Char('G') => {
            app.sop_trace_scroll = usize::MAX;
            app.sop_trace_detail = None;
        },
        KeyCode::Enter => {
            // 显示选中轨迹行的输出
            if app.sop_trace_detail.is_some() {
                app.sop_trace_detail = None;
            } else if let Some(ev) = app.sop_trace.get(app.sop_trace_scroll) {
                app.sop_trace_detail = ev.output.clone();
            }
        },
        _ => {},
    }
}

pub(crate) fn handle_cognitive_summary_key(code: KeyCode, app: &mut App) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.dialog = None;
        },
        KeyCode::Char('r') | KeyCode::Char('R') => {
            // 重新拉取健康摘要
            app.fetch_cognitive_summary();
        },
        _ => {},
    }
}
