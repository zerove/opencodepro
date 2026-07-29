use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::bridge::SopInfo;
use crate::tui::dialog::Dialog;

use super::helpers;
use super::theme;

// ── Skill Market DTO ────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteSkillEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub author: String,
    pub version: String,
}

// ── Workspace DTO ────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceEntry {
    pub id: String,
    pub name: String,
    pub project_id: String,
}

// ── Log entries (used by background + shell) ─────────────

#[derive(Debug, Clone)]
pub(crate) struct TraceEvent {
    pub node_id: String,
    pub status: String,
    #[expect(dead_code)]
    pub output: Option<String>,
    pub fuel: i64,
    pub duration: i64,
}

#[derive(Debug, Clone)]
pub enum LogEntry {
    Info(String),
    Success(String),
    Warn(String),
    Error(String),
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
        Self {
            kind,
            message: message.into(),
            created_at: Instant::now(),
        }
    }

    pub(crate) fn style(&self, theme: &theme::Theme) -> (Color, Color) {
        match self.kind {
            ToastKind::Success => (Color::Black, theme.success),
            ToastKind::Info => (Color::Black, theme.primary),
        }
    }
}

use ratatui::style::Color;

const MAX_TOASTS: usize = 5;
const TOAST_DURATION: Duration = Duration::from_secs(4);

// ── Command Palette ─────────────────────────────────────

#[derive(Clone)]
pub(crate) struct CmdPalEntry {
    pub(crate) label: &'static str,
    pub(crate) desc: &'static str,
    pub(crate) command: &'static str,
    pub(crate) shortcut: &'static str,
}

const COMMAND_PALETTE: &[CmdPalEntry] = &[
    CmdPalEntry {
        label: "Session List",
        desc: "Switch or manage sessions",
        command: ":sessions",
        shortcut: "Ctrl+L",
    },
    CmdPalEntry {
        label: "Model List",
        desc: "Switch AI model",
        command: ":model",
        shortcut: "Ctrl+M",
    },
    CmdPalEntry {
        label: "Agent List",
        desc: "Switch agent",
        command: ":agent",
        shortcut: "Ctrl+A",
    },
    CmdPalEntry {
        label: "Todos",
        desc: "Manage todo list",
        command: ":todos",
        shortcut: "",
    },
    CmdPalEntry {
        label: "LSP Diagnostics",
        desc: "View LSP diagnostics",
        command: ":lsp",
        shortcut: "",
    },
    CmdPalEntry {
        label: "SOP List",
        desc: "List & execute SOPs",
        command: ":sop",
        shortcut: "",
    },
    CmdPalEntry {
        label: "Skill Market",
        desc: "Browse & install remote skills",
        command: ":skills",
        shortcut: "",
    },
    CmdPalEntry {
        label: "Roadmap",
        desc: "View project roadmap",
        command: ":roadmap",
        shortcut: "",
    },
    CmdPalEntry {
        label: "Bridge Status",
        desc: "Check bridge health",
        command: ":bridge status",
        shortcut: "",
    },
    CmdPalEntry {
        label: "Help",
        desc: "Show keybindings & help",
        command: ":help",
        shortcut: "F1",
    },
    CmdPalEntry {
        label: "Dev Agent",
        desc: "Run autonomous development agent",
        command: ":dev",
        shortcut: "",
    },
    CmdPalEntry {
        label: "Clear Log",
        desc: "Clear the log",
        command: "clear",
        shortcut: "",
    },
    CmdPalEntry {
        label: "Exit",
        desc: "Exit Sisyphus",
        command: "exit",
        shortcut: "Ctrl+Q",
    },
    CmdPalEntry {
        label: "New Session",
        desc: "Create a new session",
        command: ":new",
        shortcut: "Ctrl+N",
    },
    CmdPalEntry {
        label: "Go Home",
        desc: "Return to Home screen",
        command: ":home",
        shortcut: "Ctrl+H",
    },
    CmdPalEntry {
        label: "Toggle Theme",
        desc: "Switch dark/light theme",
        command: ":theme",
        shortcut: "",
    },
];

