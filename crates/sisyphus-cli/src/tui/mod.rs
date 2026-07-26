use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;

pub(crate) mod app;
pub(crate) mod background;
mod dialog;
mod enhancements;
mod event;
mod helpers;
mod home;
mod render;
mod sessions;
mod theme;
mod widgets;

use crate::permission::CliPermission;
pub(crate) use app::{App, DevFocus, LogEntry, Mode, ToastKind, View};
pub(crate) use event::handle_event;
pub(crate) use render::ui;

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
    app.sessions = crate::tools::session_list(&db).unwrap_or_default();
    app.session_dirty = false;

    app.load_boulder();

    // Subscribe to bridge event stream to eliminate polling latency
    if let Some(mut client) = app.bridge_client.clone() {
        if let Ok(rx) = futures::executor::block_on(client.subscribe_events()) {
            app.event_rx = Some(rx);
        }
    }

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
        // Process all pending events from the bridge stream
        app.process_events();

        app.boulder_save_tick += 1;
        if app.boulder_save_tick >= 100 {
            app.boulder_save_tick = 0;
            app.save_boulder();
        }

        app.prune_expired_toasts();

        terminal.draw(|f| ui(f, app))?;

        // Advance spinner frame when operation is pending
        if app.is_pending {
            app.pending_spin = app.pending_spin.wrapping_add(1);
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
            if app.view == View::Session && handle_event(fake_event, app, perms)? {
                break;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
