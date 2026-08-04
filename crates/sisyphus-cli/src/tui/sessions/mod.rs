mod editing;
mod renders;

pub use renders::render_sessions;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::background::spawn_session_add_message;
use super::dialog::Dialog;
use super::helpers::copy_to_clipboard;
use super::{App, LogEntry, Mode, ToastKind};

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
                        let handled = editing::dispatch_slash_command(app, &content);
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
                            session.messages.push(crate::tools::Message {
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
                // === 取消 / 退出输入 ===
                KeyCode::Esc => {
                    app.session_mode = Mode::Select;
                    app.session_input.clear();
                    app.session_cursor = 0;
                }
                // === 基础编辑 ===
                KeyCode::Backspace => {
                    editing::cursor_backspace(&mut app.session_input, &mut app.session_cursor);
                    editing::update_slash_suggestions(app);
                }
                KeyCode::Delete => {
                    editing::cursor_delete(&mut app.session_input, app.session_cursor);
                    editing::update_slash_suggestions(app);
                }
                // === 方向键导航 ===
                KeyCode::Left => app.session_cursor = app.session_cursor.saturating_sub(1),
                KeyCode::Right => {
                    app.session_cursor = (app.session_cursor + 1).min(app.session_input.len())
                }
                KeyCode::Home => app.session_cursor = 0,
                KeyCode::End => app.session_cursor = app.session_input.len(),
                // === 历史循环（↑/↓） ===
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
                        editing::update_slash_suggestions(app);
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
                        editing::update_slash_suggestions(app);
                    }
                }
                // === Ctrl+ 组合键 ===
                KeyCode::Char('a') if ctrl => app.session_cursor = 0,
                KeyCode::Char('e') if ctrl => app.session_cursor = app.session_input.len(),
                KeyCode::Char('u') if ctrl => {
                    app.session_input.clear();
                    app.session_cursor = 0;
                    editing::update_slash_suggestions(app);
                }
                KeyCode::Char('w') if ctrl => {
                    editing::cursor_delete_word_backward(
                        &mut app.session_input,
                        &mut app.session_cursor,
                    );
                    editing::update_slash_suggestions(app);
                }
                KeyCode::Char('k') if ctrl => {
                    editing::cursor_delete_to_end(&mut app.session_input, app.session_cursor);
                    editing::update_slash_suggestions(app);
                }
                // === 普通字符输入 ===
                KeyCode::Char(c) => {
                    editing::cursor_insert(&mut app.session_input, &mut app.session_cursor, c);
                    editing::update_slash_suggestions(app);
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
                    app.add_log(LogEntry::Info(format!(
                        "Thought blocks: {expanded}/{total} expanded"
                    )));
                }
                KeyCode::Char('t') => {
                    if let Some(ref detail) = app.session_detail {
                        if let Some(idx) =
                            detail.messages.iter().rposition(|m| m.role == "assistant")
                        {
                            app.selected_msg = Some(idx);
                            app.toggle_message_thoughts(idx);
                            let state = if app.is_thought_expanded(idx) {
                                "expanded"
                            } else {
                                "collapsed"
                            };
                            app.add_log(LogEntry::Info(format!(
                                "Toggled thought blocks for message {idx} ({state})"
                            )));
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
                    // 复制选中的消息；无选中时回退到最后一条助手消息
                    if let Some(ref detail) = app.session_detail {
                        let content = app
                            .selected_msg
                            .and_then(|i| detail.messages.get(i))
                            .map(|m| m.content.as_str())
                            .or_else(|| {
                                detail
                                    .messages
                                    .iter()
                                    .rev()
                                    .find(|m| m.role == "assistant")
                                    .map(|m| m.content.as_str())
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
                    // 将会话详情全文复制到剪贴板
                    if let Some(ref detail) = app.session_detail {
                        let mut all_text = String::new();
                        for msg in &detail.messages {
                            let role = match msg.role.as_str() {
                                "user" => "You",
                                "assistant" => "Sisyphus",
                                r => r,
                            };
                            all_text.push_str(&format!(
                                "[{}] {}\n{}\n\n",
                                role, msg.created_at, msg.content
                            ));
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
                KeyCode::Char('E') => {
                    app.stream_show = !app.stream_show;
                    app.add_log(LogEntry::Info(if app.stream_show {
                        "Streaming events: shown".into()
                    } else {
                        "Streaming events: hidden".into()
                    }));
                }
                // ── 搜索（Ctrl+F） ──
                KeyCode::Char('f') if ctrl => {
                    if app.search_active {
                        app.search_active = false;
                        app.search_query.clear();
                        app.search_results.clear();
                    } else {
                        app.search_active = true;
                    }
                }
                // ── 搜索激活：字符输入、导航、退出 ──
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
                            // 其他按键取消搜索
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
            // 用空名称立即创建 → 服务器分配 ID
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
            if let Some(&idx) = filtered.get(app.session_sel.min(filtered.len().saturating_sub(1)))
            {
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
