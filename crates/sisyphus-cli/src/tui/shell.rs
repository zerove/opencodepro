use std::time::Instant;

use crossterm::event::KeyCode;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use super::helpers::{copy_to_clipboard, paste_from_clipboard, scroll_pct};
use super::background::spawn_bridge_cmd;
use super::{App, LogEntry, Mode, Panel};
use crate::permission::CliPermission;
use crate::shell;

// ── Slash Commands ──────────────────────────────────────

struct SlashCmd {
    trigger: &'static str,
    desc: &'static str,
}

const SLASH_COMMANDS: &[SlashCmd] = &[
    SlashCmd { trigger: "/agent", desc: "Show agent mode status" },
    SlashCmd { trigger: "/agent on", desc: "Enable agent mode (bridge)" },
    SlashCmd { trigger: "/agent off", desc: "Disable agent mode" },
    SlashCmd { trigger: "/session", desc: "Session commands: /session export|import" },
    SlashCmd { trigger: "/session create", desc: "Create a session: /session create <name>" },
    SlashCmd { trigger: "/suggest", desc: "Ask agent for suggestions on current session" },
    SlashCmd { trigger: "/tool", desc: "Execute a tool: /tool <name> [args]" },
    SlashCmd { trigger: "/copy", desc: "Copy last assistant message to clipboard" },
    SlashCmd { trigger: "/paste", desc: "Paste from clipboard into shell input" },
    SlashCmd { trigger: "/help", desc: "Show available slash commands" },
    SlashCmd { trigger: "/clear", desc: "Clear the shell output" },
    SlashCmd { trigger: "/ralph on", desc: "Enable Ralph Loop (auto-continue)" },
    SlashCmd { trigger: "/ralph off", desc: "Disable Ralph Loop" },
    SlashCmd { trigger: "/ralph", desc: "Show Ralph Loop status" },
    SlashCmd { trigger: "/exit", desc: "Exit the application" },
    SlashCmd { trigger: "/panel 1", desc: "Switch to Shell panel" },
    SlashCmd { trigger: "/panel 2", desc: "Switch to Todos panel" },
    SlashCmd { trigger: "/panel 3", desc: "Switch to Sessions panel" },
    SlashCmd { trigger: "/panel 4", desc: "Switch to LSP panel" },
    SlashCmd { trigger: "/panel 5", desc: "Switch to SOP panel" },
    SlashCmd { trigger: "/panel 6", desc: "Switch to Roadmap panel" },
    SlashCmd { trigger: "/wisdom record", desc: "Record wisdom: /wisdom record <category> <content>" },
    SlashCmd { trigger: "/wisdom list", desc: "List all wisdom categories" },
    SlashCmd { trigger: "/wisdom inject", desc: "Inject all wisdom as context" },
    SlashCmd { trigger: "/wisdom", desc: "Show wisdom commands help" },
    SlashCmd { trigger: "/hashline", desc: "Hashline safety system: /hashline annotate|verify|edit <path> [args]" },
    SlashCmd { trigger: "/boulder save", desc: "Manually save TUI state to boulder.json" },
    SlashCmd { trigger: "/boulder status", desc: "Show boulder.json persistence status" },
];

fn matching_file_paths(partial: &str) -> Vec<String> {
    if partial.is_empty() {
        return vec![];
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let lower = partial.to_lowercase();
    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&cwd) {
        for entry in entries.flatten() {
            if let Ok(name) = entry.file_name().into_string() {
                if name.to_lowercase().contains(&lower) {
                    results.push(name);
                }
            }
        }
    }
    results.sort();
    results.truncate(10);
    results
}

fn extract_at_ref(input: &str) -> Option<(usize, String)> {
    let bytes = input.as_bytes();
    let mut at_pos = None;
    for (i, &b) in bytes.iter().enumerate().rev() {
        if b == b'@' {
            let prev_is_space = i == 0 || bytes[i - 1].is_ascii_whitespace();
            if prev_is_space {
                at_pos = Some(i);
                break;
            }
        }
    }
    let pos = at_pos?;
    let after = input[pos + 1..].to_string();
    Some((pos, after))
}