pub(crate) fn matching_palette_entries(query: &str) -> Vec<&'static CmdPalEntry> {
    let q = query.to_lowercase();
    COMMAND_PALETTE
        .iter()
        .filter(|e| {
            q.is_empty()
                || e.label.to_lowercase().contains(&q)
                || e.desc.to_lowercase().contains(&q)
                || e.command.contains(&q)
        })
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
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

// ── App state ────────────────────────────────────────────

pub(crate) struct App {
    // sync
    pub(crate) data_dir: PathBuf,
    pub(crate) boulder_path: PathBuf,
    pub(crate) boulder_loaded: bool,
    pub(crate) boulder_dirty: bool,
    pub(crate) boulder_save_tick: u32,
    pub(crate) bridge_address: String,
    pub(crate) bridge_client: Option<crate::bridge::BridgeClient>,

    // routing
    pub(crate) view: View,

    // home
    pub(crate) home_input: String,
    pub(crate) home_cursor: usize,
    pub(crate) home_placeholder_idx: usize,

    // session
    pub(crate) sessions: Vec<crate::tools::Session>,
    pub(crate) session_filter: String,
    pub(crate) session_sel: usize,
    pub(crate) session_detail: Option<crate::tools::Session>,
    pub(crate) session_detail_scroll: usize,
    pub(crate) session_dirty: bool,
    pub(crate) session_input: String,
    pub(crate) session_cursor: usize,
    pub(crate) session_mode: Mode,
    pub(crate) session_suggestions: Vec<String>,

    // shared log / command history
    pub(crate) log: Vec<LogEntry>,
    pub(crate) scroll: usize,
    pub(crate) notification: Option<(String, Instant)>,
    pub(crate) history: Vec<String>,
    pub(crate) history_idx: Option<usize>,
    pub(crate) exit_requested: bool,
    pub(crate) exit_immediately: bool,
    pub(crate) dialog: Option<Dialog>,
    pub(crate) dialog_sel: usize,
    pub(crate) dialog_query: String,
    pub(crate) help_scroll: usize,

    // todo data (dialog state)
    pub(crate) todo_items: Vec<crate::tools::TodoItem>,
    pub(crate) todo_sel: usize,
    pub(crate) todo_dirty: bool,
    pub(crate) todo_adding: String,

    // lsp data (dialog state)
    pub(crate) lsp_diagnostics: Vec<crate::tools::lsp::LspDiagnostic>,
    pub(crate) lsp_status: String,
    pub(crate) lsp_language: String,
    pub(crate) lsp_server: String,
    pub(crate) lsp_scroll: usize,

    // sop data (dialog state)
    pub(crate) sop_sops: Vec<SopInfo>,
    pub(crate) sop_sel: usize,
    pub(crate) sop_status: String,
    pub(crate) sop_result: Option<String>,
    pub(crate) sop_trace: Vec<TraceEvent>,
    pub(crate) sop_trace_scroll: usize,
    pub(crate) sop_query: String,

    // roadmap data (dialog state)
    pub(crate) roadmap_projection: Option<String>,
    pub(crate) roadmap_bottlenecks: Option<String>,
    pub(crate) roadmap_status: String,
    pub(crate) roadmap_scroll: usize,
    #[expect(dead_code)]
    pub(crate) roadmap_sel: usize,
    #[expect(dead_code)]
    pub(crate) proposal_sel: usize,
    pub(crate) skill_market_available: Vec<RemoteSkillEntry>,
    pub(crate) skill_market_installed: Vec<RemoteSkillEntry>,
    pub(crate) skill_market_sel: usize,
    pub(crate) skill_market_status: String,
    pub(crate) workspaces: Vec<WorkspaceEntry>,
    pub(crate) workspace_sel: usize,
    pub(crate) workspace_status: String,
    pub(crate) pending_op: Option<tokio::sync::oneshot::Receiver<crate::tui::background::BgOp>>,

