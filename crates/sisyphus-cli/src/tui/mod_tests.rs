use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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
    let app = App::new(
        PathBuf::from("/tmp/test"),
        "localhost:50051".into(),
        "http://[::1]:8080".into(),
    );
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
    let app = App::new(
        PathBuf::from("/my/data"),
        "127.0.0.1:9999".into(),
        "http://[::1]:8080".into(),
    );
    assert_eq!(app.data_dir, PathBuf::from("/my/data"));
    assert_eq!(app.bridge_address, "127.0.0.1:9999");
}

// ── App::add_log ──────────────────────────────────────

#[tokio::test]
async fn test_add_log_appends_and_adjusts_scroll() {
    let mut app = App::new(
        PathBuf::from("/tmp"),
        "addr".into(),
        "http://[::1]:8080".into(),
    );
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
    let app = App::new(
        PathBuf::from("/tmp"),
        "addr".into(),
        "http://[::1]:8080".into(),
    );
    let rendered = render::render_status(&app).to_string();
    assert!(rendered.contains("SESSION") || rendered.contains("HOME"));
    assert!(rendered.contains("Context: 0%"));
    assert!(rendered.contains("unknown"));
}

#[tokio::test]
async fn test_render_status_home_help() {
    let mut app = App::new(
        PathBuf::from("/tmp"),
        "addr".into(),
        "http://[::1]:8080".into(),
    );
    app.view = View::Home;
    let rendered = render::render_status(&app).to_string();
    assert!(rendered.contains("Enter: start session"));
}

#[tokio::test]
async fn test_render_status_session_list_help() {
    let mut app = App::new(
        PathBuf::from("/tmp"),
        "addr".into(),
        "http://[::1]:8080".into(),
    );
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
    let mut app = App::new(
        PathBuf::from("/tmp"),
        "addr".into(),
        "http://[::1]:8080".into(),
    );
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
    let app = App::new(
        PathBuf::from("/tmp"),
        "addr".into(),
        "http://[::1]:8080".into(),
    );
    assert_eq!(render::context_pct(&app), 0);
}

#[tokio::test]
async fn test_diag_counts_empty() {
    let app = App::new(
        PathBuf::from("/tmp"),
        "addr".into(),
        "http://[::1]:8080".into(),
    );
    assert_eq!(render::diag_counts(&app), (0, 0, 0));
}

// ── 带临时文件的 App 待办 ──────────────────────────

#[tokio::test]
async fn test_refresh_todos_empty() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(
        dir.path().to_path_buf(),
        "addr".into(),
        "http://[::1]:8080".into(),
    );
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

    let mut app = App::new(
        dir.path().to_path_buf(),
        "addr".into(),
        "http://[::1]:8080".into(),
    );
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

    let mut app = App::new(
        dir.path().to_path_buf(),
        "addr".into(),
        "http://[::1]:8080".into(),
    );
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
async fn test_delete_todo_requires_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("todos.json");
    let _item = crate::tools::todo_add(&db, "delete me", crate::tools::TodoPriority::Low).unwrap();

    let mut app = App::new(
        dir.path().to_path_buf(),
        "addr".into(),
        "http://[::1]:8080".into(),
    );
    app.todo_dirty = true;
    app.refresh_todos();
    assert_eq!(app.todo_items.len(), 1);
    let todo_id = app.todo_items[0].id.clone();

    // 打开 todo 面板，按 d：此时**不得**删除，只应弹出确认框
    app.dialog = Some(dialog::Dialog::Todos);
    press(&mut app, KeyCode::Char('d'));
    assert!(
        matches!(app.dialog, Some(dialog::Dialog::ConfirmDelete(_))),
        "按 d 应进入确认状态而非直接删除"
    );
    app.refresh_todos();
    assert_eq!(app.todo_items.len(), 1, "确认前 todo 不得被删除");

    // 按 n 取消：todo 仍在
    press(&mut app, KeyCode::Char('n'));
    assert!(app.dialog.is_none());
    app.todo_dirty = true;
    app.refresh_todos();
    assert_eq!(app.todo_items.len(), 1, "取消后 todo 不得被删除");
    assert!(app.confirm_delete.is_none());

    // 再次按 d 并按 y：此时才真正删除
    app.dialog = Some(dialog::Dialog::Todos);
    press(&mut app, KeyCode::Char('d'));
    assert_eq!(app.confirm_delete.as_deref(), Some(todo_id.as_str()));
    press(&mut app, KeyCode::Char('y'));
    app.todo_dirty = true;
    app.refresh_todos();
    assert!(app.todo_items.is_empty(), "确认后 todo 应被删除");
}

