use std::path::PathBuf;
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseButton};
use crossterm::event::MouseEventKind;
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
    DefaultTerminal,
};

mod background;
mod helpers;
mod lsp;
mod roadmap;
mod sessions;
mod shell;
mod sop;
mod theme;
mod todos;
pub(crate) use background::BgOp;
pub(crate) use background::{
    spawn_fetch_roadmap, spawn_list_sops,
};
pub(crate) use helpers::{paste_from_clipboard, tab_at_x};
use lsp::{handle_lsp_key, render_lsp};
use roadmap::{handle_roadmap_key, render_roadmap};
use sessions::{handle_sessions_key, render_sessions};
use shell::{handle_shell_key, render_shell};
use sop::{handle_sop_key, render_sop};
use todos::{handle_todos_key, render_todos};

use crate::permission::CliPermission;

// ── Log entries (used by background + shell) ─────────────

#[derive(Debug, Clone)]
pub(crate) enum LogEntry {
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

// ── Panels ───────────────────────────────────────────────

// ── Command Palette ─────────────────────────────────────

#[derive(Clone)]
#[allow(dead_code)]
struct CmdPalEntry {
    label: &'static str,
    desc: &'static str,
    command: &'static str,
    shortcut: &'static str,
}

const COMMAND_PALETTE: &[CmdPalEntry] = &[
    CmdPalEntry { label: "Help", desc: "Show keybindings & help", command: ":help", shortcut: "F1" },
    CmdPalEntry { label: "Clear Log", desc: "Clear the shell log", command: "clear", shortcut: "" },
    CmdPalEntry { label: "Exit", desc: "Exit Sisyphus", command: "exit", shortcut: "Ctrl+Q" },
    CmdPalEntry { label: "Shell Panel", desc: "Switch to Shell panel", command: ":panel shell", shortcut: "1" },
    CmdPalEntry { label: "Todos Panel", desc: "Switch to Todos panel", command: ":panel todos", shortcut: "2" },
    CmdPalEntry { label: "Sessions Panel", desc: "Switch to Sessions panel", command: ":panel sessions", shortcut: "3" },
    CmdPalEntry { label: "LSP Panel", desc: "Switch to LSP diagnostics panel", command: ":panel lsp", shortcut: "4" },
    CmdPalEntry { label: "SOP Panel", desc: "Switch to SOP panel", command: ":panel sop", shortcut: "5" },
    CmdPalEntry { label: "Roadmap Panel", desc: "Switch to Roadmap panel", command: ":panel roadmap", shortcut: "6" },
    CmdPalEntry { label: "Bridge Status", desc: "Check bridge health", command: ":bridge status", shortcut: "" },
    CmdPalEntry { label: "Bridge SOPs", desc: "List registered SOPs", command: ":bridge sops", shortcut: "" },
    CmdPalEntry { label: "Refresh Sessions", desc: "Reload session list", command: ":refresh sessions", shortcut: "" },
    CmdPalEntry { label: "Refresh Todos", desc: "Reload todo list", command: ":refresh todos", shortcut: "" },
];

fn matching_palette_entries(query: &str) -> Vec<&'static CmdPalEntry> {
    let q = query.to_lowercase();
    COMMAND_PALETTE
        .iter()
        .filter(|e| q.is_empty() || e.label.to_lowercase().contains(&q) || e.desc.to_lowercase().contains(&q) || e.command.contains(&q))
        .collect()
}

// ── Panels ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Panel {
    Shell,
    Todos,
    Sessions,
    Lsp,
    Sop,
    Roadmap,
}

