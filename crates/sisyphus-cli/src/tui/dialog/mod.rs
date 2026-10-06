pub(crate) mod handlers;
pub(crate) mod renders;

use crossterm::event::{KeyCode, KeyEvent};

use super::background::spawn_delete_workspace;
use super::{App, LogEntry, ToastKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Dialog {
    SessionList,
    Todos,
    Lsp,
    Sop,
    Roadmap,
    SkillMarket,
    WorkspaceList,
    Trace { sop_id: String },
    CognitiveTrace,
    CognitiveSummary,
    Health,
    Help,
    ConfirmDelete(ConfirmTarget),

    Dev { goal: String },
    Rename { session_id: String, name: String },
}

/// 待确认删除的对象类型
///
/// 删除会话与删除 todo 都需二次确认，但二者的持久化后端与状态字段不同，
/// 必须区分，否则确认框会对错误的对象执行删除。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfirmTarget {
    Session,
    Todo,
    Workspace,
}

/// 对话框打开时处理按键事件。
pub fn handle_dialog_key(key: KeyEvent, app: &mut App) {
    let code = key.code;
    let Some(ref dialog) = app.dialog else { return };

    match dialog {
        Dialog::Help => {
            match code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                    app.dialog = None;
                    app.help_scroll = 0;
                },
                KeyCode::Up => {
                    app.help_scroll = app.help_scroll.saturating_sub(1);
                },
                KeyCode::Down => {
                    app.help_scroll = app.help_scroll.saturating_add(1);
                },
                KeyCode::PageUp => {
                    app.help_scroll = app.help_scroll.saturating_sub(10);
                },
                KeyCode::PageDown => {
                    app.help_scroll = app.help_scroll.saturating_add(10);
                },
                KeyCode::Home => {
                    app.help_scroll = 0;
                },
                KeyCode::End => {
                    app.help_scroll = usize::MAX; // will clamp at render
                },
                _ => {},
            }
        },
        Dialog::WorkspaceList => handlers::handle_workspace_list_key(code, app),
        Dialog::SessionList => handlers::handle_session_list_key(code, app),
        Dialog::Todos => handlers::handle_todos_key(code, app),
        Dialog::Lsp => handlers::handle_lsp_key(code, app),
        Dialog::Sop => handlers::handle_sop_key(code, app),
        Dialog::Roadmap => handlers::handle_roadmap_key(code, app),
        Dialog::SkillMarket => handlers::handle_skill_market_key(code, app),
        Dialog::Trace { sop_id: _ } => handlers::handle_trace_key(code, app),
        Dialog::CognitiveTrace => handlers::handle_cognitive_trace_key(code, app),
        Dialog::CognitiveSummary => handlers::handle_cognitive_summary_key(code, app),
        Dialog::Health => handlers::handle_health_key(code, app),
        Dialog::ConfirmDelete(target) => match code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(id) = app.confirm_delete.take() {
                    match target {
                        ConfirmTarget::Session => {
                            let db = app.data_dir.join("sessions.json");
                            if crate::tools::session_delete(&db, &id).is_ok() {
                                app.add_log(LogEntry::Success(format!("Deleted session {id}")));
                                app.session_dirty = true;
                            }
                        },
                        ConfirmTarget::Todo => {
                            let db = app.data_dir.join("todos.json");
                            if crate::tools::todo_delete(&db, &id).is_ok() {
                                app.add_log(LogEntry::Success(format!("Deleted todo {id}")));
                                app.todo_dirty = true;
                            }
                        },
                        ConfirmTarget::Workspace => {
                            // 工作区删除走异步后台任务，`id` 即目标工作区 id
                            app.add_toast(ToastKind::Info, format!("Deleting workspace {id}..."));
                            if app.pending_op.is_none() {
                                app.pending_op =
                                    Some(spawn_delete_workspace(app.bridge_address.clone(), id));
                            }
                        },
                    }
                }
                app.dialog = None;
            },
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.confirm_delete = None;
                app.dialog = None;
            },
            _ => {},
        },
        Dialog::Dev { goal: _ } => handlers::handle_dev_key(key, app),
        Dialog::Rename {
            session_id: _,
            name: _,
        } => handlers::handle_rename_key(code, app),
    }
}

/// 渲染对话框覆盖层。
pub fn render_dialog_overlay(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, app: &App) {
    let Some(ref dialog) = app.dialog else { return };

    match dialog {
        Dialog::Help => renders::render_help_dialog(frame, area, app),
        Dialog::WorkspaceList => renders::render_workspace_list_dialog(frame, area, app),
        Dialog::SessionList => renders::render_session_list_dialog(frame, area, app),
        Dialog::Todos => renders::render_todos_dialog(frame, area, app),
        Dialog::Lsp => renders::render_lsp_dialog(frame, area, app),
        Dialog::Sop => renders::render_sop_dialog(frame, area, app),
        Dialog::Roadmap => renders::render_roadmap_dialog(frame, area, app),
        Dialog::SkillMarket => renders::render_skill_market_dialog(frame, area, app),
        Dialog::Trace { sop_id } => renders::render_trace_dialog(frame, area, app, sop_id),
        Dialog::CognitiveTrace => renders::render_cognitive_trace_dialog(frame, area, app),
        Dialog::CognitiveSummary => renders::render_cognitive_summary_dialog(frame, area, app),
        Dialog::Health => renders::render_health_dialog(frame, area, app),
        Dialog::ConfirmDelete(target) => renders::render_confirm_dialog(frame, area, app, *target),
        Dialog::Dev { goal: _goal } => renders::render_dev_dialog(frame, area, app),
        Dialog::Rename {
            session_id: _,
            name,
        } => renders::render_rename_dialog(frame, area, app, name),
    }
}
