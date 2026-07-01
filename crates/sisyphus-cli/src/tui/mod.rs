use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, MouseButton};
use crossterm::event::MouseEventKind;
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    DefaultTerminal,
};

mod background;
mod dialog;
mod helpers;
mod home;
mod sessions;
mod theme;
pub(crate) use background::BgOp;
pub(crate) use background::{
    spawn_fetch_roadmap, spawn_list_sops,
};
pub(crate) use helpers::paste_from_clipboard;
use dialog::{handle_dialog_key, Dialog};
use home::handle_home_key;
use sessions::{handle_sessions_key, render_sessions};

use crate::permission::CliPermission;

// ── Log entries (used by background + shell) ─────────────

#[derive(Debug, Clone)]
pub(crate) enum LogEntry {
    Info(#[allow(dead_code)] String),
    Success(#[allow(dead_code)] String),
    Warn(#[allow(dead_code)] String),
    Error(#[allow(dead_code)] String),
}

impl LogEntry {
    pub(crate) fn entry_text(&self) -> &str {
        match self {
            LogEntry::Info(t) | LogEntry::Success(t) | LogEntry::Warn(t) | LogEntry::Error(t) => {
                t.as_str()
            }
        }
    }
}

// ── Toast notifications ────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) enum ToastKind {
    Success,
    #[allow(dead_code)]
    Error,
    Info,
}

#[derive(Debug, Clone)]
pub(crate) struct Toast {
    pub kind: ToastKind,
    pub message: String,
    pub created_at: Instant,
}

impl Toast {
    fn new(kind: ToastKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), created_at: Instant::now() }
    }

    fn style(&self, theme: &theme::Theme) -> (Color, Color) {
        match self.kind {
            ToastKind::Success => (Color::Black, theme.success),
            ToastKind::Error => (Color::Black, theme.error),
            ToastKind::Info => (Color::Black, theme.primary),
        }
    }
}

const TOAST_DURATION: Duration = Duration::from_secs(4);
const MAX_TOASTS: usize = 3;

// ── Panels ───────────────────────────────────────────────

// ── Command Palette ─────────────────────────────────────

#[derive(Clone)]
struct CmdPalEntry {
    label: &'static str,
    desc: &'static str,
    command: &'static str,
    shortcut: &'static str,
}

const COMMAND_PALETTE: &[CmdPalEntry] = &[
    CmdPalEntry { label: "Session List", desc: "Switch or manage sessions", command: ":sessions", shortcut: "Ctrl+L" },
    CmdPalEntry { label: "Model List", desc: "Switch AI model", command: ":model", shortcut: "Ctrl+M" },
    CmdPalEntry { label: "Agent List", desc: "Switch agent", command: ":agent", shortcut: "Ctrl+A" },
    CmdPalEntry { label: "Todos", desc: "Manage todo list", command: ":todos", shortcut: "" },
    CmdPalEntry { label: "LSP Diagnostics", desc: "View LSP diagnostics", command: ":lsp", shortcut: "" },
    CmdPalEntry { label: "SOP List", desc: "List & execute SOPs", command: ":sop", shortcut: "" },
    CmdPalEntry { label: "Roadmap", desc: "View project roadmap", command: ":roadmap", shortcut: "" },
    CmdPalEntry { label: "Bridge Status", desc: "Check bridge health", command: ":bridge status", shortcut: "" },
    CmdPalEntry { label: "Help", desc: "Show keybindings & help", command: ":help", shortcut: "F1" },
    CmdPalEntry { label: "Dev Agent", desc: "Run autonomous development agent", command: ":dev", shortcut: "" },
    CmdPalEntry { label: "Clear Log", desc: "Clear the log", command: "clear", shortcut: "" },
    CmdPalEntry { label: "Exit", desc: "Exit Sisyphus", command: "exit", shortcut: "Ctrl+Q" },
    CmdPalEntry { label: "New Session", desc: "Create a new session", command: ":new", shortcut: "Ctrl+N" },
    CmdPalEntry { label: "Go Home", desc: "Return to Home screen", command: ":home", shortcut: "Ctrl+H" },
    CmdPalEntry { label: "Toggle Theme", desc: "Switch dark/light theme", command: ":theme", shortcut: "" },
];

fn matching_palette_entries(query: &str) -> Vec<&'static CmdPalEntry> {
    let q = query.to_lowercase();
    COMMAND_PALETTE
        .iter()
        .filter(|e| q.is_empty() || e.label.to_lowercase().contains(&q) || e.desc.to_lowercase().contains(&q) || e.command.contains(&q))
        .collect()
}

// ── Views (routes) ───────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum View {
    Home,
    Session,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BoulderState {
    pub active_session_id: Option<String>,
    pub active_view: View,
    pub session_scroll: usize,
    pub session_filter: String,
    pub lsp_language: String,
    pub lsp_server: String,
    pub theme_mode: theme::ThemeMode,
    pub last_updated: String,
}

// ── App state ────────────────────────────────────────────

pub(crate) struct App {
    // sync
    data_dir: PathBuf,
    boulder_path: PathBuf,
    boulder_loaded: bool,
    boulder_dirty: bool,
    boulder_save_tick: u32,
    bridge_address: String,

    // routing
    view: View,

    // home
    home_input: String,
    home_cursor: usize,
    home_placeholder_idx: usize,

    // session
    sessions: Vec<sisyphus_tools::Session>,
    session_filter: String,
    session_sel: usize,
    session_detail: Option<sisyphus_tools::Session>,
    session_detail_scroll: usize,
    session_dirty: bool,
    session_input: String,
    session_cursor: usize,
    session_mode: Mode,
    session_suggestions: Vec<String>,

    // shared log / command history
    log: Vec<LogEntry>,
    scroll: usize,
    notification: Option<(String, Instant)>,
    history: Vec<String>,
    history_idx: Option<usize>,
    exit_requested: bool,
    exit_immediately: bool,
    dialog: Option<Dialog>,
    dialog_sel: usize,
    dialog_query: String,
    help_scroll: usize,

    // todo data (dialog state)
    todo_items: Vec<sisyphus_tools::TodoItem>,
    todo_sel: usize,
    todo_dirty: bool,
    todo_adding: String,

    // lsp data (dialog state)
    lsp_diagnostics: Vec<sisyphus_tools::lsp::LspDiagnostic>,
    lsp_status: String,
    lsp_language: String,
    lsp_server: String,
    lsp_scroll: usize,

    // sop data (dialog state)
    sop_sops: Vec<crate::bridge::SopInfo>,
    sop_sel: usize,
    sop_status: String,
    sop_result: Option<String>,
    sop_query: String,

    // roadmap data (dialog state)
    roadmap_projection: Option<String>,
    roadmap_bottlenecks: Option<String>,
    roadmap_status: String,
    roadmap_scroll: usize,

    // command palette
    palette_open: bool,
    palette_query: String,
    palette_sel: usize,

