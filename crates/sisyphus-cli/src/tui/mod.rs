use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;

mod app;
mod background;
mod dialog;
mod event;
mod helpers;
mod home;
mod render;
mod sessions;
mod theme;
mod widgets;

pub(crate) use app::{App, DevFocus, LogEntry, Mode, ToastKind, View};
pub(crate) use background::BgOp;
pub(crate) use event::handle_event;
pub(crate) use render::ui;
use crate::permission::CliPermission;

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
    terminal: &mut ratatui::DefaultTerminal,
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
        if app.last_refresh.elapsed() >= app::AUTO_REFRESH_INTERVAL {
            app.todo_dirty = true;
            app.session_dirty = true;
            app.last_refresh = std::time::Instant::now();
        }

        let mut event_received = false;
        let poll_ok = match crossterm::event::poll(tick_rate) {
            Ok(true) => true,
            Ok(false) => false,
            Err(e) => {
                app.add_log(LogEntry::Warn(format!("event::poll failed (IME?): {e}")));
                false
            }
        };
        if poll_ok {
            let event = match crossterm::event::read() {
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
                if app.ralph_idle_cycles >= app::RALPH_IDLE_THRESHOLD {
                    app.ralph_idle_cycles = 0;
                    app.ralph_continue_pending = true;
                }
            }
        }

        if app.ralph_continue_pending {
            app.ralph_continue_pending = false;
            app.session_input = "continue".into();
            let fake_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
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
                app.notification = Some((first_err.entry_text().to_string(), std::time::Instant::now()));
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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