impl Panel {
    fn all() -> &'static [Panel] {
        &[
            Panel::Shell,
            Panel::Todos,
            Panel::Sessions,
            Panel::Lsp,
            Panel::Sop,
            Panel::Roadmap,
        ]
    }
    fn name(self) -> &'static str {
        match self {
            Panel::Shell => "Shell",
            Panel::Todos => "Todos",
            Panel::Sessions => "Sessions",
            Panel::Lsp => "LSP",
            Panel::Sop => "SOP",
            Panel::Roadmap => "Roadmap",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LspInputMode {
    FilePath,
    Language,
    Server,
    ServerArgs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BoulderState {
    pub active_session_id: Option<String>,
    pub active_panel: Panel,
    pub session_scroll: usize,
    pub session_filter: String,
    pub lsp_language: String,
    pub lsp_server: String,
    pub sop_query: String,
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

    // panel
    active_panel: Panel,
    mode: Mode,

    // shell
    input: String,
    cursor_pos: usize,
    log: Vec<LogEntry>,
    scroll: usize,
    history: Vec<String>,
    history_idx: Option<usize>,
    exit_requested: bool,
    exit_immediately: bool,
    slash_sel: usize,

    // todos
    todo_items: Vec<sisyphus_tools::TodoItem>,
    todo_sel: usize,
    todo_dirty: bool,
    todo_adding: String,           // non-empty = inline add mode

    // sessions
    sessions: Vec<sisyphus_tools::Session>,
    session_filter: String,
    session_sel: usize,
    session_detail: Option<sisyphus_tools::Session>,
    session_detail_scroll: usize,
    session_dirty: bool,
    session_input: String,
    session_mode: Mode,
    session_suggestions: Vec<String>,

    // lsp diagnostics
    lsp_input: String,
    lsp_input_mode: LspInputMode,
    lsp_language: String,
    lsp_server: String,
    lsp_server_args: String,
    lsp_diagnostics: Vec<sisyphus_tools::lsp::LspDiagnostic>,
    lsp_status: String,
    lsp_scroll: usize,
    lsp_last_error: String,

    // sop panel
    sop_sops: Vec<crate::bridge::SopInfo>,
    sop_sel: usize,
    sop_status: String,
    sop_result: Option<String>,
    sop_query: String,

    // roadmap panel
    roadmap_projection: Option<String>,
    roadmap_bottlenecks: Option<String>,
    roadmap_status: String,
    roadmap_show_projection: bool,
    roadmap_show_bottlenecks: bool,
    roadmap_scroll: usize,

    // command palette
    palette_open: bool,
    palette_query: String,
    palette_sel: usize,

    // auto-refresh
    last_refresh: Instant,

    pending_op: Option<tokio::sync::oneshot::Receiver<BgOp>>,
    pending_spin: u8,
    agent_mode: bool,

    // confirmation / input
    confirm_delete: Option<String>,
    rename_buf: String,
    rename_session_id: Option<String>,
    create_buf: String,

    // clipboard
    copy_notification: Option<Instant>,

    theme_mode: theme::ThemeMode,
    model_name: String,
    leader_mode: bool,
    ralph_enabled: bool,
    ralph_idle_cycles: u32,
    ralph_continue_pending: bool,
}

const MAX_LOG_ENTRIES: usize = 1000;
const AUTO_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const RALPH_IDLE_THRESHOLD: u32 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Input,
    Scroll,
    Select,
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
            active_panel: Panel::Sessions,
            mode: Mode::Select,
            input: String::new(),
            cursor_pos: 0,
            log: vec![LogEntry::Info("Sisyphus CLI — Ctrl-1 Todos | Ctrl-2 Sessions | Ctrl-3 LSP | Ctrl-4 SOP | Ctrl-5 Roadmap".into())],
            scroll: 0,
            history: Vec::new(),
            history_idx: None,
            exit_requested: false,
            exit_immediately: false,
            slash_sel: 0,
            todo_items: Vec::new(),
            todo_sel: 0,
            todo_dirty: true,
            todo_adding: String::new(),
            sessions: Vec::new(),
            session_filter: String::new(),
            session_sel: 0,
            session_detail: None,
            session_detail_scroll: 0,
            session_dirty: true,
            session_input: String::new(),
            session_mode: Mode::Select,
            session_suggestions: vec![
                "Try /suggest to get AI suggestions".into(),
                "Type /agent status to check agent mode".into(),
                "Use Tab to autocomplete @files".into(),
            ],
            lsp_input: String::new(),
            lsp_input_mode: LspInputMode::FilePath,
            lsp_language: "rust".into(),
            lsp_server: "rust-analyzer".into(),
            lsp_server_args: String::new(),
            lsp_diagnostics: Vec::new(),
            lsp_status: "idle".into(),
            lsp_scroll: 0,
            lsp_last_error: String::new(),
            sop_sops: Vec::new(),
            sop_sel: 0,
            sop_status: "disconnected".into(),
            sop_result: None,
            sop_query: String::new(),
            roadmap_projection: None,
            roadmap_bottlenecks: None,
            roadmap_status: "disconnected".into(),
            roadmap_show_projection: false,
            roadmap_show_bottlenecks: false,
            roadmap_scroll: 0,
            palette_open: false,
            palette_query: String::new(),
            palette_sel: 0,
            last_refresh: Instant::now(),
            pending_op: None,
            pending_spin: 0,
            agent_mode: true,
            confirm_delete: None,
            rename_buf: String::new(),
            rename_session_id: None,
            create_buf: String::new(),
            copy_notification: None,
            theme_mode: theme::ThemeMode::Dark,
            model_name: "unknown".into(),
            leader_mode: false,
            ralph_enabled: false,
            ralph_idle_cycles: 0,
            ralph_continue_pending: false,
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
            active_panel: self.active_panel,
            session_scroll: self.session_detail_scroll,
            session_filter: self.session_filter.clone(),
            lsp_language: self.lsp_language.clone(),
            lsp_server: self.lsp_server.clone(),
            sop_query: self.sop_query.clone(),
            last_updated: now,
        };

        if let Some(parent) = self.boulder_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        if let Ok(json) = serde_json::to_string_pretty(&state) {
            if std::fs::write(&self.boulder_path, json).is_ok() {
                self.boulder_dirty = false;
            }
        }
    }

    fn load_boulder(&mut self) {
        let Ok(content) = std::fs::read_to_string(&self.boulder_path) else {
            return;
        };

        let Ok(state) = serde_json::from_str::<BoulderState>(&content) else {
            return;
        };

        self.active_panel = state.active_panel;
        self.session_filter = state.session_filter;
        self.session_detail_scroll = state.session_scroll;
        self.lsp_language = state.lsp_language;
        self.lsp_server = state.lsp_server;
        self.sop_query = state.sop_query;

        if let Some(id) = state.active_session_id {
            let db = self.data_dir.join("sessions.json");
            if let Ok(session) = sisyphus_tools::session_get(&db, &id) {
                self.session_detail = Some(session);
            }
        }

        self.boulder_loaded = true;
    }


    // ── shell ────────────────────────────────────
    fn theme(&self) -> theme::Theme {
        match self.theme_mode {
            theme::ThemeMode::Dark => theme::Theme::dark(),
            theme::ThemeMode::Light => theme::Theme::light(),
        }
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
        self.sessions = sisyphus_tools::session_list(&db).unwrap_or_default();
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
        self.session_detail = sisyphus_tools::session_get(&db, id).ok();
        self.session_detail_scroll = 0;
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
    fn filtered_sops(&self) -> Vec<&crate::bridge::SopInfo> {
        if self.sop_query.is_empty() {
            return self.sop_sops.iter().collect();
        }
        let q = self.sop_query.to_lowercase();
        self.sop_sops.iter().filter(|s| s.sop_id.to_lowercase().contains(&q)).collect()
    }
}