    // auto-refresh
    last_refresh: Instant,

    pending_op: Option<tokio::sync::oneshot::Receiver<BgOp>>,
    pending_spin: u8,


    // misc
    confirm_delete: Option<String>,
    expanded_thoughts: HashSet<usize>,
    /// Y-level of the history area in the session detail (stored for click-to-message mapping).
    last_detail_area: std::cell::Cell<ratatui::layout::Rect>,
    /// Effective scroll offset at last render (stored for click-to-message mapping).
    last_effective_scroll: std::cell::Cell<usize>,
    /// Max scroll value at last render (stored for scroll boundary clamping).
    last_detail_max_scroll: std::cell::Cell<usize>,
    create_buf: String,
    /// Which message is currently selected/highlighted for clipboard operations.
    selected_msg: Option<usize>,

    // session message search
    search_active: bool,
    search_query: String,
    search_results: Vec<usize>,
    search_idx: usize,

    /// Toast notification overlay
    toasts: Vec<Toast>,

    theme_mode: theme::ThemeMode,
    model_name: String,
    leader_mode: bool,
    ralph_enabled: bool,
    ralph_idle_cycles: u32,
    ralph_continue_pending: bool,

    // dev agent dialog
    dev_goal: String,
    dev_context: String,
    dev_parallel: bool,
    dev_resume: bool,
    dev_report: Option<String>,
    dev_status: String,
    dev_focus: DevFocus,
}

const MAX_LOG_ENTRIES: usize = 1000;
const AUTO_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const RALPH_IDLE_THRESHOLD: u32 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Input,
    Select,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DevFocus {
    Goal,
    Context,
    Parallel,
    Resume,
    Run,
}

impl App {
    fn new(data_dir: PathBuf, bridge_address: String) -> Self {
        let boulder_path = data_dir.join(".omo").join("boulder.json");
        Self {
            data_dir,
            boulder_path,
            boulder_loaded: false,
            boulder_dirty: false,
            boulder_save_tick: 0,
            bridge_address,
            view: View::Home,
            home_input: String::new(),
            home_cursor: 0,
            home_placeholder_idx: 0,
            sessions: Vec::new(),
            session_filter: String::new(),
            session_sel: 0,
            session_detail: None,
            session_detail_scroll: 0,
            session_dirty: true,
            session_input: String::new(),
            session_cursor: 0,
            session_mode: Mode::Select,
            session_suggestions: vec![
                "Type /help for keyboard shortcuts".into(),
                "Type /dev to open dev dialog".into(),
                "Try Ctrl+U/W/A/E for input editing".into(),
            ],
            log: vec![LogEntry::Info("Sisyphus — Ctrl+K: Palette | /help: shortcuts | Type / in input for commands".into())],
            scroll: 0,
            history: Vec::new(),
            history_idx: None,
            exit_requested: false,
            exit_immediately: false,
            dialog: None,
            dialog_sel: 0,
            dialog_query: String::new(),
            help_scroll: 0,
            todo_items: Vec::new(),
            todo_sel: 0,
            todo_dirty: true,
            todo_adding: String::new(),
            lsp_diagnostics: Vec::new(),
            lsp_status: "idle".into(),
            lsp_language: "rust".into(),
            lsp_server: "rust-analyzer".into(),
            lsp_scroll: 0,
            sop_sops: Vec::new(),
            sop_sel: 0,
            sop_status: "disconnected".into(),
            sop_result: None,
            sop_query: String::new(),
            roadmap_projection: None,
            roadmap_bottlenecks: None,
            roadmap_status: "disconnected".into(),
            roadmap_scroll: 0,
            palette_open: false,
            palette_query: String::new(),
            palette_sel: 0,
            last_refresh: Instant::now(),
            pending_op: None,
            pending_spin: 0,
            confirm_delete: None,
            create_buf: String::new(),
            expanded_thoughts: HashSet::new(),
            last_detail_area: std::cell::Cell::new(ratatui::layout::Rect::default()),
            last_effective_scroll: std::cell::Cell::new(0),
            last_detail_max_scroll: std::cell::Cell::new(0),
            notification: None,
            selected_msg: None,
            search_active: false,
            search_query: String::new(),
            search_results: Vec::new(),
            search_idx: 0,
            toasts: Vec::new(),
            theme_mode: theme::ThemeMode::Dark,
            model_name: "unknown".into(),
            leader_mode: false,
            ralph_enabled: false,
            ralph_idle_cycles: 0,
            ralph_continue_pending: false,

            dev_goal: String::new(),
            dev_context: String::new(),
            dev_parallel: false,
            dev_resume: false,
            dev_report: None,
            dev_status: "idle".into(),
            dev_focus: DevFocus::Goal,
        }
    }

    fn save_boulder(&mut self) {
        if !self.boulder_dirty {
            return;
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_else(|_| "0".to_string());

        let state = BoulderState {
            active_session_id: self.session_detail.as_ref().map(|s| s.id.clone()),
            active_view: self.view,
            session_scroll: self.session_detail_scroll,
            session_filter: self.session_filter.clone(),
            lsp_language: self.lsp_language.clone(),
            lsp_server: self.lsp_server.clone(),
            theme_mode: self.theme_mode,
            last_updated: now,
        };

        if let Some(parent) = self.boulder_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                self.add_log(LogEntry::Error(format!("Failed to create boulder dir: {e}")));
            }
        }

