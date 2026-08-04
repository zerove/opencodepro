use super::*;
use tokio;

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

#[tokio::test]
async fn test_app_new_initial_state() {
    let app = App::new(PathBuf::from("/tmp/test"), "localhost:50051".into());
    assert_eq!(app.view, View::Home);
    assert_eq!(app.session_mode, app::Mode::Select);
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

#[tokio::test]
async fn test_app_new_data_dir() {
    let app = App::new(PathBuf::from("/my/data"), "127.0.0.1:9999".into());
    assert_eq!(app.data_dir, PathBuf::from("/my/data"));
    assert_eq!(app.bridge_address, "127.0.0.1:9999");
}

// ── App::add_log ──────────────────────────────────────

#[tokio::test]
async fn test_add_log_appends_and_adjusts_scroll() {
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

#[tokio::test]
async fn test_render_status_shows_opencode_format() {
    let app = App::new(PathBuf::from("/tmp"), "addr".into());
    let rendered = render::render_status(&app).to_string();
    assert!(rendered.contains("SESSION") || rendered.contains("HOME"));
    assert!(rendered.contains("Context: 0%"));
    assert!(rendered.contains("unknown"));
}

#[tokio::test]
async fn test_render_status_home_help() {
    let mut app = App::new(PathBuf::from("/tmp"), "addr".into());
    app.view = View::Home;
    let rendered = render::render_status(&app).to_string();
    assert!(rendered.contains("Enter: start session"));
}

#[tokio::test]
async fn test_render_status_session_list_help() {
    let mut app = App::new(PathBuf::from("/tmp"), "addr".into());
    app.view = View::Session;
    let rendered = render::render_status(&app).to_string();
    assert!(rendered.contains("Enter open"));
}

#[tokio::test]
async fn test_render_status_diag_counts() {
    use sisyphus_tools_build::lsp::{LspDiagnostic, LspPosition, LspRange};
    let rng = LspRange {
        start: LspPosition {
            line: 0,
            character: 0,
        },
        end: LspPosition {
            line: 1,
            character: 1,
        },
    };
    let mut app = App::new(PathBuf::from("/tmp"), "addr".into());
    app.lsp_diagnostics = vec![
        LspDiagnostic {
            range: rng.clone(),
            severity: Some(1),
            message: "err".into(),
        },
        LspDiagnostic {
            range: rng.clone(),
            severity: Some(2),
            message: "warn".into(),
        },
        LspDiagnostic {
            range: rng,
            severity: Some(3),
            message: "info".into(),
        },
    ];
    let rendered = render::render_status(&app).to_string();
    assert!(rendered.contains("E1"));
    assert!(rendered.contains("W1"));
    assert!(rendered.contains("I1"));
}

#[tokio::test]
async fn test_context_pct_no_session() {
    let app = App::new(PathBuf::from("/tmp"), "addr".into());
    assert_eq!(render::context_pct(&app), 0);
}

#[tokio::test]
async fn test_diag_counts_empty() {
    let app = App::new(PathBuf::from("/tmp"), "addr".into());
    assert_eq!(render::diag_counts(&app), (0, 0, 0));
}

// ── 带临时文件的 App 待办 ──────────────────────────

#[tokio::test]
async fn test_refresh_todos_empty() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(dir.path().to_path_buf(), "addr".into());
    // todo_dirty 初始为 true，因此 refresh 会读取文件
    assert!(app.todo_dirty);
    app.refresh_todos();
    assert!(app.todo_items.is_empty());
    assert!(!app.todo_dirty);
}

#[tokio::test]
async fn test_refresh_todos_with_data() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("todos.json");
    // 通过公共 API 播种待办
    crate::tools::todo_add(&db, "write tests", crate::tools::TodoPriority::High).unwrap();

    let mut app = App::new(dir.path().to_path_buf(), "addr".into());
    app.todo_dirty = true;
    app.refresh_todos();
    assert_eq!(app.todo_items.len(), 1);
    assert_eq!(app.todo_items[0].content, "write tests");
    assert_eq!(app.todo_sel, 0);
}

#[tokio::test]
async fn test_toggle_todo_status_cycles() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("todos.json");
    let _item =
        crate::tools::todo_add(&db, "cycle test", crate::tools::TodoPriority::Medium).unwrap();

    let mut app = App::new(dir.path().to_path_buf(), "addr".into());
    app.todo_dirty = true;
    app.refresh_todos();
    assert_eq!(app.todo_items.len(), 1);
    assert!(matches!(
        app.todo_items[0].status,
        crate::tools::TodoStatus::Pending
    ));

    // 待处理 → 进行中
    app.toggle_todo_status();
    app.refresh_todos();
    assert!(matches!(
        app.todo_items[0].status,
        crate::tools::TodoStatus::InProgress
    ));

    // 进行中 → 已完成
    app.toggle_todo_status();
    app.refresh_todos();
    assert!(matches!(
        app.todo_items[0].status,
        crate::tools::TodoStatus::Completed
    ));

    // 已完成 → 待处理
    app.toggle_todo_status();
    app.refresh_todos();
    assert!(matches!(
        app.todo_items[0].status,
        crate::tools::TodoStatus::Pending
    ));
}

#[tokio::test]
async fn test_delete_todo_removes_item() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("todos.json");
    let _item = crate::tools::todo_add(&db, "delete me", crate::tools::TodoPriority::Low).unwrap();

    let mut app = App::new(dir.path().to_path_buf(), "addr".into());
    app.todo_dirty = true;
    app.refresh_todos();
    assert_eq!(app.todo_items.len(), 1);

    // 第一次调用设置确认，第二次真正删除
    app.delete_todo();
    assert!(app.confirm_delete.is_some());
    app.delete_todo();
    app.todo_dirty = true;
    app.refresh_todos();
    assert!(app.todo_items.is_empty());
}

#[tokio::test]
async fn test_toggle_todo_empty_no_panic() {
    let mut app = App::new(PathBuf::from("/tmp/nonexistent"), "addr".into());
    app.toggle_todo_status(); // should not panic when empty
    app.delete_todo(); // should not panic when empty
}

// ── 带临时文件的 App 会话 ───────────────────────

#[tokio::test]
async fn test_refresh_sessions_empty() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(dir.path().to_path_buf(), "addr".into());
    assert!(app.session_dirty);
    app.refresh_sessions();
    assert!(app.sessions.is_empty());
    assert!(!app.session_dirty);
}

#[tokio::test]
async fn test_refresh_sessions_with_data() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("sessions.json");
    let _session = crate::tools::session_create(&db, "test-session").unwrap();

    let mut app = App::new(dir.path().to_path_buf(), "addr".into());
    app.session_dirty = true;
    app.refresh_sessions();
    assert_eq!(app.sessions.len(), 1);
    assert_eq!(app.sessions[0].name, "test-session");
}

#[tokio::test]
async fn test_select_session_loads_detail() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("sessions.json");
    let session = crate::tools::session_create(&db, "detail-test").unwrap();

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

#[tokio::test]
async fn test_select_session_empty_noop() {
    let mut app = App::new(PathBuf::from("/tmp/nonexistent"), "addr".into());
    app.select_session(); // should not panic when empty
    assert!(app.session_detail.is_none());
}
