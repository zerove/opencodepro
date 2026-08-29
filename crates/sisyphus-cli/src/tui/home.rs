use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
};

use super::background::spawn_session_add_message;
use super::dialog::Dialog;
use super::enhancements::{expand_file_references, filter_slash_commands};
use super::helpers::paste_from_clipboard;
use super::{App, LogEntry, View};
use unicode_width::UnicodeWidthStr;

const LOGO_LINES: &[&str] = &[
    r"   _____ _           _                       ",
    r"  / ____(_)         | |                      ",
    r" | (___  _ _ __  ___| |__   ___  _   _ _ __  ",
    r"  \___ \| | '_ \/ __| '_ \ / _ \| | | | '_ \ ",
    r"  ____) | | | | \__ \ | | | (_) | |_| | |_) |",
    r" |_____/|_|_| |_|___/_| |_|\___/ \__,_| .__/ ",
    r"                                       | |    ",
    r"                                       |_|    ",
];

const PLACEHOLDERS: &[&str] = &[
    "Ask anything, type /command, or Ctrl+K for palette...",
    "Start a new session...",
    "New: Home/End/Ctrl+U/W in input, type /help for all shortcuts",
];

/// Handle home key.
pub fn handle_home_key(key: KeyEvent, app: &mut App) {
    let code = key.code;

    // 若斜杠补全打开，处理补全导航
    if app.slash_completion_open {
        match code {
            KeyCode::Esc => {
                app.slash_completion_open = false;
                app.slash_completion_sel = 0;
                return;
            }
            KeyCode::Up => {
                app.slash_completion_sel = app.slash_completion_sel.saturating_sub(1);
                return;
            }
            KeyCode::Down => {
                let completions = filter_slash_commands(&app.home_input);
                if !completions.is_empty() {
                    app.slash_completion_sel =
                        (app.slash_completion_sel + 1).min(completions.len() - 1);
                }
                return;
            }
            KeyCode::Tab | KeyCode::Enter => {
                let completions = filter_slash_commands(&app.home_input);
                if let Some((cmd, _, _)) = completions.get(app.slash_completion_sel) {
                    app.home_input = cmd.name.to_string();
                    app.home_cursor = app.home_input.chars().count();
                }
                app.slash_completion_open = false;
                app.slash_completion_sel = 0;
                if code == KeyCode::Tab {
                    return;
                }
                // 继续执行到 Enter 处理
            }
            _ => {
                // 其他按键关闭补全，然后正常处理
                app.slash_completion_open = false;
                app.slash_completion_sel = 0;
            }
        }
    }

    match code {
        KeyCode::Enter => {
            let cmd = app.home_input.trim().to_string();
            if cmd.is_empty() {
                return;
            }
            // 以 / 开头则作为命令路由
            if cmd.starts_with('/') || cmd.starts_with(':') {
                app.add_log(LogEntry::Info(format!("> {cmd}")));
                execute_home_command(&cmd, app);
                app.home_input.clear();
                app.home_cursor = 0;
                return;
            }
            // 否则：创建新会话，将其作为首条消息
            // 发送前展开 @file 引用
            let expanded = expand_file_references(&cmd, &app.data_dir);
            let db = app.data_dir.join("sessions.json");
            let name = if cmd.chars().count() > 40 {
                format!("{}...", cmd.chars().take(40).collect::<String>())
            } else {
                cmd.clone()
            };
            match crate::tools::session_create(&db, &name) {
                Ok(s) => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs().to_string())
                        .unwrap_or_default();
                    let mut session = s.clone();
                    session.messages.push(crate::tools::Message {
                        role: "user".into(),
                        content: expanded.clone(),
                        created_at: now.clone(),
                        estimated_tokens: (expanded.len() as u64).div_ceil(4),
                        is_compressed: false,
                    });
                    session.message_count = 1;
                    app.session_detail = Some(session);
                    app.pending_op = Some(spawn_session_add_message(
                        app.data_dir.clone(),
                        s.id.clone(),
                        "user".to_string(),
                        expanded,
                        Some(app.bridge_address.clone()),
                    ));
                    app.session_detail_scroll = usize::MAX;
                    app.session_dirty = true;
                    app.view = View::Session;
                    app.home_input.clear();
                    app.home_cursor = 0;
                }
                Err(e) => {
                    app.add_log(LogEntry::Error(format!("Failed to create session: {e}")));
                }
            }
        }
        KeyCode::Esc => {
            app.home_input.clear();
            app.home_cursor = 0;
        }
        KeyCode::Backspace => {
            if app.home_cursor > 0 {
                let mut chars: Vec<char> = app.home_input.chars().collect();
                let idx = app.home_cursor.saturating_sub(1);
                if idx < chars.len() {
                    chars.remove(idx);
                    app.home_input = chars.into_iter().collect();
                    app.home_cursor = app.home_cursor.saturating_sub(1);
                }
            }
        }
        KeyCode::Delete => {
            let mut chars: Vec<char> = app.home_input.chars().collect();
            if app.home_cursor < chars.len() {
                chars.remove(app.home_cursor);
                app.home_input = chars.into_iter().collect();
            }
        }
        KeyCode::Left => {
            app.home_cursor = app.home_cursor.saturating_sub(1);
        }
        KeyCode::Right => {
            let char_count = app.home_input.chars().count();
            app.home_cursor = (app.home_cursor + 1).min(char_count);
        }
        KeyCode::Home => app.home_cursor = 0,
        KeyCode::End => app.home_cursor = app.home_input.chars().count(),
        KeyCode::Char('\x16') => {
            // Ctrl+V 粘贴
            let pasted = paste_from_clipboard();
            if !pasted.is_empty() {
                let byte_pos = app
                    .home_input
                    .char_indices()
                    .nth(app.home_cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(app.home_input.len());
                app.home_input.insert_str(byte_pos, &pasted);
                app.home_cursor += pasted.chars().count();
            }
        }
        KeyCode::Char(c) => {
            let byte_pos = app
                .home_input
                .char_indices()
                .nth(app.home_cursor)
                .map(|(i, _)| i)
                .unwrap_or(app.home_input.len());
            app.home_input.insert(byte_pos, c);
            app.home_cursor += 1;
            // 用户在开头输入 / 时显示斜杠补全弹窗
            if c == '/' && app.home_input.chars().count() == 1 {
                app.slash_completion_open = true;
                app.slash_completion_sel = 0;
            }
        }
        _ => {}
    }
}