        if let Ok(json) = serde_json::to_string_pretty(&state) {
            if std::fs::write(&self.boulder_path, json).is_ok() {
                self.boulder_dirty = false;
            } else {
                self.add_log(LogEntry::Error("Failed to save boulder state".into()));
            }
        }
    }

    fn load_boulder(&mut self) {
        let Ok(content) = std::fs::read_to_string(&self.boulder_path) else {
            return; // First launch — no boulder yet
        };

        let Ok(state) = serde_json::from_str::<BoulderState>(&content) else {
            self.add_log(LogEntry::Warn("Boulder state corrupted, ignoring".into()));
            return;
        };

        self.view = state.active_view;
        self.session_filter = state.session_filter;
        self.session_detail_scroll = state.session_scroll;
        self.lsp_language = state.lsp_language;
        self.lsp_server = state.lsp_server;
        self.theme_mode = state.theme_mode;

        if let Some(id) = state.active_session_id {
            let db = self.data_dir.join("sessions.json");
            if let Ok(session) = sisyphus_tools::session_get(&db, &id) {
                self.session_detail = Some(session);
            }
        }

        self.boulder_loaded = true;
    }


    fn theme(&self) -> theme::Theme {
        match self.theme_mode {
            theme::ThemeMode::Dark => theme::Theme::dark(),
            theme::ThemeMode::Light => theme::Theme::light(),
        }
    }

    fn toggle_theme(&mut self) {
        self.theme_mode = match self.theme_mode {
            theme::ThemeMode::Dark => theme::ThemeMode::Light,
            theme::ThemeMode::Light => theme::ThemeMode::Dark,
        };
        self.boulder_dirty = true;
    }

    // ── thought block helpers ────────────────────────────

    /// Check if msg_idx's thought blocks are currently expanded.
    fn is_thought_expanded(&self, msg_idx: usize) -> bool {
        self.expanded_thoughts.contains(&msg_idx)
    }

    /// Compute the starting line offset of `msg_idx` in the rendered session output
    /// using the CURRENT expanded state. Used for scroll adjustment calculations.
    fn msg_start_line(&self, msg_idx: usize) -> usize {
        let session = match &self.session_detail {
            Some(s) => s,
            None => return 0,
        };
        let mut line = 0usize;
        for i in 0..msg_idx.min(session.messages.len()) {
            line += 1; // role label
            line += helpers::count_message_lines(
                &session.messages[i].content,
                self.is_thought_expanded(i),
            );
            line += 1; // blank separator
        }
        line
    }

    /// Compute the line count difference when toggling msg_idx's thought blocks.
    fn thought_toggle_line_diff(&self, msg_idx: usize) -> isize {
        let session = match &self.session_detail {
            Some(s) => s,
            None => return 0,
        };
        let msg = match session.messages.get(msg_idx) {
            Some(m) => m,
            None => return 0,
        };
        let was_expanded = self.expanded_thoughts.contains(&msg_idx);
        let old_lines = helpers::count_message_lines(&msg.content, was_expanded);
        let new_lines = helpers::count_message_lines(&msg.content, !was_expanded);
        new_lines as isize - old_lines as isize
    }

    /// Adjust session detail scroll by `delta` lines (positive = down, negative = up).
    /// Handles auto-scroll (`usize::MAX`) by converting to the stored max_scroll first,
    /// so the first Up press actually scrolls up from the bottom.
    /// Scrolling down at the bottom re-activates auto-scroll.
    fn adjust_session_scroll(&mut self, delta: isize) {
        let max_scroll = self.last_detail_max_scroll.get();
        let current = if self.session_detail_scroll == usize::MAX {
            max_scroll
        } else {
            self.session_detail_scroll
        };
        let candidate = (current as isize + delta).max(0) as usize;
        let clamped = candidate.min(max_scroll);
        // Re-activate auto-scroll when scrolling down reaches (or stays at) the bottom.
        if clamped >= max_scroll && delta > 0 {
            self.session_detail_scroll = usize::MAX;
        } else {
            self.session_detail_scroll = clamped;
        }
    }

    /// Toggle thought blocks for a single message. Adjusts scroll to keep view stable.
    fn toggle_message_thoughts(&mut self, msg_idx: usize) {
        // Compute scroll adjustment BEFORE toggling state
        let line_diff = self.thought_toggle_line_diff(msg_idx);
        let msg_start = self.msg_start_line(msg_idx);

        // Toggle state
        if !self.expanded_thoughts.remove(&msg_idx) {
            self.expanded_thoughts.insert(msg_idx);
        }

        // Adjust scroll: if the toggled message is above or within the viewport,
        // shift scroll proportionally so the user stays at the same visual position.
        // auto-scroll (usize::MAX) is preserved.
        if self.session_detail_scroll != usize::MAX && line_diff != 0 && msg_start < self.session_detail_scroll + 100 {
            self.session_detail_scroll = if line_diff > 0 {
                self.session_detail_scroll.saturating_add(line_diff as usize)
            } else {
                self.session_detail_scroll.saturating_sub((-line_diff) as usize)
            };
        }
    }

    /// Toggle all messages' thought blocks on/off.
    fn toggle_all_thoughts(&mut self) {
        let n = self.session_detail.as_ref().map_or(0, |s| s.messages.len());
        let all_expanded = (0..n).all(|i| self.expanded_thoughts.contains(&i));
        if all_expanded {
            self.expanded_thoughts.clear();
        } else {
            for i in 0..n {
                self.expanded_thoughts.insert(i);
            }
        }
        self.session_detail_scroll = 0;
    }

    /// Map a terminal row to the message index at that position.
    fn find_message_at_row(&self, row: u16) -> Option<usize> {
        let session = self.session_detail.as_ref()?;
        let area = self.last_detail_area.get();
        let scroll = self.last_effective_scroll.get();

        // crossterm mouse row is 0-based, ratatui area.y is 0-based.
        // Block border draws at area.y; content starts at area.y + 1.
        // So the first clickable content row (y_in_content = 0) is at
        // crossterm row = area.y + 1 → subtract 1.
        let y_in_history = row.saturating_sub(area.y);
        let y_in_content = y_in_history.saturating_sub(1);
        let target_line = y_in_content as usize + scroll;

        let mut offset = 0usize;
        for (idx, msg) in session.messages.iter().enumerate() {
            let start = offset;
            offset += 1;
            offset += helpers::count_message_lines(
                &msg.content,
                self.is_thought_expanded(idx),
            );
            offset += 1;
            if target_line >= start && target_line < offset {
                return Some(idx);
            }
        }
        None
    }



    fn add_log(&mut self, entry: LogEntry) {
        self.log.push(entry);
        if self.log.len() > MAX_LOG_ENTRIES {
            let excess = self.log.len() - MAX_LOG_ENTRIES;
            self.log.drain(..excess);
            self.scroll = self.scroll.saturating_sub(excess);
        }
        self.scroll = self.log.len().saturating_sub(1);
    }

    fn add_toast(&mut self, kind: ToastKind, message: impl Into<String>) {
        self.toasts.push(Toast::new(kind, message));
        if self.toasts.len() > MAX_TOASTS {
            self.toasts.remove(0);
        }
    }

    fn prune_expired_toasts(&mut self) {
        self.toasts.retain(|t| t.created_at.elapsed() < TOAST_DURATION);
    }

    fn perform_search(&mut self) {
        let q = self.search_query.to_lowercase();
        self.search_results.clear();
        self.search_idx = 0;
        if q.is_empty() || self.search_query.is_empty() {
            return;
        }
        if let Some(ref session) = self.session_detail {
            for (i, msg) in session.messages.iter().enumerate() {
                if msg.content.to_lowercase().contains(&q) {
                    self.search_results.push(i);
                }
            }
        }
        self.scroll_to_search_match();
    }

    fn scroll_to_search_match(&mut self) {
        if self.search_results.is_empty() {
            return;
        }
        let idx = self.search_results[self.search_idx.min(self.search_results.len().saturating_sub(1))];
        self.selected_msg = Some(idx);
        // Rough line estimate: each message ~ header(1) + content_lines + spacing(1)
        if let Some(ref detail) = self.session_detail {
            let mut line = 0usize;
            for (i, msg) in detail.messages.iter().enumerate() {
                if i >= idx { break; }
                line += 1; // header
                line += msg.content.lines().count().max(1); // content
                line += 1; // spacing
            }
            // Scroll 2 lines above the match so the header is visible
            self.session_detail_scroll = line.saturating_sub(2);
        } else {
            self.session_detail_scroll = 0;
        }
    }

    fn next_search_match(&mut self) {
        if self.search_results.is_empty() { return; }
        self.search_idx = (self.search_idx + 1) % self.search_results.len();
        self.scroll_to_search_match();
    }

    fn prev_search_match(&mut self) {
        if self.search_results.is_empty() { return; }
        self.search_idx = if self.search_idx == 0 {
            self.search_results.len().saturating_sub(1)
        } else {
            self.search_idx - 1
        };
        self.scroll_to_search_match();
    }

    // ── todos ────────────────────────────────────
    fn refresh_todos(&mut self) {
        if !self.todo_dirty {
            return;
        }
        let db = self.data_dir.join("todos.json");
        self.todo_items = sisyphus_tools::todo_list(&db, None).unwrap_or_default();
        self.todo_sel = self.todo_sel.min(self.todo_items.len().saturating_sub(1));
        self.todo_dirty = false;
    }

    #[allow(dead_code)]
    fn toggle_todo_status(&mut self) {
        if self.todo_items.is_empty() {
            return;
        }
        let db = self.data_dir.join("todos.json");
        let item = &self.todo_items[self.todo_sel];
        let new_status = match item.status {
            sisyphus_tools::TodoStatus::Pending => sisyphus_tools::TodoStatus::InProgress,
            sisyphus_tools::TodoStatus::InProgress => sisyphus_tools::TodoStatus::Completed,
            sisyphus_tools::TodoStatus::Completed => sisyphus_tools::TodoStatus::Pending,
            sisyphus_tools::TodoStatus::Cancelled => sisyphus_tools::TodoStatus::Pending,
        };
        if sisyphus_tools::todo_update(&db, &item.id, None, Some(new_status), None).is_ok() {
            self.todo_dirty = true;
        }
    }

    #[allow(dead_code)]
    fn delete_todo(&mut self) {
        if self.todo_items.is_empty() {
            return;
        }
        let id = self.todo_items[self.todo_sel].id.clone();
        if self.confirm_delete.as_deref() == Some(&id) {
            let db = self.data_dir.join("todos.json");
            if sisyphus_tools::todo_delete(&db, &id).is_ok() {
                self.todo_dirty = true;
                self.add_log(LogEntry::Info(format!("Deleted todo {id}")));
            }
            self.confirm_delete = None;
        } else {
            self.confirm_delete = Some(id);
            self.add_log(LogEntry::Warn("Press Delete again to confirm".into()));
        }
    }

    // ── sessions ─────────────────────────────────
    fn refresh_sessions(&mut self) {
        if !self.session_dirty {
            return;
        }
        let db = self.data_dir.join("sessions.json");
        match sisyphus_tools::session_list(&db) {
            Ok(sessions) => self.sessions = sessions,
            Err(e) => self.add_log(LogEntry::Error(format!("Failed to list sessions: {e}"))),
        }
        self.session_sel = self.session_sel.min(self.sessions.len().saturating_sub(1));
        self.session_dirty = false;
    }

    fn filtered_sessions(&self) -> Vec<usize> {
        if self.session_filter.is_empty() {
            return (0..self.sessions.len()).collect();
        }
        let q = self.session_filter.to_lowercase();
        self.sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| s.name.to_lowercase().contains(&q) || s.id.contains(&q))
            .map(|(i, _)| i)
            .collect()
    }

    fn select_session(&mut self) {
        if self.sessions.is_empty() {
            return;
        }
        let filtered = self.filtered_sessions();
        if filtered.is_empty() {
            return;
        }
        let idx = filtered[self.session_sel.min(filtered.len().saturating_sub(1))];
        let db = self.data_dir.join("sessions.json");
        let id = &self.sessions[idx].id;
        match sisyphus_tools::session_get(&db, id) {
            Ok(session) => self.session_detail = Some(session),
            Err(e) => self.add_log(LogEntry::Error(format!("Failed to load session {id}: {e}"))),
        }
        self.session_detail_scroll = usize::MAX; // auto-scroll to latest messages
        self.boulder_dirty = true;
    }

    fn delete_session(&mut self) {
        let filtered = self.filtered_sessions();
        if filtered.is_empty() {
            return;
        }
        let idx = filtered[self.session_sel.min(filtered.len().saturating_sub(1))];
        let db = self.data_dir.join("sessions.json");
        let id = self.sessions[idx].id.clone();
        if self.confirm_delete.as_deref() == Some(&id) {
            match sisyphus_tools::session_delete(&db, &id) {
                Ok(_) => {
                    self.add_log(LogEntry::Info(format!("Deleted session {id}")));
                    self.session_dirty = true;
                    self.session_sel = self.session_sel.saturating_sub(1);
                }
                Err(e) => self.add_log(LogEntry::Error(format!("Failed to delete session {id}: {e}"))),
            }
            self.confirm_delete = None;
        } else {
            self.confirm_delete = Some(id);
            self.add_log(LogEntry::Warn("Press Delete again to confirm".into()));
        }
    }

    // ── sop panel ─────────────────────────────────
    #[allow(dead_code)]
    fn filtered_sops(&self) -> Vec<&crate::bridge::SopInfo> {
        if self.sop_query.is_empty() {
            return self.sop_sops.iter().collect();
        }
        let q = self.sop_query.to_lowercase();
        self.sop_sops.iter().filter(|s| s.sop_id.to_lowercase().contains(&q)).collect()
    }
}

