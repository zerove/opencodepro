use super::super::dialog::Dialog;
use super::super::{App, DevFocus, LogEntry, View};

// === 光标感知的输入编辑辅助 ===

fn cursor_char_to_byte(s: &str, char_idx: usize) -> usize {
    s.chars().take(char_idx).map(|c| c.len_utf8()).sum()
}

pub(super) fn cursor_insert(s: &mut String, cursor: &mut usize, c: char) {
    let byte_idx = cursor_char_to_byte(s, *cursor);
    s.insert(byte_idx, c);
    *cursor += 1;
}

pub(super) fn cursor_backspace(s: &mut String, cursor: &mut usize) {
    if *cursor > 0 {
        *cursor -= 1;
        let byte_idx = cursor_char_to_byte(s, *cursor);
        s.remove(byte_idx);
    }
}

pub(super) fn cursor_delete(s: &mut String, cursor: usize) {
    if let Some((byte_idx, _)) = s.char_indices().nth(cursor) {
        s.remove(byte_idx);
    }
}

pub(super) fn cursor_delete_word_backward(s: &mut String, cursor: &mut usize) {
    if *cursor == 0 {
        return;
    }
    let chars: Vec<char> = s.chars().collect();
    let mut end = *cursor;
    while end > 0 && chars[end - 1].is_whitespace() {
        end -= 1;
    }
    while end > 0 && !chars[end - 1].is_whitespace() {
        end -= 1;
    }
    let byte_start: usize = chars[..end].iter().map(|c| c.len_utf8()).sum();
    let byte_end: usize = chars[..*cursor].iter().map(|c| c.len_utf8()).sum();
    s.drain(byte_start..byte_end);
    *cursor = end;
}

pub(super) fn cursor_delete_to_end(s: &mut String, cursor: usize) {
    let byte_start = cursor_char_to_byte(s, cursor);
    s.truncate(byte_start);
}

// === 斜杠命令分发 ===

pub(super) fn update_slash_suggestions(app: &mut App) {
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
        let matches: Vec<String> = all_commands
            .iter()
            .filter(|(cmd, _)| cmd.starts_with(&format!("/{rest}")) || rest.is_empty())
            .map(|(cmd, desc)| format!("{cmd:<12} {desc}"))
            .collect();
        app.session_suggestions = matches;
    } else {
        app.session_suggestions = vec![
            "Type /help for keyboard shortcuts".into(),
            "Type /dev to open dev dialog".into(),
            "Try Ctrl+U/W/A/E for input editing".into(),
        ];
    }
}

pub(super) fn dispatch_slash_command(app: &mut App, cmd: &str) -> bool {
    let (cmd_name, args) = cmd.split_once(' ').unwrap_or((cmd, ""));
    match cmd_name {
        "/help" | "/h" => {
            app.dialog = Some(Dialog::Help);
            true
        }
        "/dev" | "/d" => {
            let goal = if args.is_empty() {
                String::new()
            } else {
                args.trim().to_string()
            };
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
            app.view = View::Home;
            true
        }
        "/sessions" => {
            app.view = View::Session;
            true
        }
        "/clear" | "/c" => {
            app.session_input.clear();
            app.session_cursor = 0;
            true
        }
        "/new" | "/n" => {
            let db = app.data_dir.join("sessions.json");
            match crate::tools::session_create(&db, "unnamed") {
                Ok(s) => {
                    app.add_log(LogEntry::Success(format!(
                        "Created session {} ({})",
                        s.id, s.name
                    )));
                    app.session_dirty = true;
                }
                Err(e) => app.add_log(LogEntry::Error(format!("Create failed: {e}"))),
            }
            true
        }
        _ => false,
    }
}