fn execute_home_command(cmd: &str, app: &mut App) {
    match cmd {
        "/help" | ":help" => {
            app.dialog = Some(Dialog::Help);
        }
        "/exit" | ":q" => {
            if app.exit_requested {
                app.exit_immediately = true;
            } else {
                app.exit_requested = true;
                app.add_log(LogEntry::Warn("Type /exit again to confirm".into()));
            }
        }
        "/clear" => {
            app.log.clear();
            app.scroll = 0;
        }
        "/theme" => {
            app.theme_mode = match app.theme_mode {
                super::theme::ThemeMode::Dark => super::theme::ThemeMode::Light,
                super::theme::ThemeMode::Light => super::theme::ThemeMode::Dark,
            };
            app.add_log(LogEntry::Info(format!("Theme: {:?}", app.theme_mode)));
        }
        _ => {
            app.add_log(LogEntry::Warn(format!("Unknown command: {cmd}")));
        }
    }
}

/// Render home.
pub fn render_home(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();

    // 垂直居中内容
    let total_height = area.height;
    let logo_height = LOGO_LINES.len() as u16;
    let content_height = logo_height + 2 + 3; // logo + gap + input
    let top_pad = total_height.saturating_sub(content_height) / 2;

    let [_, content, _] = Layout::vertical([
        Constraint::Length(top_pad),
        Constraint::Min(content_height),
        Constraint::Min(0),
    ])
    .areas(area);

    let [logo_area, input_area] =
        Layout::vertical([Constraint::Length(logo_height + 1), Constraint::Length(3)])
            .areas(content);

    // Logo
    let logo_text: Vec<Line> = LOGO_LINES
        .iter()
        .map(|l| {
            Line::from(Span::styled(
                *l,
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ))
        })
        .collect();

    let logo_para = Paragraph::new(Text::from(logo_text)).alignment(Alignment::Center);
    frame.render_widget(logo_para, logo_area);

    // 提示词输入
    let placeholder_idx = app.home_placeholder_idx % PLACEHOLDERS.len();
    let display_text = if app.home_input.is_empty() {
        PLACEHOLDERS[placeholder_idx]
    } else {
        app.home_input.as_str()
    };

    let input_style = if app.home_input.is_empty() {
        Style::default().fg(theme.muted)
    } else {
        Style::default().fg(theme.fg)
    };

    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if app.home_input.is_empty() {
            theme.border
        } else {
            theme.primary
        }));

    let input_para = Paragraph::new(Line::from(vec![
        Span::styled(
            "\u{276f} ",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(display_text, input_style),
    ]))
    .block(input_block);

    frame.render_widget(input_para, input_area);

    // 光标
    if app.home_input.is_empty() {
        frame.set_cursor_position((input_area.x + 3, input_area.y + 1));
    } else {
        let text_before: String = app.home_input.chars().take(app.home_cursor).collect();
        let display_col = text_before.width() as u16;
        frame.set_cursor_position((input_area.x + 3 + display_col, input_area.y + 1));
    }

    // 斜杠命令补全弹窗
    if app.slash_completion_open && !app.home_input.is_empty() {
        let completions = super::enhancements::filter_slash_commands(&app.home_input);
        if !completions.is_empty() {
            let popup_width = 50u16;
            let popup_height = (completions.len() as u16 + 2).min(12);
            let popup_x = input_area.x;
            let popup_y = input_area.y.saturating_sub(popup_height);

            let popup_area = Rect {
                x: popup_x,
                y: popup_y,
                width: popup_width.min(area.width - popup_x),
                height: popup_height,
            };

            use ratatui::widgets::{Clear, List, ListItem};
            frame.render_widget(Clear, popup_area);

            let items: Vec<ListItem> = completions
                .iter()
                .enumerate()
                .map(|(i, (cmd, _, _))| {
                    let style = if i == app.slash_completion_sel {
                        Style::default()
                            .fg(theme.primary)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.fg)
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("  {:<15}", cmd.name), style),
                        Span::styled(cmd.description, Style::default().fg(theme.muted)),
                    ]))
                })
                .collect();

            let list = List::new(items).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.primary))
                    .title(" Commands "),
            );
            frame.render_widget(list, popup_area);
        }
    }
}