// ── Public entry ────────────────────────────────────────

pub fn run(data_dir: PathBuf, bridge_address: String) -> Result<()> {
    // Ensure data directory exists (same as CLI mode in main.rs:1134)
    std::fs::create_dir_all(&data_dir)?;

    let mut terminal = ratatui::init();
    terminal.clear()?;

    crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)?;

    let perms = CliPermission::load(None).unwrap_or_default();
    let mut app = App::new(data_dir, bridge_address);

    // Load session list (don't auto-create or select - start at Home)
    let db = app.data_dir.join("sessions.json");
    app.sessions = sisyphus_tools::session_list(&db).unwrap_or_default();
    app.session_dirty = false;

    app.load_boulder();

    let tick_rate = Duration::from_millis(100);
    let res = run_loop(&mut terminal, &mut app, &perms, tick_rate);

    crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture)?;
    ratatui::restore();
    res
}

fn run_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    perms: &CliPermission,
    tick_rate: Duration,
) -> Result<()> {
    loop {
        // Poll for background bridge operation completion
        poll_bg_op(app);

        app.boulder_save_tick += 1;
        if app.boulder_save_tick >= 100 {
            app.boulder_save_tick = 0;
            app.save_boulder();
        }

        app.prune_expired_toasts();

        terminal.draw(|f| ui(f, app))?;

        // Advance spinner frame when bg op is pending
        if app.pending_op.is_some() {
            app.pending_spin = app.pending_spin.wrapping_add(1);
        }

        // Auto-refresh sessions and todos periodically
        if app.last_refresh.elapsed() >= AUTO_REFRESH_INTERVAL {
            app.todo_dirty = true;
            app.session_dirty = true;
            app.last_refresh = Instant::now();
        }

        let mut event_received = false;
        let poll_ok = match event::poll(tick_rate) {
            Ok(true) => true,
            Ok(false) => false,
            Err(e) => {
                app.add_log(LogEntry::Warn(format!("event::poll failed (IME?): {e}")));
                false
            }
        };
        if poll_ok {
            let event = match event::read() {
                Ok(e) => e,
                Err(e) => {
                    app.add_log(LogEntry::Warn(format!("event::read failed (IME?): {e}")));
                    continue;
                }
            };
            event_received = true;
            match handle_event(event, app, perms) {
                Ok(true) => break,
                Ok(false) => {}
                Err(e) => {
                    app.add_log(LogEntry::Error(format!("handle_event error: {e}")));
                }
            }
        }

        if app.ralph_enabled {
            if event_received {
                app.ralph_idle_cycles = 0;
            } else {
                app.ralph_idle_cycles += 1;
                if app.ralph_idle_cycles >= RALPH_IDLE_THRESHOLD {
                    app.ralph_idle_cycles = 0;
                    app.ralph_continue_pending = true;
                }
            }
        }

        if app.ralph_continue_pending {
            app.ralph_continue_pending = false;
            app.session_input = "continue".into();
            let fake_event = Event::Key(crossterm::event::KeyEvent::new(
                KeyCode::Enter,
                crossterm::event::KeyModifiers::empty(),
            ));
            // Only auto-continue if we're in a session
            if app.view == View::Session
                && handle_event(fake_event, app, perms)? {
                    break;
                }
        }
    }
    Ok(())
}