/// 通过真实按键分发路径发送一次按键
fn press(app: &mut App, code: KeyCode) {
    dialog::handle_dialog_key(KeyEvent::new(code, KeyModifiers::NONE), app);
}

#[tokio::test]
async fn test_delete_workspace_requires_confirmation() {
    let mut app = App::new(
        PathBuf::from("/tmp/nonexistent"),
        "addr".into(),
        "http://[::1]:8080".into(),
    );
    app.workspaces = vec![crate::tui::app::WorkspaceEntry {
        id: "ws-1".to_string(),
        name: "my-workspace".to_string(),
        project_id: "proj-1".to_string(),
    }];

    // 按 d 只应进入确认状态，不得直接派发删除后台任务
    app.dialog = Some(dialog::Dialog::WorkspaceList);
    press(&mut app, KeyCode::Char('d'));
    assert!(
        matches!(app.dialog, Some(dialog::Dialog::ConfirmDelete(_))),
        "按 d 应进入确认状态而非直接删除工作区"
    );
    assert_eq!(app.confirm_delete.as_deref(), Some("ws-1"));
    assert!(
        app.pending_op.is_none(),
        "确认前不得派发删除工作区的后台任务"
    );

    // 按 n 取消：不应派发任务
    press(&mut app, KeyCode::Char('n'));
    assert!(app.dialog.is_none());
    assert!(app.pending_op.is_none(), "取消后不得派发删除任务");
}

#[tokio::test]
async fn test_toggle_todo_empty_no_panic() {
    let mut app = App::new(
        PathBuf::from("/tmp/nonexistent"),
        "addr".into(),
        "http://[::1]:8080".into(),
    );
    app.todo_dirty = true;
    app.refresh_todos();
    app.toggle_todo_status(); // should not panic when empty
}

// ── 带临时文件的 App 会话 ───────────────────────

#[tokio::test]
async fn test_refresh_sessions_empty() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(
        dir.path().to_path_buf(),
        "addr".into(),
        "http://[::1]:8080".into(),
    );
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

    let mut app = App::new(
        dir.path().to_path_buf(),
        "addr".into(),
        "http://[::1]:8080".into(),
    );
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

    let mut app = App::new(
        dir.path().to_path_buf(),
        "addr".into(),
        "http://[::1]:8080".into(),
    );
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
    let mut app = App::new(
        PathBuf::from("/tmp/nonexistent"),
        "addr".into(),
        "http://[::1]:8080".into(),
    );
    app.select_session(); // should not panic when empty
    assert!(app.session_detail.is_none());
}

// ── Cognitive Trace (explainability) ────────────────────

#[test]
fn test_format_cognitive_step_event() {
    let event = sisyphus_types::ExecutionEvent::CognitiveStep {
        step: "Predicting".into(),
        message: "predict node output".into(),
        metrics: Some(sisyphus_types::CognitiveMetrics {
            iteration: 2,
            deviation: 0.35,
            confidence: 0.85,
            progress: 0.5,
            token_cost: 120,
            stability: 0.9,
            info_gain: 0.05,
        }),
    };
    let text = dialog::handlers::format_cognitive_event(&event);
    assert!(text.contains("Step[Predicting]"));
    assert!(text.contains("predict node output"));
    assert!(text.contains("deviation: 0.35"));
    assert!(text.contains("confidence: 0.85"));
    assert!(text.contains("token_cost: 120"));
}

#[test]
fn test_format_micro_reflection_event() {
    let event = sisyphus_types::ExecutionEvent::MicroReflection {
        node_name: "node-1".into(),
        prediction: "expected output".into(),
        actual: "actual output".into(),
        decision: "adjust".into(),
        updates: Some(std::collections::HashMap::from([(
            "temperature".into(),
            serde_json::json!(0.4),
        )])),
        reason: Some("prediction drifted".into()),
    };
    let text = dialog::handlers::format_cognitive_event(&event);
    assert!(text.contains("node-1"));
    assert!(text.contains("prediction: expected output"));
    assert!(text.contains("actual: actual output"));
    assert!(text.contains("decision: adjust"));
    assert!(text.contains("reason: prediction drifted"));
    assert!(text.contains("update temperature: 0.4"));
}