fn matching_slash_commands(input: &str) -> Vec<&'static SlashCmd> {
    if !input.starts_with('/') {
        return vec![];
    }
    let partial = input.to_lowercase();
    SLASH_COMMANDS
        .iter()
        .filter(|cmd| cmd.trigger.starts_with(&partial) || cmd.trigger.to_lowercase().contains(&partial))
        .collect()
}

// ── Key Handler ─────────────────────────────────────────

pub async fn handle_shell_key(code: KeyCode, app: &mut App, perms: &CliPermission) {
    match app.mode {
        Mode::Input => match code {
            KeyCode::Enter => {
                let cmd = app.input.trim().to_string();
                if cmd.is_empty() {
                    return;
                }

                let matches = matching_slash_commands(&app.input);
                if !matches.is_empty() {
                    let sel = app.slash_sel.min(matches.len().saturating_sub(1));
                    let selected_cmd = matches[sel].trigger.to_string();
                    app.input = selected_cmd.clone();
                    app.cursor_pos = app.input.len();
                    app.slash_sel = 0;
                }

                let cmd = app.input.trim().to_string();
                if cmd.is_empty() {
                    return;
                }
                if app.history.last().is_none_or(|last| last != &cmd) {
                    app.history.push(cmd.clone());
                }
                app.history_idx = None;
                app.input.clear();
                app.cursor_pos = 0;

                match cmd.as_str() {
                    "exit" | "quit" | ":q" => {
                        if app.exit_requested {
                            app.exit_immediately = true;
                            return;
                        }
                        app.exit_requested = true;
                        app.add_log(LogEntry::Warn(
                            "Type `exit` again to confirm, or `clear` to dismiss.".into(),
                        ));
                        return;
                    }
                    "clear" | ":c" => {
                        app.exit_requested = false;
                        app.log.clear();
                        app.scroll = 0;
                        return;
                    }
                    "/boulder save" => {
                        app.boulder_dirty = true;
                        app.save_boulder();
                        app.add_log(LogEntry::Success("💾 TUI state saved to boulder.json".into()));
                        return;
                    }
                    "/boulder status" => {
                        let exists = app.boulder_path.exists();
                        let last_updated = if exists {
                            std::fs::metadata(&app.boulder_path)
                                .and_then(|m| m.modified())
                                .map(|t| format!("{:?}", t))
                                .unwrap_or_else(|_| "unknown".into())
                        } else {
                            "N/A".into()
                        };
                        app.add_log(LogEntry::Info(format!(
                            "Boulder Status: exists={}, last_updated={}",
                            exists, last_updated
                        )));
                        return;
                    }
                    ":help" | ":h" | "/help" => {
                        app.add_log(LogEntry::Info("── Available commands ──".into()));
                        app.add_log(LogEntry::Info(
                            "  exit / quit / :q    Exit (type twice)".into(),
                        ));
                        app.add_log(LogEntry::Info("  clear / :c          Clear log".into()));
                        app.add_log(LogEntry::Info("  /session export/import  Export/Import sessions".into()));
                        app.add_log(LogEntry::Info(
                            "  :bridge status      Bridge health check".into(),
                        ));
                        app.add_log(LogEntry::Info(
                            "  :bridge sops        List registered SOPs".into(),
                        ));
                        app.add_log(LogEntry::Info("  :bridge run <id>    Execute a SOP".into()));
                        app.add_log(LogEntry::Info(
                            "  :help / :h          Show this help".into(),
                        ));
                        app.add_log(LogEntry::Info("  /ralph on/off       Toggle Ralph Loop".into()));
                        app.add_log(LogEntry::Info("  /boulder save        Save TUI state".into()));
                        app.add_log(LogEntry::Info("  /boulder status      Check persistence status".into()));
                        app.add_log(LogEntry::Info("  /wisdom record/list/inject  Manage cross-session wisdom".into()));
                        app.add_log(LogEntry::Info("  ------------".into()));
                        app.add_log(LogEntry::Info("  ↑/↓    Browse command history".into()));
                        app.add_log(LogEntry::Info("  Tab    Autocomplete / slash + @file".into()));
                        app.add_log(LogEntry::Info("  /xxx   Show slash command suggestions".into()));
                        app.add_log(LogEntry::Info("  @file  Autocomplete file paths".into()));
                        app.add_log(LogEntry::Info(
                            "  Any other text runs as a shell command".into(),
                        ));
                        return;
                    }
                    _ if cmd.starts_with(":bridge ") || cmd.starts_with("/bridge ") || cmd == ":bridge" || cmd == "/bridge" => {
                        app.add_log(LogEntry::Info("🔗 Processing bridge command...".into()));
                        if app.pending_op.is_none() {
                            app.pending_op = Some(spawn_bridge_cmd(
                                app.bridge_address.clone(),
                                cmd,
                            ));
                        } else {
                            app.add_log(LogEntry::Warn(
                                "Another bridge operation is already pending.".into(),
                            ));
                        }
                        return;
                    }
                    _ if cmd.starts_with("/session export ") => {
                        let rest = cmd.strip_prefix("/session export ").unwrap_or("").trim().to_string();
                        let parts: Vec<&str> = rest.splitn(3, ' ').collect();
                        if parts.len() < 2 {
                            app.add_log(LogEntry::Error("Usage: /session export <id> <output-path> [format=json]".into()));
                            return;
                        }
                        let id = parts[0].to_string();
                        let output = std::path::PathBuf::from(parts[1]);
                        let format = parts.get(2).copied().unwrap_or("json");
                        let db = app.data_dir.join("sessions.json");
                        match format {
                            "html" => match sisyphus_tools::session_export_html(&db, &id, &output) {
                                Ok(()) => app.add_log(LogEntry::Success(format!("Session {id} exported as HTML to {}", output.display()))),
                                Err(e) => app.add_log(LogEntry::Error(format!("Export failed: {e}"))),
                            },
                            _ => match sisyphus_tools::session_export(&db, &id, &output) {
                                Ok(()) => app.add_log(LogEntry::Success(format!("Session {id} exported as JSON to {}", output.display()))),
                                Err(e) => app.add_log(LogEntry::Error(format!("Export failed: {e}"))),
                            },
                        }
                        return;
                    }
                    _ if cmd.starts_with("/session import ") => {
                        let path_str = cmd.strip_prefix("/session import ").unwrap_or("").trim();
                        let input = std::path::PathBuf::from(path_str);
                        let db = app.data_dir.join("sessions.json");
                        match sisyphus_tools::session_import(&db, &input) {
                            Ok(session) => {
                                app.add_log(LogEntry::Success(format!("Imported session {} ({})", session.id, session.name)));
                                app.session_dirty = true;
                            }
                            Err(e) => app.add_log(LogEntry::Error(format!("Import failed: {e}"))),
                        }
                        return;
                    }
                    _ if cmd.starts_with("/session create") => {
                        let name = cmd.strip_prefix("/session create").unwrap_or("").trim();
                        if name.is_empty() {
                            app.add_log(LogEntry::Error("Usage: /session create <name>".into()));
                            return;
                        }
                        let db = app.data_dir.join("sessions.json");
                        match sisyphus_tools::session_create(&db, name) {
                            Ok(s) => {
                                app.add_log(LogEntry::Success(format!("Created session {} ({})", s.id, s.name)));
                                app.session_dirty = true;
                            }
                            Err(e) => app.add_log(LogEntry::Error(format!("Create failed: {e}"))),
                        }
                        return;
                    }
                    _ if cmd == "/session" || cmd.starts_with("/session ") => {
                        app.add_log(LogEntry::Info("Session commands:".into()));
                        app.add_log(LogEntry::Info("  /session export <id> <path> [format]  Export session (json/html)".into()));
                        app.add_log(LogEntry::Info("  /session import <path>               Import session from file".into()));
                        app.add_log(LogEntry::Info("  /session create <name>              Create a new session".into()));
                        return;
                    }
                    "/agent off" => {
                        app.agent_mode = false;
                        app.add_log(LogEntry::Info("Agent mode disabled — messages saved locally only".into()));
                        return;
                    }
                    "/agent on" => {
                        app.agent_mode = true;
                        app.add_log(LogEntry::Success("Agent mode enabled — messages sent to bridge for AI response".into()));
                        return;
                    }
                    "/ralph on" => {
                        app.ralph_enabled = true;
                        app.add_log(LogEntry::Success("Ralph Loop enabled — will auto-continue after 5s idle".into()));
                        return;
                    }
                    "/ralph off" => {
                        app.ralph_enabled = false;
                        app.add_log(LogEntry::Info("Ralph Loop disabled".into()));
                        return;
                    }
                    "/ralph" => {
                        app.add_log(LogEntry::Info(format!(
                            "Ralph Loop: {} | Idle cycles: {}",
                            if app.ralph_enabled { "ON" } else { "OFF" },
                            app.ralph_idle_cycles
                        )));
                        return;
                    }
                    "/agent status" | "/agent" => {
                        app.add_log(LogEntry::Info(format!(
                            "Agent mode: {} | Bridge: {} | Pending: {}",
                            if app.agent_mode { "ON" } else { "OFF" },
                            if app.bridge_address.is_empty() { "not set" } else { &app.bridge_address },
                            if app.pending_op.is_some() { "yes" } else { "no" },
                        )));
                        return;
                    }
                    _ if cmd.starts_with("/panel ") => {
                        let num = cmd.strip_prefix("/panel ").unwrap_or("").trim();
                        match num {
                            "1" => app.active_panel = Panel::Shell,
                            "2" => app.active_panel = Panel::Todos,
                            "3" => app.active_panel = Panel::Sessions,
                            "4" => app.active_panel = Panel::Lsp,
                            "5" => app.active_panel = Panel::Sop,
                            "6" => app.active_panel = Panel::Roadmap,
                            _ => app.add_log(LogEntry::Warn(format!("Unknown panel: {num}. Use 1-6."))),
                        }
                    }
                    _ if cmd == "/suggest" || cmd.starts_with("/suggest ") => {
                        if app.pending_op.is_some() {
                            app.add_log(LogEntry::Warn("An operation is already pending. Wait for it to finish.".into()));
                            return;
                        }
                        let topic = cmd.strip_prefix("/suggest").unwrap_or("").trim();
                        let prompt = if topic.is_empty() {
                            "Suggest what to do next in the current context.".to_string()
                        } else {
                            format!("Suggest what to do next regarding: {topic}")
                        };
                        app.add_log(LogEntry::Info(format!("🤔 Requesting suggestion...")));
                        app.add_log(LogEntry::Info(format!("Suggestion requested: {prompt}")));
                        return;
                    }
                    _ if cmd.starts_with("/tool") => {
                        let args = cmd.strip_prefix("/tool").unwrap_or("").trim();
                        if args.is_empty() {
                            app.add_log(LogEntry::Error("Usage: /tool <name> [args] — e.g. /tool session_list".into()));
                            return;
                        }
                        app.add_log(LogEntry::Info(format!("🔧 Queuing tool execution: {args}")));
                        app.add_log(LogEntry::Info("Tool execution not yet integrated with bridge — logged for future use.".into()));
                        return;
                    }
                    "/copy" | "/yank" => {
                        if let Some(ref detail) = app.session_detail {
                            let last_assistant = detail.messages
                                .iter()
                                .rev()
                                .find(|m| m.role == "assistant")
                                .map(|m| m.content.as_str());
                            if let Some(content) = last_assistant {
                                copy_to_clipboard(content);
                                app.copy_notification = Some(Instant::now());
                                app.add_log(LogEntry::Success("📋 Copied last assistant message to clipboard".into()));
                            } else {
                                app.add_log(LogEntry::Warn("No assistant message to copy".into()));
                            }
                        } else {
                            app.add_log(LogEntry::Warn("Open a session detail view first".into()));
                        }
                        return;
                    }
                    "/paste" => {
                        let pasted = paste_from_clipboard();
                        if !pasted.is_empty() {
                            app.input.insert_str(app.cursor_pos, &pasted);
                            app.cursor_pos += pasted.len();
                        }
                        app.add_log(LogEntry::Success("📋 Pasted from clipboard".into()));
                        return;
                    }
                    "/clear" => {
                        app.log.clear();
                        app.scroll = 0;
                    }
                    "/exit" => {
                        app.exit_requested = true;
                    }
                    _ if cmd.starts_with("/hashline annotate ") => {
                        let path_str = cmd.strip_prefix("/hashline annotate ").unwrap_or("").trim();
                        let path = std::path::PathBuf::from(path_str);
                        match sisyphus_tools::hashline::annotate_file(&path) {
                            Ok(count) => app.add_log(LogEntry::Success(format!("Hashline: annotated {} lines in {}", count, path.display()))),
                            Err(e) => app.add_log(LogEntry::Error(format!("Hashline annotate failed: {e}"))),
                        }
                        return;
                    }
                    _ if cmd.starts_with("/hashline verify ") => {
                        let path_str = cmd.strip_prefix("/hashline verify ").unwrap_or("").trim();
                        let path = std::path::PathBuf::from(path_str);
                        match std::fs::read_to_string(&path) {
                            Ok(content) => {
                                let mut valid = 0;
                                let mut invalid = 0;
                                for line in content.lines() {
                                    if sisyphus_tools::hashline::parse_hashline(line).is_some() {
                                        if sisyphus_tools::hashline::verify_line(&content, line).unwrap_or(false) {
                                            valid += 1;
                                        } else {
                                            invalid += 1;
                                            app.add_log(LogEntry::Warn(format!("Invalid: {}", line.trim())));
                                        }
                                    }
                                }
                                app.add_log(LogEntry::Success(format!("Hashline verify: {valid} valid, {invalid} invalid in {}", path.display())));
                            }
                            Err(e) => app.add_log(LogEntry::Error(format!("Hashline verify failed: {e}"))),
                        }
                        return;
                    }
                    _ if cmd.starts_with("/hashline edit ") => {
                        let rest = cmd.strip_prefix("/hashline edit ").unwrap_or("").trim().to_string();
                        let parts: Vec<&str> = rest.splitn(3, ' ').collect();
                        if parts.len() < 3 {
                            app.add_log(LogEntry::Error("Usage: /hashline edit <path> <hashline> <new-content>".into()));
                            return;
                        }
                        let path = std::path::PathBuf::from(parts[0]);
                        let hashline = parts[1].to_string();
                        let new_content = parts[2].to_string();
                        match sisyphus_tools::hashline::edit_with_hashline(&path, &hashline, &new_content) {
                            Ok(old) => app.add_log(LogEntry::Success(format!("Hashline edit: '{}' → '{}'", old, new_content))),
                            Err(e) => app.add_log(LogEntry::Error(format!("Hashline edit failed: {e}"))),
                        }
                        return;
                    }
                    _ if cmd == "/hashline" || cmd.starts_with("/hashline ") => {
                        app.add_log(LogEntry::Info("Hashline commands:".into()));
                        app.add_log(LogEntry::Info("  /hashline annotate <path>  Add LINE#ID comments".into()));
                        app.add_log(LogEntry::Info("  /hashline verify <path>   Verify LINE#ID integrity".into()));
                        app.add_log(LogEntry::Info("  /hashline edit <path> <id> <text>  Edit by LINE#ID".into()));
                        return;
                    }
                    _ if cmd.starts_with("/wisdom record ") => {
                        let rest = cmd.strip_prefix("/wisdom record ").unwrap_or("").trim().to_string();
                        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
                        if parts.len() < 2 {
                            app.add_log(LogEntry::Error("Usage: /wisdom record <category> <content>".into()));
                            return;
                        }
                        let category = parts[0].to_string();
                        let content = parts[1].to_string();
                        app.add_log(LogEntry::Info(format!("📝 Recording wisdom to '{category}'...")));
                        match sisyphus_tools::wisdom_record(&category, &content).await {
                            Ok(()) => app.add_log(LogEntry::Success(format!("Wisdom recorded in '{category}'"))),
                            Err(e) => app.add_log(LogEntry::Error(format!("Wisdom record failed: {e}"))),
                        }
                        return;
                    }
                    _ if cmd == "/wisdom list" || cmd.starts_with("/wisdom list ") => {
                        match sisyphus_tools::wisdom_list().await {
                            Ok(categories) => {
                                if categories.is_empty() {
                                    app.add_log(LogEntry::Info("No wisdom categories found.".into()));
                                } else {
                                    app.add_log(LogEntry::Info("Available wisdom categories:".into()));
                                    for cat in categories {
                                        app.add_log(LogEntry::Info(format!("  📚 {cat}")));
                                    }
                                }
                            }
                            Err(e) => app.add_log(LogEntry::Error(format!("Wisdom list failed: {e}"))),
                        }
                        return;
                    }
                    _ if cmd == "/wisdom inject" || cmd.starts_with("/wisdom inject ") => {
                        match sisyphus_tools::wisdom_inject().await {
                            Ok(context) => {
                                if context.is_empty() {
                                    app.add_log(LogEntry::Info("No wisdom to inject.".into()));
                                } else {
                                    let line_count = context.lines().count();
                                    let char_count = context.len();
                                    app.add_log(LogEntry::Success(format!("Wisdom injected: {line_count} lines, {char_count} chars")));
                                    app.add_log(LogEntry::Info("Context block:".into()));
                                    app.add_log(LogEntry::Info(context));
                                }
                            }
                            Err(e) => app.add_log(LogEntry::Error(format!("Wisdom inject failed: {e}"))),
                        }
                        return;
                    }
                    _ if cmd == "/wisdom" || cmd.starts_with("/wisdom ") => {
                        app.add_log(LogEntry::Info("Wisdom commands:".into()));
                        app.add_log(LogEntry::Info("  /wisdom record <category> <content>  Record cross-session knowledge".into()));
                        app.add_log(LogEntry::Info("  /wisdom list                        List all wisdom categories".into()));
                        app.add_log(LogEntry::Info("  /wisdom inject                      Inject all wisdom as context".into()));
                        return;
                    }
                    _ => {}
                }

                app.add_log(LogEntry::Info(format!("$ {cmd}")));
                let start = Instant::now();
                match shell::exec(&cmd, None, 30000, perms).await {
                    Ok(output) => {
                        let dur = shell::format_duration(output.duration);
                        if !output.stdout.is_empty() {
                            for line in output.stdout.lines() {
                                app.add_log(LogEntry::Success(line.to_string()));
                            }
                        }
                        app.add_log(LogEntry::Info(format!(
                            "→ exit={} ({dur})",
                            output.exit_code
                        )));
                    }
                    Err(e) => {
                        let dur = shell::format_duration(start.elapsed());
                        app.add_log(LogEntry::Error(format!("✗ {e} ({dur})")));
                    }
                }
            }
            KeyCode::Up => {
                if app.history.is_empty() {
                    return;
                }
                let new_idx = match app.history_idx {
                    None => Some(app.history.len() - 1),
                    Some(0) => None,
                    Some(i) => Some(i - 1),
                };
                app.history_idx = new_idx;
                app.input = new_idx.map(|i| app.history[i].clone()).unwrap_or_default();
                app.cursor_pos = app.input.len();
            }
            KeyCode::Down => {
                if app.history.is_empty() {
                    return;
                }
                let new_idx = match app.history_idx {
                    None => None,
                    Some(i) if i + 1 >= app.history.len() => None,
                    Some(i) => Some(i + 1),
                };
                app.history_idx = new_idx;
                app.input = new_idx.map(|i| app.history[i].clone()).unwrap_or_default();
                app.cursor_pos = app.input.len();
            }
            KeyCode::Char(c) => {
                app.history_idx = None;
                app.input.insert(app.cursor_pos, c);
                app.cursor_pos += c.len_utf8();
            }
            KeyCode::Backspace => {
                if app.cursor_pos > 0 {
                    if let Some(idx) = app.input[..app.cursor_pos].char_indices().next_back() {
                        app.cursor_pos = idx.0;
                        app.input.remove(app.cursor_pos);
                    }
                }
            }
            KeyCode::Delete => {
                if app.cursor_pos < app.input.len() {
                    if let Some(idx) = app.input[app.cursor_pos..].char_indices().nth(1) {
                        let end_idx = app.cursor_pos + idx.0;
                        app.input.drain(app.cursor_pos..end_idx);
                    } else {
                        app.input.pop();
                    }
                }
            }
            KeyCode::Left => {
                if let Some(idx) = app.input[..app.cursor_pos].char_indices().next_back() {
                    app.cursor_pos = idx.0;
                }
            }
            KeyCode::Right => {
                if let Some(idx) = app.input[app.cursor_pos..].char_indices().nth(1) {
                    app.cursor_pos += idx.0;
                }
            }
            KeyCode::Home => app.cursor_pos = 0,
            KeyCode::End => app.cursor_pos = app.input.len(),
            KeyCode::Esc => {
                app.input.clear();
                app.cursor_pos = 0;
            }
            KeyCode::Tab => {
                if app.input.is_empty() {
                    app.mode = Mode::Scroll;
                } else if app.input.starts_with('/') {
                    let matches = matching_slash_commands(&app.input);
                    if !matches.is_empty() {
                        let next = app.slash_sel + 1;
                        app.slash_sel = if next >= matches.len() { 0 } else { next };
                    }
                } else if let Some((at_pos, ref partial)) = extract_at_ref(&app.input) {
                    let files = matching_file_paths(partial);
                    if !files.is_empty() {
                        app.slash_sel = 0;
                        let sel = app.slash_sel;
                        let selected = files[sel].clone();
                        let before = &app.input[..at_pos];
                        let after = &app.input[at_pos + 1 + partial.len()..];
                        app.input = format!("{}@{}{}", before, selected, after);
                        app.cursor_pos = app.input.len();
                        app.slash_sel = 0;
                    }
                } else {
                    let completions: Vec<&str> = match app.input.as_str() {
                        s if s.starts_with(":b") => {
                            vec![":bridge status", ":bridge sops", ":bridge run "]
                        }
                        s if s.starts_with(":h") => vec![":help", ":h"],
                        s if s.starts_with(":c") => vec![":c", "clear"],
                        s if s.starts_with(":q") => vec![":q", "quit", "exit"],
                        s if s.starts_with(":") => vec![":bridge ", ":help", ":c", ":q"],
                        s if s.starts_with("e") => vec!["exit"],
                        s if s.starts_with("c") => vec!["clear"],
                        s if s.starts_with("q") => vec!["quit"],
                        _ => vec![],
                    };
                    if !completions.is_empty() {
                        let sel = app.slash_sel.min(completions.len().saturating_sub(1));
                        app.input = completions[sel].to_string();
                        app.cursor_pos = app.input.len();
                        app.slash_sel = (app.slash_sel + 1) % completions.len();
                        app.history_idx = None;
                    }
                }
            }
            _ => {}
        },
        Mode::Scroll => match code {
            KeyCode::Up => app.scroll = app.scroll.saturating_sub(1),
            KeyCode::Down => app.scroll = (app.scroll + 1).min(app.log.len().saturating_sub(1)),
            KeyCode::PageUp => app.scroll = app.scroll.saturating_sub(10),
            KeyCode::PageDown => {
                app.scroll = (app.scroll + 10).min(app.log.len().saturating_sub(1))
            }
            KeyCode::Home | KeyCode::Char('g') => app.scroll = 0,
            KeyCode::End | KeyCode::Char('G') => app.scroll = app.log.len().saturating_sub(1),
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(entry) = app.log.get(app.scroll) {
                    let level = match entry {
                        LogEntry::Info(_) => "INFO",
                        LogEntry::Success(_) => "OK",
                        LogEntry::Warn(_) => "WARN",
                        LogEntry::Error(_) => "ERR",
                    };
                    let text = format!("[{}] {}",
                        level,
                        entry.entry_text(),
                    );
                    copy_to_clipboard(&text);
                    app.copy_notification = Some(Instant::now());
                    app.add_log(LogEntry::Success("📋 Copied log entry to clipboard".into()));
                }
            }
            KeyCode::Tab | KeyCode::Esc => app.mode = Mode::Input,
            _ => {}
        },
        Mode::Select => {}
    }
}

