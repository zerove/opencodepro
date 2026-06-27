use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    DefaultTerminal,
};

use crate::bridge::BridgeClient;
use crate::permission::CliPermission;
use crate::shell;

// ── Log entries ──────────────────────────────────────────

#[derive(Debug, Clone)]
enum LogEntry {
    Info(String),
    Success(String),
    Warn(String),
    Error(String),
}

impl LogEntry {
    fn entry_text(&self) -> &str {
        match self {
            LogEntry::Info(t) | LogEntry::Success(t) | LogEntry::Warn(t) | LogEntry::Error(t) => {
                t.as_str()
            }
        }
    }
}

// ── Panels ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Panel {
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

// ── App state ────────────────────────────────────────────

struct App {
    // sync
    data_dir: PathBuf,
    bridge_address: String,

    // panel
    active_panel: Panel,
    mode: Mode,

    // shell
    input: String,
    log: Vec<LogEntry>,
    scroll: usize,
    history: Vec<String>,
    history_idx: Option<usize>,
    exit_requested: bool,

    // todos
    todo_items: Vec<sisyphus_tools::TodoItem>,
    todo_sel: usize,
    todo_dirty: bool,

    // sessions
    sessions: Vec<sisyphus_tools::Session>,
    session_sel: usize,
    session_detail: Option<sisyphus_tools::Session>,
    session_dirty: bool,

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

    // roadmap panel
    roadmap_projection: Option<String>,
    roadmap_bottlenecks: Option<String>,
    roadmap_status: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Input,
    Scroll,
    Select,
}

impl App {
    fn new(data_dir: PathBuf, bridge_address: String) -> Self {
        Self {
            data_dir,
            bridge_address,
            active_panel: Panel::Shell,
            mode: Mode::Input,
            input: String::new(),
            log: vec![LogEntry::Info("Sisyphus CLI — Ctrl-1 Shell | Ctrl-2 Todos | Ctrl-3 Sessions | Ctrl-4 LSP | Ctrl-5 SOP | Ctrl-6 Roadmap".into())],
            scroll: 0,
            history: vec![],
            history_idx: None,
            todo_items: vec![],
            todo_sel: 0,
            todo_dirty: true,
            sessions: vec![],
            session_sel: 0,
            session_detail: None,
            session_dirty: true,
            lsp_input: String::new(),
            lsp_input_mode: LspInputMode::FilePath,
            lsp_language: "rust".into(),
            lsp_server: "rust-analyzer".into(),
            lsp_server_args: String::new(),
            lsp_diagnostics: vec![],
            lsp_status: "idle".into(),
            lsp_scroll: 0,
            lsp_last_error: String::new(),
            sop_sops: vec![],
            sop_sel: 0,
            sop_status: "disconnected".into(),
            sop_result: None,
            roadmap_projection: None,
            roadmap_bottlenecks: None,
            roadmap_status: "disconnected".into(),
            exit_requested: false,
        }
    }

    // ── shell ────────────────────────────────────
    fn add_log(&mut self, entry: LogEntry) {
        self.log.push(entry);
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
        let db = self.data_dir.join("todos.json");
        let id = self.todo_items[self.todo_sel].id.clone();
        if sisyphus_tools::todo_delete(&db, &id).is_ok() {
            self.todo_dirty = true;
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

    fn select_session(&mut self) {
        if self.sessions.is_empty() {
            return;
        }
        let db = self.data_dir.join("sessions.json");
        let id = &self.sessions[self.session_sel].id;
        self.session_detail = sisyphus_tools::session_get(&db, id).ok();
    }
}

// ── Public entry ────────────────────────────────────────

pub async fn run(data_dir: PathBuf, bridge_address: String) -> Result<()> {
    let mut terminal = ratatui::init();
    terminal.clear()?;

    let perms = CliPermission::load(None).unwrap_or_default();
    let mut app = App::new(data_dir, bridge_address);

    let tick_rate = Duration::from_millis(100);
    let res = run_loop(&mut terminal, &mut app, &perms, tick_rate).await;

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
        terminal.draw(|f| ui(f, app))?;

        if event::poll(tick_rate)? {
            let event = event::read()?;
            if handle_event(event, app, perms).await? {
                break;
            }
        }
    }
    Ok(())
}

// ── Event handler ───────────────────────────────────────

async fn handle_event(event: Event, app: &mut App, perms: &CliPermission) -> Result<bool> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            // Panel switching (global)
            match key.code {
                KeyCode::Char('1')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    app.active_panel = Panel::Shell;
                    app.mode = Mode::Input;
                    return Ok(false);
                }
                KeyCode::Char('2')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    app.active_panel = Panel::Todos;
                    app.mode = Mode::Select;
                    app.todo_dirty = true;
                    return Ok(false);
                }
                KeyCode::Char('3')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    app.active_panel = Panel::Sessions;
                    app.mode = Mode::Select;
                    app.session_dirty = true;
                    app.session_detail = None;
                    return Ok(false);
                }
                KeyCode::Char('4')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    app.active_panel = Panel::Lsp;
                    app.mode = Mode::Input;
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
                    // Auto-refresh SOP list
                    let result = fetch_sops(app).await;
                    match result {
                        Ok(count) => app.sop_status = format!("connected ({} SOPs)", count),
                        Err(e) => app.sop_status = format!("connect failed: {e}"),
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
                    // Auto-refresh roadmap data
                    let result = fetch_roadmap_data(app).await;
                    match result {
                        Ok(_) => app.roadmap_status = "connected".into(),
                        Err(e) => app.roadmap_status = format!("connect failed: {e}"),
                    }
                    return Ok(false);
                }
                _ => {}
            }

            match app.active_panel {
                Panel::Shell => handle_shell_key(key.code, app, perms).await,
                Panel::Todos => handle_todos_key(key.code, app),
                Panel::Sessions => handle_sessions_key(key.code, app),
                Panel::Lsp => handle_lsp_key(key.code, app).await,
                Panel::Sop => handle_sop_key(key.code, app).await,
                Panel::Roadmap => handle_roadmap_key(key.code, app).await,
            }
        }
        Event::Resize(_, _) => {}
        _ => {}
    }
    Ok(false)
}