// ── Public entry ────────────────────────────────────────

pub async fn run(data_dir: PathBuf, bridge_address: String) -> Result<()> {
    // Ensure data directory exists (same as CLI mode in main.rs:1134)
    std::fs::create_dir_all(&data_dir)?;

    let mut terminal = ratatui::init();
    terminal.clear()?;

    crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)?;

    let perms = CliPermission::load(None).unwrap_or_default();
    let mut app = App::new(data_dir, bridge_address);

    let db = app.data_dir.join("sessions.json");
    let sessions = sisyphus_tools::session_list(&db).unwrap_or_default();
    if sessions.is_empty() {
        let name = "默认会话".to_string();
        if let Ok(session) = sisyphus_tools::session_create(&db, &name) {
            app.session_detail = Some(session);
            app.session_detail_scroll = usize::MAX;
            app.session_dirty = true;
        }
    } else {
        // Auto-select first session so content is visible immediately
        app.sessions = sessions;
        app.session_detail = sisyphus_tools::session_get(&db, &app.sessions[0].id).ok();
        app.session_detail_scroll = usize::MAX;
        app.session_dirty = false;
    }

    app.load_boulder();

    let tick_rate = Duration::from_millis(100);
    let res = run_loop(&mut terminal, &mut app, &perms, tick_rate).await;

    crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture)?;
    ratatui::restore();
    res
}