fn poll_bg_op(app: &mut App) {
    let Some(rx) = &mut app.pending_op else { return };
    match rx.try_recv() {
        Ok(op) => {
            apply_bg_op(app, op);
            app.pending_op = None;
        }
        Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
        Err(_) => {
            app.pending_op = None;
        }
    }
}

fn apply_bg_op(app: &mut App, op: BgOp) {
    match op {
        BgOp::SopsListed { count, sops } => {
            app.sop_sops = sops;
            app.sop_sel = 0;
            app.sop_status = format!("connected ({} SOPs)", count);
        }
        BgOp::SopsFailed(e) => {
            app.sop_status = format!("failed: {e}");
        }
        BgOp::SopExecuted { sop_id, result } => {
            app.sop_result = Some(result);
            app.sop_status = format!("executed {sop_id}");
        }
        BgOp::SopExecuteFailed { sop_id, error } => {
            app.sop_result = None;
            app.sop_status = format!("execute {sop_id} failed: {error}");
        }
        BgOp::RoadmapFetched { projection, bottlenecks } => {
            app.roadmap_projection = Some(projection);
            app.roadmap_bottlenecks = Some(bottlenecks);
            app.roadmap_status = "connected".into();
        }
        BgOp::RoadmapFailed(e) => {
            app.roadmap_projection = None;
            app.roadmap_bottlenecks = None;
            app.roadmap_status = format!("failed: {e}");
        }
        BgOp::BridgeCmd { entries } => {
            for entry in entries {
                app.add_log(entry);
            }
        }
        BgOp::DevResult { goal, report, success } => {
            if success {
                app.add_log(LogEntry::Success(format!("Dev agent completed: {goal}")));
            } else {
                app.add_log(LogEntry::Warn(format!("Dev agent finished with issues: {goal}")));
            }
            app.dev_report = Some(report);
            app.dev_status = "done".into();
        }
        BgOp::SessionUpdated { session_id, errors } => {
            for entry in &errors {
                app.add_log(entry.clone());
            }
            if let Some(first_err) = errors.first() {
                app.notification = Some((first_err.entry_text().to_string(), Instant::now()));
            }
            app.refresh_sessions();
            let viewing_session = app.session_detail.as_ref().map(|d| d.id.clone());
            if viewing_session.as_deref() == Some(&session_id) {
                let db = app.data_dir.join("sessions.json");
                if let Ok(session) = sisyphus_tools::session_get(&db, &session_id) {
                    app.session_detail = Some(session);
                }
                app.session_detail_scroll = usize::MAX;
                // Generate contextual suggestions after agent response
                let last_role = app.session_detail.as_ref()
                    .and_then(|d| d.messages.last())
                    .map(|m| m.role.as_str());
                    if last_role == Some("assistant") {
                        app.session_suggestions = vec![
                            "Type a follow-up message or press Esc to browse sessions".into(),
                            "Use /agent off to disable AI responses".into(),
                            "Use /suggest <topic> for targeted suggestions".into(),
                        ];
                    } else {
                        app.session_suggestions = vec![
                            "Press Enter to send your message".into(),
                            "Type /help for available commands".into(),
                            "Use @filename to reference a file".into(),
                        ];
                    }
                }
            }
        }
    }

// ── Event handler ───────────────────────────────────────