// ── Renderer ────────────────────────────────────────────

pub fn render_shell(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let suggestion_count = matching_slash_commands(&app.input).len();
    let theme = app.theme();
    let suggestion_height = if app.input.starts_with('/') && suggestion_count > 0 {
        (suggestion_count as u16).min(8) + 2
    } else {
        0
    };
    let [input_area, suggestion_area, log_area] =
        Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(suggestion_height),
            Constraint::Min(0),
        ]).areas(area);

    let input_block = Block::default()
        .borders(Borders::ALL)
        .title(" Command ")
        .border_style(if app.mode == Mode::Input {
            Style::default().fg(theme.primary)
        } else {
            Style::default().fg(theme.border)
        });
    let input_paragraph = Paragraph::new(app.input.as_str())
        .block(input_block)
        .style(Style::default());
    frame.render_widget(input_paragraph, input_area);

    if app.mode == Mode::Input {
        let visual_cursor_pos: u16 = app.input[..app.cursor_pos]
            .chars()
            .map(|c| if c.is_ascii() { 1 } else { 2 })
            .sum();
        let cursor_x = input_area.x + 1 + visual_cursor_pos;
        let cursor_y = input_area.y + 1;
        frame.set_cursor_position(ratatui::layout::Position::new(cursor_x, cursor_y));
    }

    let _show_suggestions = if app.input.starts_with('/') {
        let matches = matching_slash_commands(&app.input);
        if !matches.is_empty() {
            let suggestion_sel = app.slash_sel.min(matches.len().saturating_sub(1));
            let suggestion_block = Block::default()
                .borders(Borders::ALL)
                .title(" Commands ")
                .border_style(Style::default().fg(theme.border));
            let suggestion_items: Vec<ListItem> = matches
                .iter()
                .enumerate()
                .map(|(i, cmd)| {
                    let prefix = if i == suggestion_sel { " > " } else { "   " };
                    let line = Line::from(vec![
                        Span::styled(prefix, Style::default().fg(theme.primary)),
                        Span::styled(cmd.trigger, Style::default().fg(theme.fg)),
                        Span::raw("  "),
                        Span::styled(cmd.desc, Style::default().fg(theme.muted)),
                    ]);
                    ListItem::new(line)
                })
                .collect();
            let suggestion_list = List::new(suggestion_items)
                .block(suggestion_block)
                .highlight_style(Style::default().fg(theme.secondary).add_modifier(Modifier::REVERSED));
            frame.render_widget(suggestion_list, suggestion_area);
            true
        } else {
            false
        }
    } else if let Some((_pos, ref partial)) = extract_at_ref(&app.input) {
        let files = matching_file_paths(partial);
        if !files.is_empty() {
            let file_sel = app.slash_sel.min(files.len().saturating_sub(1));
            let file_block = Block::default()
                .borders(Borders::ALL)
                .title(" Files ")
                .border_style(Style::default().fg(theme.accent));
            let file_items: Vec<ListItem> = files
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    let prefix = if i == file_sel { " > " } else { "   " };
                    let line = Line::from(vec![
                        Span::styled(prefix, Style::default().fg(theme.accent)),
                        Span::styled(name, Style::default().fg(theme.fg)),
                    ]);
                    ListItem::new(line)
                })
                .collect();
            let file_list = List::new(file_items)
                .block(file_block)
                .highlight_style(Style::default().fg(theme.secondary).add_modifier(Modifier::REVERSED));
            frame.render_widget(file_list, suggestion_area);
            true
        } else {
            false
        }
    } else {
        false
    };

    let log_block = Block::default()
        .borders(Borders::ALL)
        .title(" Output ")
        .title_bottom(scroll_pct(app.scroll, app.log.len()))
        .border_style(if app.mode == Mode::Scroll {
            Style::default().fg(theme.primary)
        } else {
            Style::default().fg(theme.border)
        });

    let log_lines: Vec<Line> = app
        .log
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let style = match entry {
                LogEntry::Info(_) => Style::default().fg(theme.fg),
                LogEntry::Success(_) => Style::default().fg(theme.success),
                LogEntry::Warn(_) => Style::default().fg(theme.warning),
                LogEntry::Error(_) => Style::default().fg(theme.error),
            };
            let text = entry.entry_text();
            Line::from(vec![
                Span::styled(format!("{:>4} ", i), Style::default().fg(theme.muted)),
                Span::styled(text, style),
            ])
        })
        .collect();

    let log_height = log_area.height.saturating_sub(2) as usize;
    let text_height = log_lines.len();
    let scroll_y = app.scroll.saturating_sub(log_height.saturating_sub(1));
    let scroll_y = scroll_y.min(text_height.saturating_sub(log_height));

    let paragraph = Paragraph::new(Text::from(log_lines))
        .block(log_block)
        .wrap(Wrap { trim: false })
        .scroll((scroll_y as u16, 0));
    frame.render_widget(paragraph, log_area);
}
