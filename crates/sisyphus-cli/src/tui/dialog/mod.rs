pub(crate) mod handlers;
pub(crate) mod renders;

use crossterm::event::{KeyCode, KeyEvent};

use super::{App, LogEntry};

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
        Dialog::SessionList => handlers::handle_session_list_key(code, app),
        Dialog::Todos => handlers::handle_todos_key(code, app),
        Dialog::Lsp => handlers::handle_lsp_key(code, app),
        Dialog::Sop => handlers::handle_sop_key(code, app),
        Dialog::Roadmap => handlers::handle_roadmap_key(code, app),
        Dialog::ConfirmDelete => {
            match code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if let Some(id) = app.confirm_delete.take() {
                        let db = app.data_dir.join("sessions.json");
                        if crate::tools::session_delete(&db, &id).is_ok() {
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
        Dialog::Dev { goal: _ } => handlers::handle_dev_key(key, app),
        Dialog::Rename { session_id: _, name: _ } => handlers::handle_rename_key(code, app),
    }
}

/// Render the dialog overlay.
pub fn render_dialog_overlay(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, app: &App) {

    let Some(ref dialog) = app.dialog else { return };

    match dialog {
        Dialog::Help => renders::render_help_dialog(frame, area, app),
        Dialog::SessionList => renders::render_session_list_dialog(frame, area, app),
        Dialog::Todos => renders::render_todos_dialog(frame, area, app),
        Dialog::Lsp => renders::render_lsp_dialog(frame, area, app),
        Dialog::Sop => renders::render_sop_dialog(frame, area, app),
        Dialog::Roadmap => renders::render_roadmap_dialog(frame, area, app),
        Dialog::ConfirmDelete => renders::render_confirm_dialog(frame, area, app),
        Dialog::Dev { goal: _goal } => renders::render_dev_dialog(frame, area, app),
        Dialog::Rename { session_id: _, name } => renders::render_rename_dialog(frame, area, app, name),
    }
}
