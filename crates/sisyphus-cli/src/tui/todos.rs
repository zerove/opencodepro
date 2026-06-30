use crossterm::event::KeyCode;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use super::helpers::{copy_to_clipboard, paste_from_clipboard, scroll_pct};
use super::{App, LogEntry};

pub fn handle_todos_key(code: KeyCode, app: &mut App) {
    // Inline add mode
    if !app.todo_adding.is_empty() {
        match code {
            KeyCode::Enter => {
                let task = app.todo_adding.trim().to_string();
                if !task.is_empty() {
                    let db = app.data_dir.join("todos.json");
                    match sisyphus_tools::todo_add(&db, &task, sisyphus_tools::TodoPriority::Medium) {
                        Ok(item) => {
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
            KeyCode::Char('\x16') => {
                let pasted = paste_from_clipboard();
                if !pasted.is_empty() {
                    app.todo_adding.push_str(&pasted);
                }
            }
            KeyCode::Char(c) => { app.todo_adding.push(c); }
            _ => {}
        }
        return;
    }

    app.refresh_todos();
    match code {
        KeyCode::Up => app.todo_sel = app.todo_sel.saturating_sub(1),
        KeyCode::Down => {
            app.todo_sel = (app.todo_sel + 1).min(app.todo_items.len().saturating_sub(1))
        }
        KeyCode::PageUp => app.todo_sel = app.todo_sel.saturating_sub(10),
        KeyCode::PageDown => {
            app.todo_sel = (app.todo_sel + 10).min(app.todo_items.len().saturating_sub(1));
        }
        KeyCode::Home => app.todo_sel = 0,
        KeyCode::End => app.todo_sel = app.todo_items.len().saturating_sub(1),
        KeyCode::Char(' ') | KeyCode::Enter => {
            app.confirm_delete = None;
            app.toggle_todo_status();
        }
        KeyCode::Char('d') | KeyCode::Delete => app.delete_todo(),
        KeyCode::Char('r') => app.todo_dirty = true,
        KeyCode::Char('a') => {
            app.todo_adding = String::new();
        }
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            if let Some(todo) = app.todo_items.get(app.todo_sel) {
                let checked = matches!(todo.status, sisyphus_tools::TodoStatus::Completed);
                let text = format!("[{}] {} (prio: {:?})",
                    if checked { "x" } else { " " },
                    todo.content,
                    todo.priority,
                );
                copy_to_clipboard(&text);
                app.copy_notification = Some(std::time::Instant::now());
                app.add_log(LogEntry::Success("📋 Copied todo to clipboard".into()));
            }
        }
        _ => {}
    }
}

pub fn render_todos(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    // Inline add bar
    if !app.todo_adding.is_empty() {
        let [input_area, list_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(0),
        ]).areas(area);

        let input_block = Block::default()
            .borders(Borders::ALL)
            .title(" New Todo ")
            .border_style(Style::default().fg(theme.success));
        frame.render_widget(
            Paragraph::new(app.todo_adding.as_str()).block(input_block),
            input_area,
        );

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Todos ")
            .title_bottom(scroll_pct(app.todo_sel, app.todo_items.len()))
            .border_style(Style::default().fg(theme.border));

        if app.todo_items.is_empty() {
            let text = Text::from(Line::from(Span::styled(
                "No todos yet. Type a description and press Enter.",
                Style::default().fg(Color::DarkGray),
            )));
            frame.render_widget(Paragraph::new(text).block(block), list_area);
        } else {
            render_todo_list(frame, list_area, app, block);
        }
        return;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Todos ")
        .title_bottom(scroll_pct(app.todo_sel, app.todo_items.len()))
        .border_style(Style::default().fg(theme.border));

    if app.todo_items.is_empty() {
        let text = Text::from(Line::from(Span::styled(
            "No todos. Press a to add one.",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(Paragraph::new(text).block(block), area);
        return;
    }

    render_todo_list(frame, area, app, block);
}

fn render_todo_list(frame: &mut ratatui::Frame, area: Rect, app: &App, block: Block) {
    let theme = app.theme();
    let items: Vec<ListItem> = app
        .todo_items
        .iter()
        .map(|item| {
            let checkbox = match item.status {
                sisyphus_tools::TodoStatus::Pending => "[ ]",
                sisyphus_tools::TodoStatus::InProgress => "[~]",
                sisyphus_tools::TodoStatus::Completed => "[✓]",
                sisyphus_tools::TodoStatus::Cancelled => "[✗]",
            };
            let style = match item.status {
                sisyphus_tools::TodoStatus::Completed => Style::default()
                     .fg(theme.success)
                    .add_modifier(Modifier::DIM),
                sisyphus_tools::TodoStatus::Cancelled => {
                     Style::default().fg(theme.error).add_modifier(Modifier::DIM)
                }
                 sisyphus_tools::TodoStatus::InProgress => Style::default().fg(theme.primary),
                 sisyphus_tools::TodoStatus::Pending => Style::default().fg(theme.fg),
            };
            let line = Line::from(vec![
                Span::styled(format!("{} ", checkbox), style),
                Span::styled(&item.content, style),
                Span::styled(
                    format!(" ({})", item.priority),
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
        &mut ratatui::widgets::ListState::default().with_selected(Some(app.todo_sel)),
    );
}