fn handle_event(event: Event, app: &mut App, _perms: &CliPermission) -> Result<bool> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            app.notification = None; // Clear notification on any key press

            // Global: Ctrl+C / Ctrl+Q → quick exit (double-tap to confirm)
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

            // Global: Ctrl+K → toggle command palette (opencode-style)
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

            // Global: Ctrl+L → session list dialog
            if key.code == KeyCode::Char('l')
                && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
            {
                app.dialog = Some(Dialog::SessionList);
                app.dialog_sel = 0;
                app.dialog_query.clear();
                app.session_dirty = true;
                return Ok(false);
            }

            // Global: Ctrl+M → model info (placeholder for model list)
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

            // Global: Ctrl+A → agent info (placeholder for agent list)
            if key.code == KeyCode::Char('a')
                && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
            {
                app.add_log(LogEntry::Info("Agent routing managed by bridge. See Config for profiles.".into()));
                return Ok(false);
            }

            // Global: F1 → help dialog
            if key.code == KeyCode::F(1) {
                app.dialog = Some(Dialog::Help);
                return Ok(false);
            }

            // Global: Ctrl+H → go home
            if key.code == KeyCode::Char('h')
                && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
            {
                app.view = View::Home;
                app.dialog = None;
                app.boulder_dirty = true;
                return Ok(false);
            }

            // Global: Ctrl+N → new session
            if key.code == KeyCode::Char('n')
                && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
            {
                let db = app.data_dir.join("sessions.json");
                match sisyphus_tools::session_create(&db, "unnamed") {
                    Ok(s) => {
                        app.add_log(LogEntry::Success(format!("Created session {}", s.name)));
                        match sisyphus_tools::session_get(&db, &s.id) {
                            Ok(session) => app.session_detail = Some(session),
                            Err(e) => app.add_log(LogEntry::Error(format!("Session created but load failed: {e}"))),
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
                && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
            {
                let pasted = paste_from_clipboard();
                if !pasted.is_empty() {
                    match app.view {
                        View::Home => {
                            let byte_pos = app.home_input.char_indices()
                                .nth(app.home_cursor)
                                .map(|(i, _)| i)
                                .unwrap_or(app.home_input.len());
                            app.home_input.insert_str(byte_pos, &pasted);
                            app.home_cursor += pasted.chars().count();
                        }
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
            }

            if app.exit_immediately {
                return Ok(true);
            }
        }
        Event::Mouse(m) => {
            match m.kind {
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
            }
        }
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
                    match sisyphus_tools::session_create(&db, "unnamed") {
                        Ok(s) => {
                            app.add_log(LogEntry::Success(format!("Created session {}", s.name)));
                            match sisyphus_tools::session_get(&db, &s.id) {
                                Ok(session) => app.session_detail = Some(session),
                                Err(e) => app.add_log(LogEntry::Error(format!("Session created but load failed: {e}"))),
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
                        app.pending_op = Some(spawn_list_sops(app.bridge_address.clone()));
                    }
                }
                "roadmap" | ":roadmap" => {
                    app.dialog = Some(Dialog::Roadmap);
                    app.roadmap_status = "connecting...".into();
                    if app.pending_op.is_none() {
                        app.pending_op = Some(spawn_fetch_roadmap(app.bridge_address.clone()));
                    }
                }
                "clear" | ":clear" => {
                    app.log.clear();
                    app.scroll = 0;
                }
                "exit" | ":exit" | "quit" | ":quit" => {
                    app.exit_requested = true;
                    app.add_log(LogEntry::Warn("Type `exit` again to confirm, or `clear` to dismiss.".into()));
                }
                cmd if cmd.starts_with(":bridge") => {
                    let db = app.data_dir.join("sessions.json");
                    app.session_input = cmd.to_string();
                    app.view = View::Session;
                    if let Err(e) = sisyphus_tools::session_create(&db, "unnamed") {
                        app.add_log(LogEntry::Error(format!("Failed to create bridge session: {e}")));
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
                    app.add_log(LogEntry::Info("Use Config or bridge to switch models.".into()));
                }
                "agent" | ":agent" => {
                    app.add_log(LogEntry::Info("Agent routing is managed by the bridge.".into()));
                    app.add_log(LogEntry::Info("Use Config to configure agent profiles.".into()));
                }
                "theme" | ":theme" => {
                    app.toggle_theme();
                    app.add_log(LogEntry::Success(format!(
                        "Theme switched to {}",
                        match app.theme_mode {
                            theme::ThemeMode::Dark => "Dark",
                            theme::ThemeMode::Light => "Light",
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
                    app.dialog = Some(Dialog::Dev { goal: String::new() });
                }
                _ => {}
            }
        }
        _ => {}
    }
}


// ── UI: Routing by View ─────────────────────────────────

fn ui(frame: &mut ratatui::Frame, app: &App) {
    if app.palette_open {
        render_palette(frame, app);
        return;
    }

    let [content_area, status_area] = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Min(0),
        ratatui::layout::Constraint::Length(1),
    ])
    .areas(frame.area());

    // Route content by current view
    match app.view {
        View::Home => crate::tui::home::render_home(frame, content_area, app),
        View::Session => render_sessions(frame, content_area, app),
    }

    // If a dialog is active, render it as overlay
    if app.dialog.is_some() {
        let overlay = ratatui::widgets::Clear;
        frame.render_widget(overlay, frame.area());
        crate::tui::dialog::render_dialog_overlay(frame, frame.area(), app);
    }

    frame.render_widget(ratatui::widgets::Paragraph::new(render_status(app)), status_area);

    // Render toast notifications as floating overlay (bottom-right)
    render_toasts(frame, app);
}

fn render_palette(frame: &mut ratatui::Frame, app: &App) {
    let theme = app.theme();
    let area = frame.area();
    frame.render_widget(ratatui::widgets::Clear, area);

    let [_, vert_area, _] = Layout::vertical([
        Constraint::Percentage(25),
        Constraint::Length(20),
        Constraint::Percentage(25),
    ])
    .areas(area);
    let [_, pal_area, _] = Layout::horizontal([
        Constraint::Percentage(20),
        Constraint::Percentage(60),
        Constraint::Percentage(20),
    ])
    .areas(vert_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Command Palette ")
        .title_bottom(format!(" {} matches | Type to filter... ", matching_palette_entries(&app.palette_query).len()))
        .border_style(Style::default().fg(theme.primary));

    let items: Vec<ListItem> = matching_palette_entries(&app.palette_query)
        .iter()
        .map(|e| {
            let shortcut_style = if e.shortcut.is_empty() {
                Style::default().fg(theme.muted)
            } else {
                Style::default().fg(theme.secondary)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {:<16}", e.label), Style::default().fg(theme.fg).add_modifier(Modifier::BOLD)),
                Span::styled(e.desc, Style::default().fg(theme.muted)),
                Span::raw("  "),
                Span::styled(e.shortcut, shortcut_style),
            ]))
        })
        .collect();

    let sel = app.palette_sel.min(matching_palette_entries(&app.palette_query).len().saturating_sub(1));
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().fg(theme.secondary).add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(
        list,
        pal_area,
        &mut ratatui::widgets::ListState::default().with_selected(Some(sel)),
    );
}

/// Count diagnostics by severity (LSP: 1=error, 2=warning, 3=info, 4=hint).
fn diag_counts(app: &App) -> (usize, usize, usize) {
    let (mut errs, mut warns, mut infos) = (0, 0, 0);
    for d in &app.lsp_diagnostics {
        match d.severity {
            Some(1) => errs += 1,
            Some(2) => warns += 1,
            Some(3) | Some(4) => infos += 1,
            _ => infos += 1,
        }
    }
    (errs, warns, infos)
}

/// Estimate context usage percentage from the active session's messages.
fn context_pct(app: &App) -> u8 {
    const MAX_TOKENS: u64 = 128_000;
    let total: u64 = app
        .session_detail
        .as_ref()
        .map(|s| s.messages.iter().map(|m| m.estimated_tokens).sum())
        .unwrap_or(0);
    if total == 0 {
        return 0;
    }
    let pct = (total as f64 / MAX_TOKENS as f64) * 100.0;
    pct.min(100.0) as u8
}

fn render_status(app: &App) -> Line<'static> {
    let theme = app.theme();

    if app.leader_mode {
        return Line::from(Span::styled(
            " [leader] Press h: help | p: palette | Esc: cancel ",
            Style::default().fg(Color::Black).bg(theme.warning),
        ));
    }

    let view_label = match app.view {
        View::Home => " HOME ",
        View::Session => " SESSION ",
    };

    let help: String = match app.view {
        View::Home => "Enter: start session | Ctrl+K: palette | Ctrl+L: sessions | type prompt".into(),
        View::Session => {
            if app.session_detail.is_some() {
                let mode_label = if app.session_mode == Mode::Input { "INPUT" } else { "NORMAL" };
                let mode_extra = if app.session_mode == Mode::Input {
                    " | Ctrl+U clear | Ctrl+W del word | Ctrl+A/E home/end | /command"
                } else {
                    ""
                };
                format!("[{mode_label}] Enter send | Esc back{mode_extra}")
            } else {
                "↑↓ select | Enter open | Ctrl+N new | Ctrl+H home | type filter".to_string()
            }
        }
    };

    let spinner_chars = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏";
    let spinner = if app.pending_op.is_some() {
        let idx = (app.pending_spin as usize) % spinner_chars.chars().count();
        let c = spinner_chars.chars().nth(idx).unwrap_or(' ');
        format!(" {c} ")
    } else {
        String::new()
    };

    let ctx = context_pct(app);
    let ctx_style = if ctx > 80 {
        Style::default().fg(Color::Black).bg(theme.warning)
    } else {
        Style::default().fg(theme.muted)
    };

    let (errs, warns, infos) = diag_counts(app);

    let mut spans: Vec<Span<'static>> = vec![
        Span::styled(
            view_label,
            Style::default().fg(Color::Black).bg(theme.primary),
        ),
        Span::raw(" "),
        Span::styled(help, Style::default().fg(theme.muted)),
    ];

    if !spinner.is_empty() {
        spans.push(Span::styled(spinner, Style::default().fg(theme.emphasized)));
    }

    spans.push(Span::raw("  "));
    spans.push(Span::styled(format!("Context: {ctx}%"), ctx_style));

    // Diagnostics
    if errs > 0 || warns > 0 || infos > 0 {
        spans.push(Span::raw("  "));
        if errs > 0 {
            spans.push(Span::styled(format!("E{} ", errs), Style::default().fg(theme.error)));
        }
        if warns > 0 {
            spans.push(Span::styled(format!("W{} ", warns), Style::default().fg(theme.warning)));
        }
        if infos > 0 {
            spans.push(Span::styled(format!("I{} ", infos), Style::default().fg(theme.info)));
        }
    }

    spans.push(Span::raw("  "));
    if app.ralph_enabled {
        spans.push(Span::styled(
            " RALPH ",
            Style::default().fg(Color::Black).bg(theme.warning),
        ));
        spans.push(Span::raw(" "));
    }
    if app.dev_status == "running" {
        spans.push(Span::styled(
            " ⚡Dev ",
            Style::default().fg(Color::Black).bg(theme.success),
        ));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(
        app.model_name.clone(),
        Style::default().fg(theme.muted),
    ));

    if let Some((msg, ts)) = &app.notification {
        if ts.elapsed() < std::time::Duration::from_secs(4) {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                format!("⚠ {msg}"),
                Style::default().fg(theme.warning),
            ));
        }
    }

    Line::from(spans)
}