async fn handle_shell_key(code: KeyCode, app: &mut App, perms: &CliPermission) {
    match app.mode {
        Mode::Input => match code {
            KeyCode::Enter => {
                let cmd = app.input.trim().to_string();
                if cmd.is_empty() {
                    return;
                }
                if app.history.last().is_none_or(|last| last != &cmd) {
                    app.history.push(cmd.clone());
                }
                app.history_idx = None;
                app.input.clear();

                match cmd.as_str() {
                    "exit" | "quit" | ":q" => {
                        if app.exit_requested {
                            std::process::exit(0);
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
                    ":help" | ":h" => {
                        app.add_log(LogEntry::Info("── Available commands ──".into()));
                        app.add_log(LogEntry::Info(
                            "  exit / quit / :q    Exit (type twice)".into(),
                        ));
                        app.add_log(LogEntry::Info("  clear / :c          Clear log".into()));
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
                        app.add_log(LogEntry::Info("  ------------".into()));
                        app.add_log(LogEntry::Info("  ↑/↓    Browse command history".into()));
                        app.add_log(LogEntry::Info("  Tab    Toggle scroll/input mode".into()));
                        app.add_log(LogEntry::Info(
                            "  Any other text runs as a shell command".into(),
                        ));
                        return;
                    }
                    // ── Bridge / SOP commands ─────────────────────
                    _ if cmd.starts_with(":bridge ") || cmd == ":bridge" => {
                        let parts: Vec<&str> = cmd.splitn(3, ' ').collect();
                        let sub = parts.get(1).copied().unwrap_or("status");
                        app.add_log(LogEntry::Info(format!("🔗 Bridge: {sub}...")));
                        match BridgeClient::connect(&app.bridge_address).await {
                            Ok(mut client) => match sub {
                                "status" | "health" => match client.health_check().await {
                                    Ok(resp) => app.add_log(LogEntry::Success(format!(
                                        "Bridge: {} (v{})",
                                        resp.status, resp.version
                                    ))),
                                    Err(e) => app.add_log(LogEntry::Error(format!(
                                        "Health check failed: {e}"
                                    ))),
                                },
                                "sops" | "list" => match client.list_sops().await {
                                    Ok(sops) if sops.is_empty() => {
                                        app.add_log(LogEntry::Info("No SOPs registered.".into()));
                                    }
                                    Ok(sops) => {
                                        for sop in &sops {
                                            app.add_log(LogEntry::Success(format!(
                                                "  {} ({} nodes)",
                                                sop.sop_id, sop.node_count
                                            )));
                                        }
                                    }
                                    Err(e) => app
                                        .add_log(LogEntry::Error(format!("List SOPs failed: {e}"))),
                                },
                                "run" => {
                                    let sop_id = parts.get(2).unwrap_or(&"");
                                    if sop_id.is_empty() {
                                        app.add_log(LogEntry::Error(
                                            "Usage: :bridge run <sop_id>".into(),
                                        ));
                                    } else {
                                        match client.execute_sop(sop_id).await {
                                            Ok(result) => app.add_log(LogEntry::Success(result)),
                                            Err(e) => app.add_log(LogEntry::Error(format!(
                                                "SOP execution failed: {e}"
                                            ))),
                                        }
                                    }
                                }
                                other => app.add_log(LogEntry::Error(format!(
                                    "Unknown bridge command: {other} (try: status, sops, run)"
                                ))),
                            },
                            Err(e) => app.add_log(LogEntry::Error(format!(
                                "Failed to connect to bridge at {}: {e}",
                                app.bridge_address
                            ))),
                        }
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
                    Some(0) => None, // already at oldest
                    Some(i) => Some(i - 1),
                };
                app.history_idx = new_idx;
                app.input = new_idx.map(|i| app.history[i].clone()).unwrap_or_default();
            }
            KeyCode::Down => {
                if app.history.is_empty() {
                    return;
                }
                let new_idx = match app.history_idx {
                    None => None,                                  // already at newest
                    Some(i) if i + 1 >= app.history.len() => None, // move past newest
                    Some(i) => Some(i + 1),
                };
                app.history_idx = new_idx;
                app.input = new_idx.map(|i| app.history[i].clone()).unwrap_or_default();
            }
            KeyCode::Char(c) => {
                app.history_idx = None;
                app.input.push(c);
            }
            KeyCode::Backspace => {
                app.input.pop();
            }
            KeyCode::Esc => app.input.clear(),
            KeyCode::Tab => {
                if app.input.is_empty() {
                    app.mode = Mode::Scroll;
                } else {
                    // Tab completion: cycle through matching special commands
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
                    if let Some(m) = completions.first() {
                        app.input = m.to_string();
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
            KeyCode::Tab | KeyCode::Esc => app.mode = Mode::Input,
            _ => {}
        },
        Mode::Select => {}
    }
}

fn handle_todos_key(code: KeyCode, app: &mut App) {
    app.refresh_todos();
    match code {
        KeyCode::Up => app.todo_sel = app.todo_sel.saturating_sub(1),
        KeyCode::Down => {
            app.todo_sel = (app.todo_sel + 1).min(app.todo_items.len().saturating_sub(1))
        }
        KeyCode::Char(' ') | KeyCode::Enter => app.toggle_todo_status(),
        KeyCode::Char('d') | KeyCode::Delete => app.delete_todo(),
        KeyCode::Char('r') => app.todo_dirty = true,
        _ => {}
    }
}

fn handle_sessions_key(code: KeyCode, app: &mut App) {
    app.refresh_sessions();
    if app.session_detail.is_some() {
        // viewing detail → Esc goes back
        if let KeyCode::Esc | KeyCode::Char('q') = code {
            app.session_detail = None;
        }
        return;
    }
    match code {
        KeyCode::Up => app.session_sel = app.session_sel.saturating_sub(1),
        KeyCode::Down => {
            app.session_sel = (app.session_sel + 1).min(app.sessions.len().saturating_sub(1))
        }
        KeyCode::Enter => app.select_session(),
        KeyCode::Char('r') => app.session_dirty = true,
        _ => {}
    }
}

/// Try to connect to the bridge and fetch the SOP list.
async fn fetch_sops(app: &mut App) -> Result<usize, String> {
    app.sop_sops.clear();
    app.sop_sel = 0;
    let mut client = crate::bridge::BridgeClient::connect(&app.bridge_address)
        .await
        .map_err(|e| format!("connect: {e}"))?;
    let sops = client.list_sops().await.map_err(|e| format!("list: {e}"))?;
    let count = sops.len();
    app.sop_sops = sops;
    Ok(count)
}

async fn fetch_roadmap_data(app: &mut App) -> Result<(), String> {
    let mut client = crate::bridge::BridgeClient::connect(&app.bridge_address)
        .await
        .map_err(|e| format!("connect: {e}"))?;

    let projection = client
        .execute_tool("strategic:project_costs", "")
        .await
        .map_err(|e| format!("projection error: {e}"))?;
    let bottlenecks = client
        .execute_tool("strategic:bottlenecks", "")
        .await
        .map_err(|e| format!("bottleneck error: {e}"))?;

    app.roadmap_projection = Some(projection.result_json);
    app.roadmap_bottlenecks = Some(bottlenecks.result_json);
    Ok(())
}

async fn handle_sop_key(code: KeyCode, app: &mut App) {
    match code {
        KeyCode::Char('r') | KeyCode::Char('R') => {
            app.sop_status = "connecting...".into();
            app.sop_result = None;
            match fetch_sops(app).await {
                Ok(count) => app.sop_status = format!("connected ({} SOPs)", count),
                Err(e) => app.sop_status = format!("failed: {e}"),
            }
        }
        KeyCode::Char('e') | KeyCode::Enter => {
            if app.sop_sops.is_empty() || app.sop_sel >= app.sop_sops.len() {
                return;
            }
            let sop_id = &app.sop_sops[app.sop_sel].sop_id;
            app.sop_status = format!("executing {sop_id}...");
            app.sop_result = None;
            match crate::bridge::BridgeClient::connect(&app.bridge_address).await {
                Ok(mut client) => match client.execute_sop(sop_id).await {
                    Ok(result) => {
                        app.sop_result = Some(result.clone());
                        app.sop_status = format!("executed {sop_id}");
                    }
                    Err(e) => {
                        app.sop_status = format!("execute failed: {e}");
                    }
                },
                Err(e) => {
                    app.sop_status = format!("connect failed: {e}");
                }
            }
        }
        KeyCode::Up => {
            app.sop_sel = app.sop_sel.saturating_sub(1);
        }
        KeyCode::Down if !app.sop_sops.is_empty() => {
            app.sop_sel = (app.sop_sel + 1).min(app.sop_sops.len().saturating_sub(1));
        }
        _ => {}
    }
}

async fn handle_roadmap_key(code: KeyCode, app: &mut App) {
    match code {
        KeyCode::Char('r') | KeyCode::Char('R') => {
            app.roadmap_status = "connecting...".into();
            match fetch_roadmap_data(app).await {
                Ok(_) => app.roadmap_status = "connected".into(),
                Err(e) => app.roadmap_status = format!("failed: {e}"),
            }
        }
        _ => {}
    }
}

async fn handle_lsp_key(code: KeyCode, app: &mut App) {
    match code {
        KeyCode::Char('f') if app.mode == Mode::Select => {
            app.lsp_input_mode = LspInputMode::FilePath;
            app.mode = Mode::Input;
            app.lsp_input.clear();
        }
        KeyCode::Char('l') if app.mode == Mode::Select => {
            app.lsp_input_mode = LspInputMode::Language;
            app.mode = Mode::Input;
            app.lsp_input.clear();
        }
        KeyCode::Char('s') if app.mode == Mode::Select => {
            app.lsp_input_mode = LspInputMode::Server;
            app.mode = Mode::Input;
            app.lsp_input.clear();
        }
        KeyCode::Char('a') if app.mode == Mode::Select => {
            app.lsp_input_mode = LspInputMode::ServerArgs;
            app.mode = Mode::Input;
            app.lsp_input.clear();
        }
        // FilePath: run diagnostics
        KeyCode::Enter
            if app.mode == Mode::Input && app.lsp_input_mode == LspInputMode::FilePath =>
        {
            let path = app.lsp_input.trim().to_string();
            if path.is_empty() {
                return;
            }
            app.lsp_status = "running".into();
            app.lsp_last_error.clear();
            app.lsp_diagnostics.clear();
            let file_path = std::path::PathBuf::from(&path);
            let lang = app.lsp_language.clone();
            let server = app.lsp_server.clone();
            let server_args: Vec<String> = if app.lsp_server_args.is_empty() {
                vec![]
            } else {
                app.lsp_server_args
                    .split(' ')
                    .map(|s| s.to_string())
                    .collect()
            };
            match crate::lsp::run_diagnostics_raw(
                &file_path,
                &lang,
                &server,
                &server_args,
                "file:///",
            )
            .await
            {
                Ok(diags) => {
                    let is_empty = diags.is_empty();
                    app.lsp_diagnostics = diags;
                    app.lsp_status = if is_empty {
                        "clean".into()
                    } else {
                        "issues".into()
                    };
                    app.lsp_scroll = 0;
                }
                Err(e) => {
                    let err_msg = format!("{e}");
                    app.lsp_last_error = err_msg.clone();
                    app.lsp_status = "error".into();
                }
            }
            app.mode = Mode::Select;
        }
        // Language: apply and return to file path mode
        KeyCode::Enter
            if app.mode == Mode::Input && app.lsp_input_mode == LspInputMode::Language =>
        {
            let input = app.lsp_input.trim().to_string();
            if !input.is_empty() {
                app.lsp_language = input;
            }
            app.lsp_input.clear();
            app.lsp_input_mode = LspInputMode::FilePath;
            app.mode = Mode::Input;
        }
        // Server: apply and return to file path mode
        KeyCode::Enter if app.mode == Mode::Input && app.lsp_input_mode == LspInputMode::Server => {
            let input = app.lsp_input.trim().to_string();
            if !input.is_empty() {
                app.lsp_server = input;
            }
            app.lsp_input.clear();
            app.lsp_input_mode = LspInputMode::FilePath;
            app.mode = Mode::Input;
        }
        // ServerArgs: apply and return to file path mode
        KeyCode::Enter
            if app.mode == Mode::Input && app.lsp_input_mode == LspInputMode::ServerArgs =>
        {
            let input = app.lsp_input.trim().to_string();
            if !input.is_empty() {
                app.lsp_server_args = input;
            }
            app.lsp_input.clear();
            app.lsp_input_mode = LspInputMode::FilePath;
            app.mode = Mode::Input;
        }
        KeyCode::Up => {
            app.lsp_scroll = app.lsp_scroll.saturating_sub(1);
        }
        KeyCode::Down => {
            app.lsp_scroll = app.lsp_scroll.saturating_add(1);
        }
        KeyCode::Char(c) if app.mode == Mode::Input => app.lsp_input.push(c),
        KeyCode::Backspace if app.mode == Mode::Input => {
            app.lsp_input.pop();
        }
        KeyCode::Esc if app.mode == Mode::Input => {
            app.lsp_input.clear();
            app.mode = Mode::Select;
        }
        _ => {}
    }
}

// ── UI ──────────────────────────────────────────────────

fn ui(frame: &mut ratatui::Frame, app: &App) {
    let area = frame.area();
    let [main_area, status_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);

    // Tab bar
    render_tabs(frame, main_area, app);

    // Status bar
    let status = render_status(app);
    frame.render_widget(Paragraph::new(status), status_area);
}

fn render_tabs(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let [tab_bar, content] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);

    // Tab bar
    let tabs: Vec<Span> = Panel::all()
        .iter()
        .map(|p| {
            let active = *p == app.active_panel;
            let label = format!(" {} ", p.name());
            if active {
                Span::styled(label, Style::default().fg(Color::Black).bg(Color::Cyan))
            } else {
                Span::styled(label, Style::default().fg(Color::DarkGray))
            }
        })
        .collect();

    // separator spans
    let mut tab_line = Vec::new();
    for (i, span) in tabs.iter().enumerate() {
        if i > 0 {
            tab_line.push(Span::raw(" │ "));
        }
        tab_line.push(span.clone());
    }
    frame.render_widget(Paragraph::new(Line::from(tab_line)), tab_bar);

    // Content area
    match app.active_panel {
        Panel::Shell => render_shell(frame, content, app),
        Panel::Todos => render_todos(frame, content, app),
        Panel::Sessions => render_sessions(frame, content, app),
        Panel::Lsp => render_lsp(frame, content, app),
        Panel::Sop => render_sop(frame, content, app),
        Panel::Roadmap => render_roadmap(frame, content, app),
    }
}

// ── Shell panel ─────────────────────────────────────────

fn render_shell(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let [input_area, log_area] =
        Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(area);

    // Input
    let input_block = Block::default()
        .borders(Borders::ALL)
        .title(" Command ")
        .border_style(if app.mode == Mode::Input {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        });
    let input_paragraph = Paragraph::new(app.input.as_str())
        .block(input_block)
        .style(Style::default());
    frame.render_widget(input_paragraph, input_area);

    if app.mode == Mode::Input {
        frame.set_cursor_position((input_area.x + 1 + app.input.len() as u16, input_area.y + 1));
    }

    // Log
    let log_block = Block::default()
        .borders(Borders::ALL)
        .title(" Output ")
        .border_style(if app.mode == Mode::Scroll {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        });

    let items: Vec<ListItem> = app
        .log
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let style = match entry {
                LogEntry::Info(_) => Style::default().fg(Color::White),
                LogEntry::Success(_) => Style::default().fg(Color::Green),
                LogEntry::Warn(_) => Style::default().fg(Color::Yellow),
                LogEntry::Error(_) => Style::default().fg(Color::Red),
            };
            let text = entry.entry_text();
            let line = Line::from(vec![
                Span::styled(format!("{:>4} ", i), Style::default().fg(Color::DarkGray)),
                Span::styled(text, style),
            ]);
            ListItem::new(line)
        })
        .collect();

    let log_height = log_area.height.saturating_sub(2) as usize;
    let visible_start = app.scroll.saturating_sub(log_height.saturating_sub(1));
    let list = List::new(items)
        .block(log_block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(
        list,
        log_area,
        &mut ratatui::widgets::ListState::default().with_selected(Some(visible_start)),
    );
}

// ── Todos panel ─────────────────────────────────────────

fn render_todos(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Todos ")
        .border_style(Style::default().fg(Color::Cyan));

    if app.todo_items.is_empty() {
        let text = Text::from(Line::from(Span::styled(
            "No todos. Use `sisyphus todo add` in shell tab.",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(Paragraph::new(text).block(block), area);
        return;
    }

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
                    .fg(Color::Green)
                    .add_modifier(Modifier::DIM),
                sisyphus_tools::TodoStatus::Cancelled => {
                    Style::default().fg(Color::Red).add_modifier(Modifier::DIM)
                }
                sisyphus_tools::TodoStatus::InProgress => Style::default().fg(Color::Cyan),
                sisyphus_tools::TodoStatus::Pending => Style::default().fg(Color::White),
            };
            let line = Line::from(vec![
                Span::styled(format!("{} ", checkbox), style),
                Span::styled(&item.content, style),
                Span::styled(
                    format!(" ({})", item.priority),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(
        list,
        area,
        &mut ratatui::widgets::ListState::default().with_selected(Some(app.todo_sel)),
    );
}

// ── Sessions panel ──────────────────────────────────────

fn render_sessions(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Sessions ")
        .border_style(Style::default().fg(Color::Cyan));

    if app.sessions.is_empty() {
        let text = Text::from(Line::from(Span::styled(
            "No sessions. Use `sisyphus session create` in shell tab.",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(Paragraph::new(text).block(block), area);
        return;
    }

    // Detail view
    if let Some(ref detail) = app.session_detail {
        render_session_detail(frame, area, detail);
        return;
    }

    let items: Vec<ListItem> = app
        .sessions
        .iter()
        .map(|s| {
            let line = Line::from(vec![
                Span::raw("  "),
                Span::styled(&s.name, Style::default().fg(Color::White)),
                Span::styled(
                    format!(" ({} msgs)", s.message_count),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(
        list,
        area,
        &mut ratatui::widgets::ListState::default().with_selected(Some(app.session_sel)),
    );
}

fn render_session_detail(
    frame: &mut ratatui::Frame,
    area: Rect,
    session: &sisyphus_tools::Session,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Session: {} ", session.name))
        .border_style(Style::default().fg(Color::Cyan));

    let mut lines = vec![
        Line::from(Span::styled(
            format!("ID: {}  Messages: {}", session.id, session.message_count),
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::raw("")),
    ];

    for msg in &session.messages {
        let role_style = match msg.role.as_str() {
            "user" => Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            "assistant" => Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            _ => Style::default().fg(Color::DarkGray),
        };
        lines.push(Line::from(vec![
            Span::styled(format!("[{}] ", msg.role), role_style),
            Span::styled(&msg.content, Style::default().fg(Color::White)),
        ]));
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

// ── SOP panel ────────────────────────────────────────────

fn render_sop(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let [status_area, list_area, result_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(5),
    ])
    .areas(area);

    let status_style = match true {
        _ if app.sop_status.starts_with("connect") => Style::default().fg(Color::Red),
        _ if app.sop_status.starts_with("executing") => Style::default().fg(Color::Cyan),
        _ if app.sop_status.starts_with("executed") => Style::default().fg(Color::Green),
        _ if app.sop_status.starts_with("connected") => Style::default().fg(Color::Green),
        _ => Style::default().fg(Color::DarkGray),
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(&app.sop_status, status_style))),
        status_area,
    );

    let list_block = Block::default()
        .borders(Borders::ALL)
        .title(" Standard Operating Procedures ")
        .border_style(Style::default().fg(Color::Cyan));

    let items: Vec<ListItem> = if app.sop_sops.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "Press r to refresh / connect to bridge",
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        app.sop_sops
            .iter()
            .map(|sop| {
                let line = Line::from(vec![
                    Span::styled(&sop.sop_id, Style::default().fg(Color::White)),
                    Span::styled(
                        format!(" ({} nodes)", sop.node_count),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]);
                ListItem::new(line)
            })
            .collect()
    };

    let list = List::new(items)
        .block(list_block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(
        list,
        list_area,
        &mut ratatui::widgets::ListState::default().with_selected(Some(app.sop_sel)),
    );

    let result_block = Block::default()
        .borders(Borders::ALL)
        .title(" Result ")
        .border_style(Style::default().fg(Color::DarkGray));
    let result_text = match app.sop_result {
        Some(ref r) => Text::from(Line::from(Span::styled(
            r,
            Style::default().fg(Color::White),
        ))),
        None => Text::from(Line::from(Span::styled(
            "Select an SOP and press Enter to execute",
            Style::default().fg(Color::DarkGray),
        ))),
    };
    frame.render_widget(
        Paragraph::new(result_text)
            .block(result_block)
            .wrap(Wrap { trim: false }),
        result_area,
    );
}

// ── LSP Diagnostics panel ────────────────────────────────

fn render_roadmap(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let [status_area, content_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);

    let status_style = match app.roadmap_status.as_str() {
        "connected" => Style::default().fg(Color::Green),
        "connecting..." => Style::default().fg(Color::Cyan),
        _ => Style::default().fg(Color::Red),
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(&app.roadmap_status, status_style))),
        status_area,
    );

    let [proj_area, bot_area] =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)])
            .areas(content_area);

    let proj_block = Block::default()
        .borders(Borders::ALL)
        .title(" Roadmap Projections ")
        .border_style(Style::default().fg(Color::Cyan));

    let proj_text = match &app.roadmap_projection {
        Some(p) => Text::from(Line::from(Span::styled(
            p,
            Style::default().fg(Color::White),
        ))),
        None => Text::from(Line::from(Span::styled(
            "No projection data. Press r to refresh.",
            Style::default().fg(Color::DarkGray),
        ))),
    };
    frame.render_widget(
        Paragraph::new(proj_text)
            .block(proj_block)
            .wrap(Wrap { trim: false }),
        proj_area,
    );

    let bot_block = Block::default()
        .borders(Borders::ALL)
        .title(" Identified Bottlenecks ")
        .border_style(Style::default().fg(Color::Yellow));

    let bot_text = match &app.roadmap_bottlenecks {
        Some(b) => Text::from(Line::from(Span::styled(
            b,
            Style::default().fg(Color::White),
        ))),
        None => Text::from(Line::from(Span::styled(
            "No bottlenecks detected.",
            Style::default().fg(Color::DarkGray),
        ))),
    };
    frame.render_widget(
        Paragraph::new(bot_text)
            .block(bot_block)
            .wrap(Wrap { trim: false }),
        bot_area,
    );
}

// ── LSP Diagnostics panel ────────────────────────────────

fn render_lsp(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let [input_area, info_area, list_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(area);

    let field_label = match app.lsp_input_mode {
        LspInputMode::FilePath => " File Path ",
        LspInputMode::Language => " Language ID ",
        LspInputMode::Server => " Server Command ",
        LspInputMode::ServerArgs => " Server Args (space-separated) ",
    };
    let input_block = Block::default()
        .borders(Borders::ALL)
        .title(field_label)
        .border_style(if app.mode == Mode::Input {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        });
    let input_paragraph = Paragraph::new(app.lsp_input.as_str())
        .block(input_block)
        .style(Style::default());
    frame.render_widget(input_paragraph, input_area);

    if app.mode == Mode::Input && app.lsp_input_mode == LspInputMode::FilePath {
        frame.set_cursor_position((
            input_area.x + 1 + app.lsp_input.len() as u16,
            input_area.y + 1,
        ));
    }

    // Info line
    let info_text = if app.lsp_status == "error" && !app.lsp_last_error.is_empty() {
        format!(
            "Error: {} | Server: {} {}",
            app.lsp_last_error, app.lsp_server, app.lsp_server_args
        )
    } else {
        format!(
            "Status: {} | Lang: {} | Server: {} {}",
            app.lsp_status, app.lsp_language, app.lsp_server, app.lsp_server_args
        )
    };
    let info_style = match app.lsp_status.as_str() {
        "idle" | "clean" => Style::default().fg(Color::Green),
        "running" => Style::default().fg(Color::Cyan),
        "issues" => Style::default().fg(Color::Yellow),
        _ => Style::default().fg(Color::Red),
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(info_text, info_style))),
        info_area,
    );

    // Diagnostics list
    let list_block = Block::default()
        .borders(Borders::ALL)
        .title(" Diagnostics ")
        .border_style(Style::default().fg(Color::DarkGray));

    let items: Vec<ListItem> = app
        .lsp_diagnostics
        .iter()
        .map(|d| {
            let sev = d
                .severity
                .map(sisyphus_tools::lsp::severity_name)
                .unwrap_or("?");
            let style = match sev {
                "error" => Style::default().fg(Color::Red),
                "warning" => Style::default().fg(Color::Yellow),
                "info" => Style::default().fg(Color::Cyan),
                _ => Style::default().fg(Color::DarkGray),
            };
            let line = Line::from(vec![
                Span::styled(format!("{:>6} ", sev), style),
                Span::styled(
                    format!(
                        "{}:{}:{}",
                        d.range.start.line, d.range.start.character, d.range.end.character
                    ),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(" "),
                Span::styled(&d.message, Style::default().fg(Color::White)),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).block(list_block);
    frame.render_stateful_widget(
        list,
        list_area,
        &mut ratatui::widgets::ListState::default().with_selected(Some(app.lsp_scroll)),
    );
}

// ── Status bar ──────────────────────────────────────────

fn render_status(app: &App) -> Line<'static> {
    let panel_name = app.active_panel.name();
    let mode_name = match (app.active_panel, app.mode) {
        (Panel::Shell, Mode::Input) => "INPUT",
        (Panel::Shell, Mode::Scroll) => "SCROLL",
        (Panel::Shell, Mode::Select) => "SELECT",
        (Panel::Todos, _) => "SELECT",
        (Panel::Sessions, _) => "SELECT",
        (Panel::Lsp, Mode::Input) => "INPUT",
        (Panel::Lsp, _) => "SELECT",
        (Panel::Sop, _) => "SELECT",
        (Panel::Roadmap, _) => "SELECT",
    };

    let help = match app.active_panel {
        Panel::Shell => match app.mode {
            Mode::Input => {
                "Enter: exec | ↑↓: history | Tab: complete/scroll | Esc: clear | :bridge <cmd>"
            }
            Mode::Scroll => "↑↓: scroll | Tab/Esc: input",
            Mode::Select => "",
        },
        Panel::Todos => "↑↓: select | Space/Enter: toggle | d: delete | r: refresh",
        Panel::Sessions => "↑↓: select | Enter: detail | Esc: back | r: refresh",
        Panel::Lsp => match app.mode {
            Mode::Input => "Enter: submit | Esc: cancel",
            Mode::Scroll | Mode::Select => "f: path | l: lang | s: server | a: args | Enter: run",
        },
        Panel::Sop => "↑↓: select | Enter: execute | r: refresh | Ctrl-1/2/3/4/5/6: switch",
        Panel::Roadmap => "r: refresh | Ctrl-1/2/3/4/5/6: switch",
    };

    Line::from(vec![
        Span::styled(
            format!(" {panel_name}/{mode_name} "),
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
        Span::raw(" "),
        Span::styled(help, Style::default().fg(Color::DarkGray)),
    ])
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
        assert_eq!(app.active_panel, Panel::Shell);
        assert_eq!(app.mode, Mode::Input);
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
    fn test_render_status_shows_panel_mode_and_help() {
        let mut app = App::new(PathBuf::from("/tmp"), "addr".into());
        app.active_panel = Panel::Shell;
        app.mode = Mode::Input;
        let line = render_status(&app);
        let rendered = line.to_string();
        assert!(rendered.contains("Shell/INPUT"));
        assert!(rendered.contains("Enter:"));

        app.mode = Mode::Scroll;
        let line = render_status(&app);
        let rendered = line.to_string();
        assert!(rendered.contains("Shell/SCROLL"));
        assert!(rendered.contains("Tab/Esc"));
    }

    #[test]
    fn test_render_status_todos_panel() {
        let mut app = App::new(PathBuf::from("/tmp"), "addr".into());
        app.active_panel = Panel::Todos;
        let line = render_status(&app);
        let rendered = line.to_string();
        assert!(rendered.contains("Todos/SELECT"));
        assert!(rendered.contains("toggle"));
    }

    #[test]
    fn test_render_status_sessions_panel() {
        let mut app = App::new(PathBuf::from("/tmp"), "addr".into());
        app.active_panel = Panel::Sessions;
        let line = render_status(&app);
        let rendered = line.to_string();
        assert!(rendered.contains("Sessions/SELECT"));
        assert!(rendered.contains("refresh"));
    }

    #[test]
    fn test_render_status_lsp_panel() {
        let mut app = App::new(PathBuf::from("/tmp"), "addr".into());
        app.active_panel = Panel::Lsp;
        app.mode = Mode::Input;
        let line = render_status(&app);
        let rendered = line.to_string();
        assert!(rendered.contains("LSP/INPUT"));
        assert!(rendered.contains("Enter: submit"));

        app.mode = Mode::Select;
        let line = render_status(&app);
        let rendered = line.to_string();
        assert!(rendered.contains("LSP/SELECT"));
        assert!(rendered.contains("f: path"));
    }

    #[test]
    fn test_render_status_sop_panel() {
        let mut app = App::new(PathBuf::from("/tmp"), "addr".into());
        app.active_panel = Panel::Sop;
        let line = render_status(&app);
        let rendered = line.to_string();
        assert!(rendered.contains("SOP/SELECT"));
        assert!(rendered.contains("execute"));
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

        // Select and delete
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