    // command palette
    pub(crate) palette_open: bool,
    pub(crate) palette_query: String,
    pub(crate) palette_sel: usize,

    // slash command completion
    pub(crate) slash_completion_open: bool,
    pub(crate) slash_completion_sel: usize,

    // auto-refresh
    #[expect(dead_code)]
    pub(crate) last_refresh: Instant,
    pub(crate) event_rx: Option<tokio::sync::mpsc::Receiver<crate::tui::background::BgOp>>,
    pub(crate) is_pending: bool,
    pub(crate) pending_spin: u8,

    // misc
    pub(crate) confirm_delete: Option<String>,
    pub(crate) expanded_thoughts: HashSet<usize>,
    /// Y-level of the history area in the session detail (stored for click-to-message mapping).
    pub(crate) last_detail_area: std::cell::Cell<ratatui::layout::Rect>,
    /// Effective scroll offset at last render (stored for click-to-message mapping).
    pub(crate) last_effective_scroll: std::cell::Cell<usize>,
    /// Max scroll value at last render (stored for scroll boundary clamping).
    pub(crate) last_detail_max_scroll: std::cell::Cell<usize>,
    pub(crate) create_buf: String,
    /// Which message is currently selected/highlighted for clipboard operations.
    pub(crate) selected_msg: Option<usize>,

    // session message search
    pub(crate) search_active: bool,
    pub(crate) search_query: String,
    pub(crate) search_results: Vec<usize>,
    pub(crate) search_idx: usize,

    /// Toast notification overlay
    pub(crate) toasts: Vec<Toast>,

    pub(crate) theme_mode: theme::ThemeMode,
    pub(crate) model_name: String,
    pub(crate) leader_mode: bool,
    pub(crate) ralph_enabled: bool,
    pub(crate) ralph_idle_cycles: u32,
    pub(crate) ralph_continue_pending: bool,

    // dev agent dialog
    pub(crate) dev_goal: String,
    pub(crate) dev_context: String,
    pub(crate) dev_parallel: bool,
    pub(crate) dev_resume: bool,
    pub(crate) dev_report: Option<String>,
    pub(crate) dev_status: String,
    pub(crate) dev_focus: DevFocus,
}

const MAX_LOG_ENTRIES: usize = 1000;
#[expect(dead_code)]
pub(crate) const AUTO_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
pub(crate) const RALPH_IDLE_THRESHOLD: u32 = 50;