async fn run_loop(
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
        if event::poll(tick_rate)? {
            event_received = true;
            let event = event::read()?;
            if handle_event(event, app, perms).await? {
                break;
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
            app.input = "continue".into();
            app.cursor_pos = 8;
            let fake_event = Event::Key(crossterm::event::KeyEvent::new(
                KeyCode::Enter,
                crossterm::event::KeyModifiers::empty(),
            ));
            if handle_event(fake_event, app, perms).await? {
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
        BgOp::SessionUpdated { session_id } => {
            app.refresh_sessions();
            let viewing_session = app.session_detail.as_ref().map(|d| d.id.clone());
            if viewing_session.as_deref() == Some(&session_id) {
                let db = app.data_dir.join("sessions.json");
                app.session_detail = sisyphus_tools::session_get(&db, &session_id).ok();
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

// ── Panel helpers ────────────────────────────────────────

/// Cycle active panel forward (+1) or backward (-1), wrapping around.
fn cycle_panel(app: &mut App, direction: i8) {
    let panels = Panel::all();
    let pos = panels
        .iter()
        .position(|p| *p == app.active_panel)
        .unwrap_or(0);
    let len = panels.len();
    let next = match direction {
        1 => (pos + 1) % len,
        -1 => (pos + len - 1) % len,
        _ => pos,
    };
    app.active_panel = panels[next];
    app.mode = match app.active_panel {
        Panel::Shell => Mode::Input,
        Panel::Lsp => Mode::Select,
        _ => Mode::Select,
    };
    app.session_mode = Mode::Select;
    app.boulder_dirty = true;
    // Eagerly refresh data so panel renders with content immediately
    match app.active_panel {
        Panel::Todos => app.refresh_todos(),
        Panel::Sessions => app.refresh_sessions(),
        _ => {}
    }
}

// ── Event handler ───────────────────────────────────────

async fn handle_event(event: Event, app: &mut App, perms: &CliPermission) -> Result<bool> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            // Global: Ctrl+Q → quick exit (double-tap to confirm)
            if key.code == KeyCode::Char('q')
                && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
            {
                if app.exit_requested {
                    return Ok(true);
                }
                app.exit_requested = true;
                app.add_log(LogEntry::Warn("Press Ctrl+Q again to confirm exit.".into()));
                return Ok(false);
            }

            // Global: Ctrl+P → toggle command palette
            if key.code == KeyCode::Char('p')
                && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
            {
                app.palette_open = !app.palette_open;
                if app.palette_open {
                    app.palette_query.clear();
                    app.palette_sel = 0;
                }
                return Ok(false);
            }

            // Global: Ctrl+V → paste from clipboard into active input field
            if key.code == KeyCode::Char('v')
                && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
            {
                let pasted = paste_from_clipboard();
                if !pasted.is_empty() {
                    match app.active_panel {
                        Panel::Shell if app.mode == Mode::Input => {
                            app.input.insert_str(app.cursor_pos, &pasted);
                            app.cursor_pos += pasted.len();
                        }
                        Panel::Sessions if app.session_detail.is_some() && app.session_mode == Mode::Input => {
                            app.session_input.push_str(&pasted);
                        }
                        _ => {
                            app.add_log(LogEntry::Warn("Paste: switch to an input field first".into()));
                        }
                    }
                }
                return Ok(false);
            }

            // Leader key: Ctrl+X enters leader mode, next key is a command
            if app.leader_mode {
                match key.code {
                    KeyCode::Char('h') => {
                        app.leader_mode = false;
                        app.active_panel = Panel::Shell;
                        app.mode = Mode::Input;
                        app.input = ":help".into();
                        app.cursor_pos = 5;
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
                        // Pass through to normal handling
                    }
                }
            }

            if key.code == KeyCode::Char('x')
                && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
                && !app.leader_mode
            {
                app.leader_mode = true;
                return Ok(false);
            }

            // When palette is open, route all keys to palette handler
            if app.palette_open {
                handle_palette_key(key.code, app);
                return Ok(false);
            }
            // Panel switching (global)
                match key.code {
                    KeyCode::Char('1')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        app.active_panel = Panel::Shell;
                        app.mode = Mode::Input;
                        app.boulder_dirty = true;
                        return Ok(false);
                    }
                    KeyCode::Char('2')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        app.active_panel = Panel::Todos;
                        app.mode = Mode::Select;
                        app.refresh_todos();
                        app.boulder_dirty = true;
                        return Ok(false);
                    }
                    KeyCode::Char('3')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        app.active_panel = Panel::Sessions;
                        app.mode = Mode::Select;
                        app.session_detail = None;
                        app.refresh_sessions();
                        app.boulder_dirty = true;
                        return Ok(false);
                    }
                    KeyCode::Char('4')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        app.active_panel = Panel::Lsp;
                        app.mode = Mode::Select;
                        app.boulder_dirty = true;
                        return Ok(false);
                    }
                    KeyCode::Char('5')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        app.active_panel = Panel::Sop;
                        app.mode = Mode::Select;
                        app.sop_status = "connecting...".into();
                        app.sop_result = None;
                        app.boulder_dirty = true;
                        if app.pending_op.is_none() {
                            app.pending_op = Some(spawn_list_sops(app.bridge_address.clone()));
                        }
                        return Ok(false);
                    }
                    KeyCode::Char('6')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        app.active_panel = Panel::Roadmap;
                        app.mode = Mode::Select;
                        app.roadmap_status = "connecting...".into();
                        app.boulder_dirty = true;
                        if app.pending_op.is_none() {
                            app.pending_op = Some(spawn_fetch_roadmap(app.bridge_address.clone()));
                        }
                        return Ok(false);
                    }

                // Ctrl+Tab / Ctrl+Shift+Tab → cycle panels
                KeyCode::Tab
                    if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
                        && !key.modifiers.contains(crossterm::event::KeyModifiers::SHIFT) =>
                {
                    cycle_panel(app, 1);
                    return Ok(false);
                }
                KeyCode::BackTab | KeyCode::Tab
                    if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
                        && key.modifiers.contains(crossterm::event::KeyModifiers::SHIFT) =>
                {
                    cycle_panel(app, -1);
                    return Ok(false);
                }
                _ => {}
            }

            // Fallback: digits 1-6 switch panels when not in Input mode,
            // or in Input mode when the input buffer is empty.
            // This handles terminals that don't send Ctrl modifier for Ctrl+digit
            let can_fallback = app.mode != Mode::Input || app.input.is_empty();
            if can_fallback {
                match key.code {
                    KeyCode::Char('1') => {
                        app.active_panel = Panel::Shell;
                        app.mode = Mode::Input;
                        app.boulder_dirty = true;
                        return Ok(false);
                    }
                    KeyCode::Char('2') => {
                        app.active_panel = Panel::Todos;
                        app.mode = Mode::Select;
                        app.refresh_todos();
                        app.boulder_dirty = true;
                        return Ok(false);
                    }
                    KeyCode::Char('3') => {
                        app.active_panel = Panel::Sessions;
                        app.mode = Mode::Select;
                        app.session_detail = None;
                        app.refresh_sessions();
                        app.boulder_dirty = true;
                        return Ok(false);
                    }
                    KeyCode::Char('4') => {
                        app.active_panel = Panel::Lsp;
                        app.mode = Mode::Select;
                        app.boulder_dirty = true;
                        return Ok(false);
                    }
                    KeyCode::Char('5') => {
                        app.active_panel = Panel::Sop;
                        app.mode = Mode::Select;
                        app.sop_status = "connecting...".into();
                        app.sop_result = None;
                        app.boulder_dirty = true;
                        if app.pending_op.is_none() {
                            app.pending_op = Some(spawn_list_sops(app.bridge_address.clone()));
                        }
                        return Ok(false);
                    }
                    KeyCode::Char('6') => {
                        app.active_panel = Panel::Roadmap;
                        app.mode = Mode::Select;
                        app.roadmap_status = "connecting...".into();
                        app.boulder_dirty = true;
                        if app.pending_op.is_none() {
                            app.pending_op = Some(spawn_fetch_roadmap(app.bridge_address.clone()));
                        }
                        return Ok(false);
                    }
                    _ => {}
                }
            }

            match app.active_panel {
                Panel::Shell => handle_shell_key(key.code, app, perms).await,
                Panel::Todos => handle_todos_key(key.code, app),
                Panel::Sessions => handle_sessions_key(key.code, app),
                Panel::Lsp => handle_lsp_key(key.code, app).await,
                Panel::Sop => handle_sop_key(key.code, app).await,
                Panel::Roadmap => handle_roadmap_key(key.code, app).await,
            }

            // Check if a handler requested immediate exit
            if app.exit_immediately {
                return Ok(true);
            }
        }
        Event::Mouse(m) => {
            match m.kind {
                // Click on tab bar to switch panels directly
                MouseEventKind::Down(MouseButton::Left) if m.row == 0 && !app.palette_open => {
                    if let Some(panel) = tab_at_x(app, m.column) {
                        app.active_panel = panel;
                        app.mode = match panel {
                            Panel::Shell => Mode::Input,
                            Panel::Lsp => Mode::Select,
                            _ => Mode::Select,
                        };
                        app.session_mode = Mode::Select;
                        app.boulder_dirty = true;
                        match panel {
                            Panel::Todos => app.refresh_todos(),
                            Panel::Sessions => app.refresh_sessions(),
                            _ => {}
                        }
                    }
                }
                MouseEventKind::ScrollDown => {
                    match app.active_panel {
                        Panel::Shell => {
                            app.scroll = (app.scroll + 3).min(app.log.len().saturating_sub(1));
                            app.boulder_dirty = true;
                        }
                        Panel::Sessions if app.session_detail.is_some() => {
                            app.session_detail_scroll = app.session_detail_scroll.saturating_add(3);
                            app.boulder_dirty = true;
                        }
                        Panel::Sessions => {
                            app.session_sel = (app.session_sel + 3).min(app.filtered_sessions().len().saturating_sub(1));
                            app.boulder_dirty = true;
                        }
                        Panel::Todos => {
                            app.todo_sel = (app.todo_sel + 3).min(app.todo_items.len().saturating_sub(1));
                            app.boulder_dirty = true;
                        }
                        Panel::Lsp => {
                            app.lsp_scroll = (app.lsp_scroll + 3).min(app.lsp_diagnostics.len().saturating_sub(1));
                            app.boulder_dirty = true;
                        }
                        Panel::Sop => {
                            let cnt = app.filtered_sops().len();
                            app.sop_sel = (app.sop_sel + 3).min(cnt.saturating_sub(1));
                            app.boulder_dirty = true;
                        }
                        Panel::Roadmap => {
                            app.roadmap_scroll = app.roadmap_scroll.saturating_add(3);
                            app.boulder_dirty = true;
                        }
                    }
                }
                MouseEventKind::ScrollUp => {
                    match app.active_panel {
                        Panel::Shell => {
                            app.scroll = app.scroll.saturating_sub(3);
                            app.boulder_dirty = true;
                        }
                        Panel::Sessions if app.session_detail.is_some() => {
                            app.session_detail_scroll = app.session_detail_scroll.saturating_sub(3);
                            app.boulder_dirty = true;
                        }
                        Panel::Sessions => {
                            app.session_sel = app.session_sel.saturating_sub(3);
                            app.boulder_dirty = true;
                        }
                        Panel::Todos => {
                            app.todo_sel = app.todo_sel.saturating_sub(3);
                            app.boulder_dirty = true;
                        }
                        Panel::Lsp => {
                            app.lsp_scroll = app.lsp_scroll.saturating_sub(3);
                            app.boulder_dirty = true;
                        }
                        Panel::Sop => {
                            app.sop_sel = app.sop_sel.saturating_sub(3);
                            app.boulder_dirty = true;
                        }
                        Panel::Roadmap => {
                            app.roadmap_scroll = app.roadmap_scroll.saturating_sub(3);
                            app.boulder_dirty = true;
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


fn handle_palette_key(code: KeyCode, app: &mut App) {
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
                    ":help" => {
                        app.active_panel = Panel::Shell;
                        app.input = ":help".into();
                        app.cursor_pos = 5;
                        app.mode = Mode::Input;
                        app.boulder_dirty = true;
                    }
                    ":panel shell" => {
                        app.active_panel = Panel::Shell;
                        app.boulder_dirty = true;
                    }
                    ":panel todos" => {
                        app.active_panel = Panel::Todos;
                        app.boulder_dirty = true;
                    }
                    ":panel sessions" => {
                        app.active_panel = Panel::Sessions;
                        app.boulder_dirty = true;
                    }
                    ":panel lsp" => {
                        app.active_panel = Panel::Lsp;
                        app.boulder_dirty = true;
                    }
                    ":panel sop" => {
                        app.active_panel = Panel::Sop;
                        app.boulder_dirty = true;
                    }
                    ":panel roadmap" => {
                        app.active_panel = Panel::Roadmap;
                        app.boulder_dirty = true;
                    }
                    _ => {

                    if entry.command == "clear" {
                        app.log.clear();
                        app.scroll = 0;
                    } else if entry.command == "exit" {
                        app.exit_requested = true;
                        app.add_log(LogEntry::Warn("Type `exit` again to confirm, or `clear` to dismiss.".into()));
                    } else if entry.command.starts_with(":bridge") {
                        app.active_panel = Panel::Shell;
                        app.input = entry.command.to_string();
                        app.cursor_pos = app.input.len();
                        app.mode = Mode::Input;
                    } else if entry.command.starts_with(":refresh") {
                        app.session_dirty = true;
                        app.todo_dirty = true;
                    }
                }
            }
            app.mode = Mode::Input;
        }
        _ => {}
    }
}


// ── Status bar ──────────────────────────────────────────

fn ui(frame: &mut ratatui::Frame, app: &App) {
    if app.palette_open {
        //render_command_palette(frame, frame.area(), app);
        return;
    }
    let [_tabs_area, content_area, status_area] = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Length(2),
        ratatui::layout::Constraint::Min(0),
        ratatui::layout::Constraint::Length(1),
    ])
    .areas(frame.area());
    //render_tabs(frame, tabs_area, app);
    match app.active_panel {
        Panel::Shell => render_shell(frame, content_area, app),
        Panel::Todos => render_todos(frame, content_area, app),
        Panel::Sessions => render_sessions(frame, content_area, app),
        Panel::Lsp => render_lsp(frame, content_area, app),
        Panel::Sop => render_sop(frame, content_area, app),
        Panel::Roadmap => render_roadmap(frame, content_area, app),
    }
    frame.render_widget(ratatui::widgets::Paragraph::new(render_status(app)), status_area);
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

    let help = match app.active_panel {
        Panel::Shell => match app.mode {
            Mode::Input => "Enter exec | ↑↓ history | Tab complete | Esc clear | Ctrl+V paste",
            Mode::Scroll => "↑↓ scroll | Tab/Esc input",
            Mode::Select => "",
        },
        Panel::Todos => {
            if !app.todo_adding.is_empty() {
                "Enter add | Esc cancel"
            } else {
                "↑↓ select | Space toggle | a add | d delete | r refresh"
            }
        }
        Panel::Sessions => {
            if app.rename_session_id.is_some() {
                "Enter rename | Esc/q cancel"
            } else if app.session_detail.is_some() {
                "↑↓/PgUp/PgDn scroll | Home top | End bottom | y copy | Esc/q back"
            } else {
                "↑↓ select | Enter detail | r refresh | c create | Esc/q clear | type filter"
            }
        }
        Panel::Lsp => match app.mode {
            Mode::Input => "Enter submit | Esc cancel",
            Mode::Scroll | Mode::Select => {
                "f path | l lang | s server | 1 Rust | 2 TS | 3 Python | Enter run"
            }
        },
        Panel::Sop => {
            if !app.sop_query.is_empty() {
                "type filter | Esc clear | e/Enter execute | r refresh"
            } else {
                "type filter | e/Enter execute | r refresh"
            }
        }
        Panel::Roadmap => "r refresh | p projection | b bottlenecks",
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
            " ctrl+? help ",
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
    spans.push(Span::styled(
        app.model_name.clone(),
        Style::default().fg(theme.muted),
    ));

    Line::from(spans)
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

    // ── Panel ─────────────────────────────────────────────

    #[test]
    fn test_panel_all_returns_six() {
        assert_eq!(Panel::all().len(), 6);
    }

    #[test]
    fn test_panel_names() {
        assert_eq!(Panel::Shell.name(), "Shell");
        assert_eq!(Panel::Todos.name(), "Todos");
        assert_eq!(Panel::Sessions.name(), "Sessions");
        assert_eq!(Panel::Lsp.name(), "LSP");
        assert_eq!(Panel::Sop.name(), "SOP");
        assert_eq!(Panel::Roadmap.name(), "Roadmap");
    }

    #[test]
    fn test_panel_all_unique() {
        let all = Panel::all();
        let mut dedup = all.to_vec();
        dedup.dedup();
        assert_eq!(all.len(), dedup.len());
    }

    // ── Mode ──────────────────────────────────────────────

    #[test]
    fn test_mode_equality() {
        assert_eq!(Mode::Input, Mode::Input);
        assert_eq!(Mode::Scroll, Mode::Scroll);
        assert_eq!(Mode::Select, Mode::Select);
        assert_ne!(Mode::Input, Mode::Scroll);
    }

    // ── App::new ──────────────────────────────────────────

    #[test]
    fn test_app_new_initial_state() {
        let app = App::new(PathBuf::from("/tmp/test"), "localhost:50051".into());
        assert_eq!(app.active_panel, Panel::Sessions);
        assert_eq!(app.mode, Mode::Select);
        assert!(app.input.is_empty());
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
        assert!(app.lsp_input.is_empty());
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
        assert!(rendered.contains("ctrl+? help"));
        assert!(rendered.contains("Context: 0%"));
        assert!(rendered.contains("unknown"));
    }

    #[test]
    fn test_render_status_shell_help() {
        let mut app = App::new(PathBuf::from("/tmp"), "addr".into());
        app.active_panel = Panel::Shell;
        app.mode = Mode::Input;
        let rendered = render_status(&app).to_string();
        assert!(rendered.contains("Tab complete"));

        app.mode = Mode::Scroll;
        let rendered = render_status(&app).to_string();
        assert!(rendered.contains("↑↓ scroll"));
    }

    #[test]
    fn test_render_status_todos_help() {
        let mut app = App::new(PathBuf::from("/tmp"), "addr".into());
        app.active_panel = Panel::Todos;
        let rendered = render_status(&app).to_string();
        assert!(rendered.contains("Space toggle"));
    }

    #[test]
    fn test_render_status_sessions_help() {
        let mut app = App::new(PathBuf::from("/tmp"), "addr".into());
        app.active_panel = Panel::Sessions;
        let rendered = render_status(&app).to_string();
        assert!(rendered.contains("Enter detail"));
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
