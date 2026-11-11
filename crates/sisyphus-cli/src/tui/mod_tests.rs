use super::*;
use crate::tui::app::{RemoteSkillEntry, SelfProgrammingStats};
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
fn test_format_tool_call_event() {
    let event = sisyphus_types::ExecutionEvent::ToolCall {
        tool_id: "builtin:file_read".into(),
        phase: "completed".into(),
        duration_ms: Some(12),
        success: Some(true),
        error: None,
    };
    let text = dialog::handlers::format_cognitive_event(&event);
    assert!(text.contains("Tool builtin:file_read: completed"));
    assert!(text.contains("duration: 12ms"));
    assert!(text.contains("success: true"));
}

#[test]
fn test_format_tool_call_failure_event() {
    let event = sisyphus_types::ExecutionEvent::ToolCall {
        tool_id: "builtin:bash_exec".into(),
        phase: "failed".into(),
        duration_ms: Some(30),
        success: Some(false),
        error: Some("permission denied".into()),
    };
    let text = dialog::handlers::format_cognitive_event(&event);
    assert!(text.contains("phase") || text.contains("failed"));
    assert!(text.contains("permission denied"));
}

#[test]
fn test_format_token_usage_event() {
    let event = sisyphus_types::ExecutionEvent::TokenUsage {
        model: Some("gpt-probe".into()),
        prompt_tokens: 10,
        completion_tokens: 20,
        total_tokens: 30,
    };
    let text = dialog::handlers::format_cognitive_event(&event);
    assert!(text.contains("Token usage (gpt-probe)"));
    assert!(text.contains("prompt: 10"));
    assert!(text.contains("completion: 20"));
    assert!(text.contains("total: 30"));
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

/// 契约测试：与 REST `/api/anomaly` 返回结构对齐（bridge 的 AnomalyReport JSON）。
#[test]
fn test_anomaly_row_deserialize_from_rest_contract() {
    let json = serde_json::json!({
        "node_name": "node_a",
        "severity": "high",
        "metric": "duration",
        "observed": 42.0,
        "expected": 10.0,
        "description": "duration deviation",
        "timestamp": 1700000000_u64
    });
    let row: background::AnomalyRow =
        serde_json::from_value(json).expect("anomaly contract parse failed");
    assert_eq!(row.node_name, "node_a");
    assert_eq!(row.severity, "high");
    assert_eq!(row.metric, "duration");
    assert!((row.observed - 42.0).abs() < 1e-9);
    assert!((row.expected - 10.0).abs() < 1e-9);
    assert_eq!(row.timestamp, 1_700_000_000);
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

// ── 自编程统计（仪表盘数据源） ────────────────────────

fn skill(id: &str, author: &str, tags: &[&str]) -> RemoteSkillEntry {
    RemoteSkillEntry {
        id: id.into(),
        name: id.into(),
        description: String::new(),
        author: author.into(),
        version: "0.1.0".into(),
        tags: tags.iter().map(|t| (*t).to_string()).collect(),
    }
}

#[test]
fn sp_stats_empty_list_has_no_quality() {
    let stats = SelfProgrammingStats::from_skills(&[]);
    assert_eq!(stats.self_programmed, 0);
    assert_eq!(stats.average_quality, None);
}

#[test]
fn sp_stats_counts_self_programmed_by_tag() {
    let skills = vec![
        skill("a", "human", &["manual"]),
        skill(
            "b",
            "sisyphus-self-programming",
            &["auto-generated", "self-programming", "quality:80"],
        ),
        skill("c", "someone", &["self-programming", "quality:60"]),
    ];
    let stats = SelfProgrammingStats::from_skills(&skills);
    assert_eq!(stats.self_programmed, 2);
    assert_eq!(stats.average_quality, Some(70));
}

#[test]
fn sp_stats_counts_self_programmed_by_author_when_tag_missing() {
    // register_skill 同时写 author 与 tag；只有 author 时也必须识别
    let skills = vec![skill("b", "sisyphus-self-programming", &["quality:90"])];
    let stats = SelfProgrammingStats::from_skills(&skills);
    assert_eq!(stats.self_programmed, 1);
    assert_eq!(stats.average_quality, Some(90));
}

#[test]
fn sp_stats_ignores_manual_skill_quality_tags() {
    // 非自编程技能即使带 quality 标签也不参与均值
    let skills = vec![
        skill("a", "human", &["quality:100"]),
        skill(
            "b",
            "sisyphus-self-programming",
            &["self-programming", "quality:40"],
        ),
    ];
    let stats = SelfProgrammingStats::from_skills(&skills);
    assert_eq!(stats.self_programmed, 1);
    assert_eq!(stats.average_quality, Some(40));
}

#[test]
fn sp_stats_quality_tag_parsing_rejects_bad_values() {
    assert_eq!(skill("x", "a", &["quality:abc"]).quality_score(), None);
    assert_eq!(skill("x", "a", &["quality:101"]).quality_score(), None);
    assert_eq!(skill("x", "a", &["quality:"]).quality_score(), None);
    assert_eq!(skill("x", "a", &["quality:0"]).quality_score(), Some(0));
    assert_eq!(skill("x", "a", &["quality:100"]).quality_score(), Some(100));
    assert_eq!(skill("x", "a", &["other"]).quality_score(), None);
}

#[test]
fn sp_stats_self_programmed_without_quality_tags_yields_none_average() {
    let skills = vec![skill(
        "b",
        "sisyphus-self-programming",
        &["self-programming"],
    )];
    let stats = SelfProgrammingStats::from_skills(&skills);
    assert_eq!(stats.self_programmed, 1);
    assert_eq!(
        stats.average_quality, None,
        "no quality tags must not fabricate a score"
    );
}

#[test]
fn self_programmed_badge_empty_for_manual_skill() {
    let s = skill("a", "human", &["quality:90"]);
    assert_eq!(crate::tui::widgets::self_programmed_badge(&s), "");
}

#[test]
fn self_programmed_badge_includes_quality_when_present() {
    let s = skill(
        "b",
        "sisyphus-self-programming",
        &["self-programming", "quality:88"],
    );
    assert_eq!(
        crate::tui::widgets::self_programmed_badge(&s),
        " ⚙ auto q88% "
    );
}

#[test]
fn self_programmed_badge_without_quality_tag() {
    let s = skill("b", "sisyphus-self-programming", &["self-programming"]);
    assert_eq!(crate::tui::widgets::self_programmed_badge(&s), " ⚙ auto ");
}

// ── T2.1 MVP：dev 审批与中断（S3 接缝）──────────────────────

/// 构造一个 dev 运行句柄，并返回其审批应答接收端。
fn dev_handle_with_answer() -> (
    crate::tui::background::DevRunHandle,
    tokio::sync::mpsc::UnboundedReceiver<bool>,
) {
    let (answer_tx, answer_rx) = tokio::sync::mpsc::unbounded_channel::<bool>();
    let (_result_tx, result_rx) = tokio::sync::oneshot::channel::<crate::tui::background::BgOp>();
    let (_ask_tx, asks) = tokio::sync::mpsc::unbounded_channel::<crate::tui::background::BgOp>();
    (
        crate::tui::background::DevRunHandle {
            result: result_rx,
            asks,
            answer: answer_tx,
            interrupt: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        },
        answer_rx,
    )
}

fn dev_app() -> App {
    let mut app = App::new(
        PathBuf::from("/tmp/opencode/t2_1_dev_app"),
        "addr".into(),
        "http://[::1]:8080".into(),
    );
    app.dialog = Some(dialog::Dialog::Dev {
        goal: String::new(),
    });
    app
}

#[tokio::test]
async fn test_dev_ask_y_approves_tool_call() {
    let mut app = dev_app();
    app.dev_status = "running".into();
    app.dev_ask = Some("builtin:bash_exec".into());
    let (handle, mut answer_rx) = dev_handle_with_answer();
    app.pending_dev = Some(handle);

    press(&mut app, KeyCode::Char('y'));

    assert!(app.dev_ask.is_none(), "应答后应清除待审批状态");
    assert_eq!(answer_rx.try_recv().ok(), Some(true), "y 应放行");
}

#[tokio::test]
async fn test_dev_ask_n_denies_tool_call() {
    let mut app = dev_app();
    app.dev_status = "running".into();
    app.dev_ask = Some("builtin:bash_exec".into());
    let (handle, mut answer_rx) = dev_handle_with_answer();
    app.pending_dev = Some(handle);

    press(&mut app, KeyCode::Char('n'));

    assert!(app.dev_ask.is_none(), "应答后应清除待审批状态");
    assert_eq!(answer_rx.try_recv().ok(), Some(false), "n 应拒绝");
}

#[tokio::test]
async fn test_dev_esc_denies_and_interrupts() {
    let mut app = dev_app();
    app.dev_status = "running".into();
    app.dev_ask = Some("builtin:bash_exec".into());
    let (handle, mut answer_rx) = dev_handle_with_answer();
    app.pending_dev = Some(handle);

    press(&mut app, KeyCode::Esc);

    assert_eq!(answer_rx.try_recv().ok(), Some(false), "Esc 应拒绝本次审批");
    assert_eq!(app.dev_status, "idle");
    assert_eq!(app.dev_focus, app::DevFocus::Resume);
    assert!(app.dev_resume, "中断后应提供恢复入口");
}

#[tokio::test]
async fn test_dev_esc_interrupts_running_agent() {
    let mut app = dev_app();
    app.dev_status = "running".into();
    let (handle, _answer_rx) = dev_handle_with_answer();
    app.pending_dev = Some(handle);

    press(&mut app, KeyCode::Esc);

    assert_eq!(app.dev_status, "idle");
    assert_eq!(app.dev_focus, app::DevFocus::Resume);
    assert!(app.dev_resume, "中断后应提供恢复入口");
    let interrupted = app
        .pending_dev
        .as_ref()
        .map(|h| h.interrupt.load(std::sync::atomic::Ordering::SeqCst))
        .unwrap_or(false);
    assert!(
        interrupted,
        "Esc 应置位中断标志，使任务在步骤边界返回 Cancelled"
    );
}

#[tokio::test]
async fn test_dev_ask_state_set_by_bg_op() {
    let mut app = dev_app();
    app.dev_status = "running".into();
    app.apply_bg_op(crate::tui::background::BgOp::DevAsk {
        tool_id: "builtin:file_write".into(),
    });
    assert_eq!(app.dev_ask.as_deref(), Some("builtin:file_write"));
}