#[test]
fn test_cognitive_trace_row_deserialize_from_rest_contract() {
    // 契约测试：与 REST /api/cognitive/trace 返回结构对齐（CognitiveTraceEntry JSON）。
    let json = serde_json::json!({
        "timestamp_ms": 1700000000123_u64,
        "sop_id": "sop-test",
        "event": {
            "CognitiveStep": {
                "step": "Reflecting",
                "message": "reflect on result",
                "metrics": {
                    "iteration": 1,
                    "deviation": 0.1,
                    "confidence": 0.9,
                    "progress": 0.6,
                    "token_cost": 50,
                    "stability": 0.95,
                    "info_gain": 0.02
                }
            }
        }
    });
    let row: background::CognitiveTraceRow =
        serde_json::from_value(json).expect("contract parse failed");
    assert_eq!(row.timestamp_ms, 1700000000123);
    assert_eq!(row.sop_id, "sop-test");
    match row.event {
        sisyphus_types::ExecutionEvent::CognitiveStep { step, metrics, .. } => {
            assert_eq!(step, "Reflecting");
            let m = metrics.expect("metrics present");
            assert_eq!(m.deviation, 0.1);
            assert_eq!(m.confidence, 0.9);
        },
        other => panic!("unexpected event variant: {other:?}"),
    }
}

#[test]
fn test_cognitive_summary_row_deserialize_from_rest_contract() {
    // 契约测试：与 REST /api/cognitive/summary 返回结构对齐（CognitiveTraceSummary JSON）。
    let json = serde_json::json!({
        "total_decisions": 12,
        "step_count": 8,
        "reflection_count": 4,
        "avg_deviation": 0.21,
        "avg_confidence": 0.83,
        "avg_stability": 0.91,
        "avg_info_gain": 0.05,
        "total_token_cost": 430,
        "decision_counts": {"CONTINUE": 3, "ADJUST": 1},
        "first_timestamp_ms": 1700000000000_u64,
        "last_timestamp_ms": 1700000005000_u64
    });
    let summary: background::CognitiveSummaryRow =
        serde_json::from_value(json).expect("summary contract parse failed");
    assert_eq!(summary.total_decisions, 12);
    assert_eq!(summary.step_count, 8);
    assert_eq!(summary.reflection_count, 4);
    assert!((summary.avg_deviation - 0.21).abs() < 1e-9);
    assert!((summary.avg_confidence - 0.83).abs() < 1e-9);
    assert_eq!(summary.total_token_cost, 430);
    assert_eq!(summary.decision_counts.get("ADJUST"), Some(&1));
    assert_eq!(summary.first_timestamp_ms, 1700000000000);
}

/// 回归：TUI 的 REST 调用曾拼出 `http://http://[::1]:50051/api/...`。
/// 配置里的 bridge_address 自带 scheme，再加一层前缀 reqwest 直接拒绝解析，
/// 于是健康检查/认知轨迹/技能/工作区全部静默失败。测试用生产默认配置值断言。
#[test]
fn rest_base_never_double_prefixes_scheme() {
    let base = crate::tui::rest_base_from("http://[::1]:8080");
    assert_eq!(base, "http://[::1]:8080");

    let url = format!("{base}/health/deep");
    assert_eq!(url, "http://[::1]:8080/health/deep");
    assert!(!url.contains("http://http://"), "scheme must appear once");
    assert!(reqwest::Url::parse(&url).is_ok(), "URL must parse");
}

/// 回归：REST 基址不得沿用 gRPC 端口。
///
/// TUI 的 REST 调用此前直接复用 `bridge_address`（gRPC，默认 50051），
/// 而 REST 监听在 `http_address`（默认 8080）。修好 scheme 之后仍会打错端口，
/// 所有 REST 功能依旧静默失败。
#[test]
fn rest_base_uses_rest_address_not_grpc_address() {
    let from_grpc = crate::tui::rest_base_from("http://[::1]:50051");
    let from_rest = crate::tui::rest_base_from("http://[::1]:8080");

    assert_ne!(
        from_grpc, from_rest,
        "gRPC and REST addresses must yield different bases"
    );
    assert!(from_rest.contains(":8080"));
    assert!(!from_rest.contains("50051"));
    assert_eq!(
        format!("{from_rest}/health/deep"),
        "http://[::1]:8080/health/deep"
    );
}

#[test]
fn rest_base_accepts_bare_host_port() {
    assert_eq!(
        crate::tui::rest_base_from("127.0.0.1:9999"),
        "http://127.0.0.1:9999"
    );
}

#[test]
fn rest_base_strips_trailing_slash() {
    assert_eq!(
        crate::tui::rest_base_from("http://localhost:8080/"),
        "http://localhost:8080"
    );
}