impl App {
    pub(crate) fn new(data_dir: PathBuf, bridge_address: String) -> Self {
        let boulder_path = data_dir.join(".omo").join("boulder.json");
        // Attempt initial connection to the bridge
        let bridge_client =
            futures::executor::block_on(crate::bridge::BridgeClient::connect(&bridge_address)).ok();
        Self {
            data_dir,
            boulder_path,
            boulder_loaded: false,
            boulder_dirty: false,
            boulder_save_tick: 0,
            bridge_address,
            bridge_client,
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
            log: vec![LogEntry::Info(
                "Sisyphus — Ctrl+K: Palette | /help: shortcuts | Type / in input for commands"
                    .into(),
            )],
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
            sop_trace: Vec::new(),
            sop_trace_scroll: 0,
            sop_query: String::new(),
            roadmap_projection: None,
            roadmap_bottlenecks: None,
            roadmap_status: "disconnected".into(),
            roadmap_scroll: 0,
            roadmap_sel: 0,
            proposal_sel: 0,
            skill_market_available: Vec::new(),
            skill_market_installed: Vec::new(),
            skill_market_sel: 0,
            skill_market_status: "idle".into(),
            workspaces: Vec::new(),
            workspace_sel: 0,
            workspace_status: "idle".into(),
            pending_op: None,
            palette_open: false,
            palette_query: String::new(),
            palette_sel: 0,
            slash_completion_open: false,
            slash_completion_sel: 0,
            last_refresh: Instant::now(),
            event_rx: None,
            is_pending: false,
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

    pub(crate) fn save_boulder(&mut self) {
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
                self.add_log(LogEntry::Error(format!(
                    "Failed to create boulder dir: {e}"
                )));
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

    pub(crate) fn load_boulder(&mut self) {
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
            if let Ok(session) = crate::tools::session_get(&db, &id) {
                self.session_detail = Some(session);
            }
        }

        self.boulder_loaded = true;
    }

    pub(crate) fn theme(&self) -> theme::Theme {
        match self.theme_mode {
            theme::ThemeMode::Dark => theme::Theme::dark(),
            theme::ThemeMode::Light => theme::Theme::light(),
        }
    }

    pub(crate) fn toggle_theme(&mut self) {
        self.theme_mode = match self.theme_mode {
            theme::ThemeMode::Dark => theme::ThemeMode::Light,
            theme::ThemeMode::Light => theme::ThemeMode::Dark,
        };
        self.boulder_dirty = true;
    }

    // ── thought block helpers ────────────────────────────

    pub(crate) fn is_thought_expanded(&self, msg_idx: usize) -> bool {
        self.expanded_thoughts.contains(&msg_idx)
    }

    pub(crate) fn msg_start_line(&self, msg_idx: usize) -> usize {
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

    pub(crate) fn thought_toggle_line_diff(&self, msg_idx: usize) -> isize {
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

    pub(crate) fn adjust_session_scroll(&mut self, delta: isize) {
        let max_scroll = self.last_detail_max_scroll.get();
        let current = if self.session_detail_scroll == usize::MAX {
            max_scroll
        } else {
            self.session_detail_scroll
        };
        let candidate = (current as isize + delta).max(0) as usize;
        let clamped = candidate.min(max_scroll);
        if clamped >= max_scroll && delta > 0 {
            self.session_detail_scroll = usize::MAX;
        } else {
            self.session_detail_scroll = clamped;
        }
    }

    pub(crate) fn toggle_message_thoughts(&mut self, msg_idx: usize) {
        let line_diff = self.thought_toggle_line_diff(msg_idx);
        let msg_start = self.msg_start_line(msg_idx);

        if !self.expanded_thoughts.remove(&msg_idx) {
            self.expanded_thoughts.insert(msg_idx);
        }

        if self.session_detail_scroll != usize::MAX
            && line_diff != 0
            && msg_start < self.session_detail_scroll + 100
        {
            self.session_detail_scroll = if line_diff > 0 {
                self.session_detail_scroll
                    .saturating_add(line_diff as usize)
            } else {
                self.session_detail_scroll
                    .saturating_sub((-line_diff) as usize)
            };
        }
    }

    pub(crate) fn toggle_all_thoughts(&mut self) {
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

    pub(crate) fn find_message_at_row(&self, row: u16) -> Option<usize> {
        let session = self.session_detail.as_ref()?;
        let area = self.last_detail_area.get();
        let scroll = self.last_effective_scroll.get();

        let y_in_history = row.saturating_sub(area.y);
        let y_in_content = y_in_history.saturating_sub(1);
        let target_line = y_in_content as usize + scroll;

        let mut offset = 0usize;
        for (idx, msg) in session.messages.iter().enumerate() {
            let start = offset;
            offset += 1;
            offset += helpers::count_message_lines(&msg.content, self.is_thought_expanded(idx));
            offset += 1;
            if target_line >= start && target_line < offset {
                return Some(idx);
            }
        }
        None
    }

    pub(crate) fn add_log(&mut self, entry: LogEntry) {
        self.log.push(entry);
        if self.log.len() > MAX_LOG_ENTRIES {
            let excess = self.log.len() - MAX_LOG_ENTRIES;
            self.log.drain(..excess);
            self.scroll = self.scroll.saturating_sub(excess);
        }
        self.scroll = self.log.len().saturating_sub(1);
    }

    pub(crate) fn add_toast(&mut self, kind: ToastKind, message: impl Into<String>) {
        self.toasts.push(Toast::new(kind, message));
        if self.toasts.len() > MAX_TOASTS {
            self.toasts.remove(0);
        }
    }

    pub(crate) fn prune_expired_toasts(&mut self) {
        self.toasts
            .retain(|t| t.created_at.elapsed() < TOAST_DURATION);
    }

    pub(crate) fn perform_search(&mut self) {
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
        let idx = self.search_results[self
            .search_idx
            .min(self.search_results.len().saturating_sub(1))];
        self.selected_msg = Some(idx);
        if let Some(ref detail) = self.session_detail {
            let mut line = 0usize;
            for (i, msg) in detail.messages.iter().enumerate() {
                if i >= idx {
                    break;
                }
                line += 1; // header
                line += msg.content.lines().count().max(1); // content
                line += 1; // spacing
            }
            self.session_detail_scroll = line.saturating_sub(2);
        } else {
            self.session_detail_scroll = 0;
        }
    }

    pub(crate) fn next_search_match(&mut self) {
        if self.search_results.is_empty() {
            return;
        }
        self.search_idx = (self.search_idx + 1) % self.search_results.len();
        self.scroll_to_search_match();
    }

    pub(crate) fn prev_search_match(&mut self) {
        if self.search_results.is_empty() {
            return;
        }
        self.search_idx = if self.search_idx == 0 {
            self.search_results.len().saturating_sub(1)
        } else {
            self.search_idx - 1
        };
        self.scroll_to_search_match();
    }

    // ── todos ────────────────────────────────────
    pub(crate) fn refresh_todos(&mut self) {
        if !self.todo_dirty {
            return;
        }
        let db = self.data_dir.join("todos.json");
        self.todo_items = crate::tools::todo_list(&db, None).unwrap_or_default();
        self.todo_sel = self.todo_sel.min(self.todo_items.len().saturating_sub(1));
        self.todo_dirty = false;
    }

    #[allow(dead_code)]
    pub(crate) fn toggle_todo_status(&mut self) {
        if self.todo_items.is_empty() {
            return;
        }
        let db = self.data_dir.join("todos.json");
        let item = &self.todo_items[self.todo_sel];
        let new_status = match item.status {
            crate::tools::TodoStatus::Pending => crate::tools::TodoStatus::InProgress,
            crate::tools::TodoStatus::InProgress => crate::tools::TodoStatus::Completed,
            crate::tools::TodoStatus::Completed => crate::tools::TodoStatus::Pending,
            crate::tools::TodoStatus::Cancelled => crate::tools::TodoStatus::Pending,
        };
        if crate::tools::todo_update(&db, &item.id, None, Some(new_status), None).is_ok() {
            self.todo_dirty = true;
        }
    }

    #[allow(dead_code)]
    pub(crate) fn delete_todo(&mut self) {
        if self.todo_items.is_empty() {
            return;
        }
        let id = self.todo_items[self.todo_sel].id.clone();
        if self.confirm_delete.as_deref() == Some(&id) {
            let db = self.data_dir.join("todos.json");
            if crate::tools::todo_delete(&db, &id).is_ok() {
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
    pub(crate) fn refresh_sessions(&mut self) {
        if !self.session_dirty {
            return;
        }
        let db = self.data_dir.join("sessions.json");
        match crate::tools::session_list(&db) {
            Ok(sessions) => self.sessions = sessions,
            Err(e) => self.add_log(LogEntry::Error(format!("Failed to list sessions: {e}"))),
        }
        self.session_sel = self.session_sel.min(self.sessions.len().saturating_sub(1));
        self.session_dirty = false;
    }

    pub(crate) fn filtered_sessions(&self) -> Vec<usize> {
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

    pub(crate) fn select_session(&mut self) {
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
        match crate::tools::session_get(&db, id) {
            Ok(session) => self.session_detail = Some(session),
            Err(e) => self.add_log(LogEntry::Error(format!("Failed to load session {id}: {e}"))),
        }
        self.session_detail_scroll = usize::MAX; // auto-scroll to latest messages
        self.boulder_dirty = true;
    }

    pub(crate) fn delete_session(&mut self) {
        let filtered = self.filtered_sessions();
        if filtered.is_empty() {
            return;
        }
        let idx = filtered[self.session_sel.min(filtered.len().saturating_sub(1))];
        let db = self.data_dir.join("sessions.json");
        let id = self.sessions[idx].id.clone();
        if self.confirm_delete.as_deref() == Some(&id) {
            match crate::tools::session_delete(&db, &id) {
                Ok(_) => {
                    self.add_log(LogEntry::Info(format!("Deleted session {id}")));
                    self.session_dirty = true;
                    self.session_sel = self.session_sel.saturating_sub(1);
                }
                Err(e) => self.add_log(LogEntry::Error(format!(
                    "Failed to delete session {id}: {e}"
                ))),
            }
            self.confirm_delete = None;
        } else {
            self.confirm_delete = Some(id);
            self.add_log(LogEntry::Warn("Press Delete again to confirm".into()));
        }
    }

    pub(crate) fn process_events(&mut self) {
        let mut ops = Vec::new();
        if let Some(rx) = self.event_rx.as_mut() {
            while let Ok(op) = rx.try_recv() {
                ops.push(op);
            }
        }
        if let Some(mut rx) = self.pending_op.take() {
            if let Ok(op) = rx.try_recv() {
                ops.push(op);
            } else {
                self.pending_op = Some(rx);
            }
        }
        for op in ops {
            self.apply_bg_op(op);
        }
    }

    fn apply_bg_op(&mut self, op: crate::tui::background::BgOp) {
        match op {
            crate::tui::background::BgOp::SopsListed { count, sops } => {
                self.sop_sops = sops;
                self.sop_sel = 0;
                self.sop_status = format!("connected ({} SOPs)", count);
            }
            crate::tui::background::BgOp::SopsFailed(e) => {
                self.sop_status = format!("failed: {e}");
            }
            crate::tui::background::BgOp::SopExecuted { sop_id, result } => {
                self.sop_result = Some(result);
                self.sop_status = format!("executed {sop_id}");
            }
            crate::tui::background::BgOp::SopExecuteFailed { sop_id, error } => {
                self.sop_result = None;
                self.sop_status = format!("execute {sop_id} failed: {error}");
            }
            crate::tui::background::BgOp::SopTraceFetched { sop_id, trace } => {
                self.sop_trace = trace
                    .into_iter()
                    .map(|e| crate::tui::app::TraceEvent {
                        node_id: e.node_id,
                        status: e.status,
                        output: e.output,
                        fuel: e.fuel_consumed,
                        duration: e.duration_ms,
                    })
                    .collect();
                self.sop_trace_scroll = 0;
                self.add_log(LogEntry::Info(format!("Fetched trace for {sop_id}")));
            }
            crate::tui::background::BgOp::SopTraceFailed(e) => {
                self.add_log(LogEntry::Error(format!("Failed to fetch trace: {e}")));
            }
            crate::tui::background::BgOp::RoadmapFetched {
                projection,
                bottlenecks,
            } => {
                self.roadmap_projection = Some(projection);
                self.roadmap_bottlenecks = Some(bottlenecks);
                self.roadmap_status = "connected".into();
            }
            crate::tui::background::BgOp::RoadmapFailed(e) => {
                self.roadmap_projection = None;
                self.roadmap_bottlenecks = None;
                self.roadmap_status = format!("failed: {e}");
            }
            crate::tui::background::BgOp::BridgeCmd { entries } => {
                for entry in entries {
                    self.add_log(entry);
                }
            }
            crate::tui::background::BgOp::DevResult {
                goal,
                report,
                success,
            } => {
                if success {
                    self.add_log(LogEntry::Success(format!("Dev agent completed: {goal}")));
                } else {
                    self.add_log(LogEntry::Warn(format!(
                        "Dev agent finished with issues: {goal}"
                    )));
                }
                self.dev_report = Some(report);
                self.dev_status = "done".into();
            }
            crate::tui::background::BgOp::SessionUpdated { session_id, errors } => {
                for entry in &errors {
                    self.add_log(entry.clone());
                }
                if let Some(first_err) = errors.first() {
                    self.notification = Some((
                        first_err.entry_text().to_string(),
                        std::time::Instant::now(),
                    ));
                }
                self.refresh_sessions();
                let viewing_session = self.session_detail.as_ref().map(|d| d.id.clone());
                if viewing_session.as_deref() == Some(&session_id) {
                    let db = self.data_dir.join("sessions.json");
                    if let Ok(session) = crate::tools::session_get(&db, &session_id) {
                        self.session_detail = Some(session);
                    }
                    self.session_detail_scroll = usize::MAX;
                    // Generate contextual suggestions after agent response
                    let last_role = self
                        .session_detail
                        .as_ref()
                        .and_then(|d| d.messages.last())
                        .map(|m| m.role.as_str());
                    if last_role == Some("assistant") {
                        self.session_suggestions = vec![
                            "Type a follow-up message or press Esc to browse sessions".into(),
                            "Use /agent off to disable AI responses".into(),
                            "Use /suggest <topic> for targeted suggestions".into(),
                        ];
                    } else {
                        self.session_suggestions = vec![
                            "Press Enter to send your message".into(),
                            "Type /help for available commands".into(),
                            "Use @filename to reference a file".into(),
                        ];
                    }
                }
            }
            crate::tui::background::BgOp::TodoUpdated => {
                self.todo_dirty = true;
            }
            crate::tui::background::BgOp::SkillsAvailable { skills } => {
                self.skill_market_available = skills;
                self.skill_market_status = "available skills loaded".into();
            }
            crate::tui::background::BgOp::SkillsInstalled { skills } => {
                self.skill_market_installed = skills;
                self.skill_market_status = "installed skills loaded".into();
            }
            crate::tui::background::BgOp::SkillOpResult { success, message } => {
                if success {
                    self.add_log(LogEntry::Success(message));
                    self.skill_market_status = "operation successful".into();
                } else {
                    self.add_log(LogEntry::Error(message));
                    self.skill_market_status = "operation failed".into();
                }
            }
            crate::tui::background::BgOp::WorkspacesListed { workspaces } => {
                self.workspaces = workspaces;
                self.workspace_status = "loaded".into();
            }
            crate::tui::background::BgOp::WorkspaceSwitched { id } => {
                self.workspace_status = format!("switched to {id}");
                self.add_log(LogEntry::Info(format!("Workspace switched to {id}")));
            }
            crate::tui::background::BgOp::WorkspaceOpResult { success, message } => {
                if success {
                    self.add_log(LogEntry::Success(message));
                    self.workspace_status = "operation successful".into();
                } else {
                    self.add_log(LogEntry::Error(message.clone()));
                    self.workspace_status = format!("error: {message}");
                }
            }
        }
    }
    pub(crate) fn fetch_sop_trace(&mut self, sop_id: String) {
        let bridge_addr = self.bridge_address.clone();
        let rx = crate::tui::background::spawn_fetch_sop_trace(bridge_addr, sop_id);
        self.pending_op = Some(rx);
    }
}