fn render_toasts(frame: &mut ratatui::Frame, app: &App) {
    if app.toasts.is_empty() {
        return;
    }
    let theme = app.theme();
    let area = frame.area();

    let stacked_height = (app.toasts.len() * 3 + 2) as u16;
    let [_, toast_vertical] = Layout::vertical([Constraint::Min(0), Constraint::Length(stacked_height)])
        .areas(area);
    let [_, toast_area] = Layout::horizontal([
        Constraint::Percentage(40),
        Constraint::Percentage(60),
    ])
    .areas(toast_vertical);

    let inner = Layout::vertical(
        std::iter::repeat_n(Constraint::Length(3), app.toasts.len())
            .collect::<Vec<_>>(),
    )
    .split(toast_area);

    for (i, toast) in app.toasts.iter().enumerate() {
        if i >= inner.len() {
            break;
        }
        let (fg, bg) = toast.style(&theme);
        let label = Paragraph::new(Line::from(Span::styled(
            &toast.message,
            Style::default().fg(fg).bg(bg),
        )))
        .block(
            Block::default()
                .borders(Borders::NONE)
                .style(Style::default().bg(bg)),
        );
        frame.render_widget(ratatui::widgets::Clear, inner[i]);
        frame.render_widget(label, inner[i]);
    }
}

