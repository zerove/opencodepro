use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;

pub(crate) mod app;
pub(crate) mod background;
pub(crate) mod dashboard;
mod dialog;
pub mod enhancements;
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

// ── 公开入口 ────────────────────────────────────────

/// Derive a REST base URL from a gRPC bridge address.
///
/// The configured bridge address already carries a scheme (`http://[::1]:50051`),
/// so prefixing another one yields `http://http://...` and reqwest rejects it.
/// Strip any scheme so callers can concatenate paths directly.
pub(crate) fn rest_base_from(bridge_address: &str) -> String {
    let trimmed = bridge_address
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/');
    if bridge_address.trim_start().starts_with("https://") {
        format!("https://{trimmed}")
    } else {
        format!("http://{trimmed}")
    }
}

/// Run.
pub fn run(data_dir: PathBuf, bridge_address: String, http_address: String) -> Result<()> {
    // 确保数据目录存在（与 main.rs:1134 的 CLI 模式一致）
    std::fs::create_dir_all(&data_dir)?;

    let mut terminal = ratatui::init();
    terminal.clear()?;

    crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)?;

    let perms = CliPermission::load(None).unwrap_or_default();
    let mut app = App::new(data_dir, bridge_address, http_address);

    // 注册审批 broker（T2.2）：后台工具执行在 Ask 决策时向 TUI 请求放行。
    crate::approval::register();

    // 加载会话列表（不自动创建或选择 — 从主页开始）
    let db = app.data_dir.join("sessions.json");
    app.sessions = crate::tools::session_list(&db).unwrap_or_default();
    app.session_dirty = false;

    app.load_boulder();

    // 订阅桥接事件流以消除轮询延迟
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
        // 处理桥接流中所有待处理事件
        app.process_events();

        app.boulder_save_tick += 1;
        if app.boulder_save_tick >= 100 {
            app.boulder_save_tick = 0;
            app.save_boulder();
        }

        app.prune_expired_toasts();

        terminal.draw(|f| ui(f, app))?;

        // 有操作挂起时推进 spinner 帧
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
            },
        };
        if poll_ok {
            let event = match crossterm::event::read() {
                Ok(e) => e,
                Err(e) => {
                    app.add_log(LogEntry::Warn(format!("event::read failed (IME?): {e}")));
                    continue;
                },
            };
            event_received = true;
            match handle_event(event, app, perms) {
                Ok(true) => break,
                Ok(false) => {},
                Err(e) => {
                    app.add_log(LogEntry::Error(format!("handle_event error: {e}")));
                },
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
            // 仅在会话中时自动继续
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