// ── Tests ─────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── LogEntry ──────────────────────────────────────────

    #[test]
    fn test_log_entry_entry_text() {
        let info = LogEntry::Info("hello".into());
        let success = LogEntry::Success("done".into());
        let warn = LogEntry::Warn("caution".into());
        let error = LogEntry::Error("boom".into());
        assert_eq!(info.entry_text(), "hello");
        assert_eq!(success.entry_text(), "done");
        assert_eq!(warn.entry_text(), "caution");
        assert_eq!(error.entry_text(), "boom");
    }

    // ── App::new ──────────────────────────────────────────

    #[test]
    fn test_app_new_initial_state() {
        let app = App::new(PathBuf::from("/tmp/test"), "localhost:50051".into());
        assert_eq!(app.view, View::Home);
        assert_eq!(app.session_mode, Mode::Select);
        assert_eq!(app.log.len(), 1); // welcome message
        assert_eq!(app.scroll, 0);
        assert!(app.history.is_empty());
        assert!(app.history_idx.is_none());
        assert!(!app.exit_requested);
        assert!(app.todo_items.is_empty());
        assert_eq!(app.todo_sel, 0);
        assert!(app.todo_dirty);
        assert!(app.sessions.is_empty());
        assert_eq!(app.session_sel, 0);
        assert!(app.session_dirty);
        assert!(app.session_detail.is_none());
        assert_eq!(app.lsp_language, "rust");
        assert_eq!(app.lsp_server, "rust-analyzer");
        assert!(app.lsp_diagnostics.is_empty());
        assert_eq!(app.lsp_status, "idle");
        assert_eq!(app.sop_status, "disconnected");
        assert!(app.sop_result.is_none());
        assert!(app.roadmap_projection.is_none());
        assert!(app.roadmap_bottlenecks.is_none());
        assert_eq!(app.roadmap_status, "disconnected");
        assert!(!app.ralph_enabled);
        assert_eq!(app.ralph_idle_cycles, 0);
        assert!(!app.ralph_continue_pending);
    }

    #[test]
    fn test_app_new_data_dir() {
        let app = App::new(PathBuf::from("/my/data"), "127.0.0.1:9999".into());
        assert_eq!(app.data_dir, PathBuf::from("/my/data"));
        assert_eq!(app.bridge_address, "127.0.0.1:9999");
    }

    // ── App::add_log ──────────────────────────────────────

    #[test]
    fn test_add_log_appends_and_adjusts_scroll() {
        let mut app = App::new(PathBuf::from("/tmp"), "addr".into());
        let initial_scroll = app.scroll;

        app.add_log(LogEntry::Info("test".into()));
        assert_eq!(app.log.len(), 2); // welcome + new
        assert_eq!(app.scroll, initial_scroll + 1);

        app.add_log(LogEntry::Success("done".into()));
        assert_eq!(app.log.len(), 3);
        assert_eq!(app.scroll, 2);
        assert_eq!(app.log[2].entry_text(), "done");
    }

    // ── render_status ─────────────────────────────────────

    #[test]
    fn test_render_status_shows_opencode_format() {
        let app = App::new(PathBuf::from("/tmp"), "addr".into());
        let rendered = render_status(&app).to_string();
        assert!(rendered.contains("SESSION") || rendered.contains("HOME"));
        assert!(rendered.contains("Context: 0%"));
        assert!(rendered.contains("unknown"));
    }

    #[test]
    fn test_render_status_home_help() {
        let mut app = App::new(PathBuf::from("/tmp"), "addr".into());
        app.view = View::Home;
        let rendered = render_status(&app).to_string();
        assert!(rendered.contains("Enter: start session"));
    }

    #[test]
    fn test_render_status_session_list_help() {
        let mut app = App::new(PathBuf::from("/tmp"), "addr".into());
        app.view = View::Session;
        let rendered = render_status(&app).to_string();
        assert!(rendered.contains("Enter open"));
    }

    #[test]
    fn test_render_status_diag_counts() {
        use sisyphus_tools::lsp::{LspDiagnostic, LspPosition, LspRange};
        let rng = LspRange {
            start: LspPosition { line: 0, character: 0 },
            end: LspPosition { line: 1, character: 1 },
        };
        let mut app = App::new(PathBuf::from("/tmp"), "addr".into());
        app.lsp_diagnostics = vec![
            LspDiagnostic { range: rng.clone(), severity: Some(1), message: "err".into() },
            LspDiagnostic { range: rng.clone(), severity: Some(2), message: "warn".into() },
            LspDiagnostic { range: rng, severity: Some(3), message: "info".into() },
        ];
        let rendered = render_status(&app).to_string();
        assert!(rendered.contains("E1"));
        assert!(rendered.contains("W1"));
        assert!(rendered.contains("I1"));
    }

    #[test]
    fn test_context_pct_no_session() {
        let app = App::new(PathBuf::from("/tmp"), "addr".into());
        assert_eq!(context_pct(&app), 0);
    }

    #[test]
    fn test_diag_counts_empty() {
        let app = App::new(PathBuf::from("/tmp"), "addr".into());
        assert_eq!(diag_counts(&app), (0, 0, 0));
    }

    // ── App todos with tempfile ──────────────────────────

    #[test]
    fn test_refresh_todos_empty() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), "addr".into());
        // todo_dirty starts as true, so refresh reads the file
        assert!(app.todo_dirty);
        app.refresh_todos();
        assert!(app.todo_items.is_empty());
        assert!(!app.todo_dirty);
    }

    #[test]
    fn test_refresh_todos_with_data() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("todos.json");
        // Seed a todo via the public API
        sisyphus_tools::todo_add(&db, "write tests", sisyphus_tools::TodoPriority::High).unwrap();

        let mut app = App::new(dir.path().to_path_buf(), "addr".into());
        app.todo_dirty = true;
        app.refresh_todos();
        assert_eq!(app.todo_items.len(), 1);
        assert_eq!(app.todo_items[0].content, "write tests");
        assert_eq!(app.todo_sel, 0);
    }

    #[test]
    fn test_toggle_todo_status_cycles() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("todos.json");
        let _item =
            sisyphus_tools::todo_add(&db, "cycle test", sisyphus_tools::TodoPriority::Medium)
                .unwrap();

        let mut app = App::new(dir.path().to_path_buf(), "addr".into());
        app.todo_dirty = true;
        app.refresh_todos();
        assert_eq!(app.todo_items.len(), 1);
        assert!(matches!(
            app.todo_items[0].status,
            sisyphus_tools::TodoStatus::Pending
        ));

        // Pending → InProgress
        app.toggle_todo_status();
        app.refresh_todos();
        assert!(matches!(
            app.todo_items[0].status,
            sisyphus_tools::TodoStatus::InProgress
        ));

        // InProgress → Completed
        app.toggle_todo_status();
        app.refresh_todos();
        assert!(matches!(
            app.todo_items[0].status,
            sisyphus_tools::TodoStatus::Completed
        ));

        // Completed → Pending
        app.toggle_todo_status();
        app.refresh_todos();
        assert!(matches!(
            app.todo_items[0].status,
            sisyphus_tools::TodoStatus::Pending
        ));
    }

    #[test]
    fn test_delete_todo_removes_item() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("todos.json");
        let _item =
            sisyphus_tools::todo_add(&db, "delete me", sisyphus_tools::TodoPriority::Low).unwrap();

        let mut app = App::new(dir.path().to_path_buf(), "addr".into());
        app.todo_dirty = true;
        app.refresh_todos();
        assert_eq!(app.todo_items.len(), 1);

        // First call sets confirmation, second actually deletes
        app.delete_todo();
        assert!(app.confirm_delete.is_some());
        app.delete_todo();
        app.todo_dirty = true;
        app.refresh_todos();
        assert!(app.todo_items.is_empty());
    }

    #[test]
    fn test_toggle_todo_empty_no_panic() {
        let mut app = App::new(PathBuf::from("/tmp/nonexistent"), "addr".into());
        app.toggle_todo_status(); // should not panic when empty
        app.delete_todo(); // should not panic when empty
    }

    // ── App sessions with tempfile ───────────────────────

    #[test]
    fn test_refresh_sessions_empty() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), "addr".into());
        assert!(app.session_dirty);
        app.refresh_sessions();
        assert!(app.sessions.is_empty());
        assert!(!app.session_dirty);
    }

    #[test]
    fn test_refresh_sessions_with_data() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("sessions.json");
        let _session = sisyphus_tools::session_create(&db, "test-session").unwrap();

        let mut app = App::new(dir.path().to_path_buf(), "addr".into());
        app.session_dirty = true;
        app.refresh_sessions();
        assert_eq!(app.sessions.len(), 1);
        assert_eq!(app.sessions[0].name, "test-session");
    }

    #[test]
    fn test_select_session_loads_detail() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("sessions.json");
        let session = sisyphus_tools::session_create(&db, "detail-test").unwrap();

        let mut app = App::new(dir.path().to_path_buf(), "addr".into());
        app.session_dirty = true;
        app.refresh_sessions();
        assert_eq!(app.sessions.len(), 1);
        assert!(app.session_detail.is_none());

        app.select_session();
        assert!(app.session_detail.is_some());
        let detail = app.session_detail.unwrap();
        assert_eq!(detail.name, "detail-test");
        assert_eq!(detail.id, session.id);
    }

    #[test]
    fn test_select_session_empty_noop() {
        let mut app = App::new(PathBuf::from("/tmp/nonexistent"), "addr".into());
        app.select_session(); // should not panic when empty
        assert!(app.session_detail.is_none());
    }
}
