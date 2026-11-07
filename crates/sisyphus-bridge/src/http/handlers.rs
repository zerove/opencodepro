//! REST API 处理器（一）：认知映射、技能、工作区、目标执行、路线图、会话。

use super::types::*;
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use futures_util::stream::{self};
use sisyphus_types::{graph::SOPGraphConfig, ToolExecutor};
use tokio::sync::mpsc;
use tracing::instrument;

pub(super) async fn get_cognitive_map_handler(
    State(state): State<RestApiState>,
    Path(sop_id): Path<String>,
) -> impl IntoResponse {
    match state.bridge.kernel().execution_manager.cognitive_maps.get(&sop_id) {
        Some(map) => (StatusCode::OK, Json(map.clone())).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "No active cognitive map for this SOP" })),
        )
            .into_response(),
    }
}

pub(super) async fn list_available_skills_handler(
    State(state): State<RestApiState>,
) -> impl IntoResponse {
    match state.bridge.skill_registry.fetch_available_skills().await {
        Ok(skills) => (StatusCode::OK, Json(skills)).into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

pub(super) async fn install_remote_skill_handler(
    State(state): State<RestApiState>,
    Json(req): Json<InstallSkillRequest>,
) -> impl IntoResponse {
    match state.bridge.skill_registry.install_skill(&req.skill_id).await {
        Ok(_) => StatusCode::CREATED.into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

pub(super) async fn list_installed_skills_handler(
    State(state): State<RestApiState>,
) -> impl IntoResponse {
    match state.bridge.skill_registry.list_installed_skills().await {
        Ok(skills) => (StatusCode::OK, Json(skills)).into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

pub(super) async fn uninstall_remote_skill_handler(
    State(state): State<RestApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.bridge.skill_registry.uninstall_skill(&id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

// --- Workspace Management Handlers ---

pub(super) async fn list_workspaces_handler(
    State(state): State<RestApiState>,
) -> impl IntoResponse {
    let workspaces = state.bridge.workspace_manager.list_workspaces().await;
    (StatusCode::OK, Json(workspaces)).into_response()
}

pub(super) async fn create_workspace_handler(
    State(state): State<RestApiState>,
    Json(req): Json<CreateWorkspaceRequest>,
) -> impl IntoResponse {
    match state
        .bridge
        .workspace_manager
        .create_workspace(req.name, req.project_id, req.root_dir)
        .await
    {
        Ok(ws) => (StatusCode::CREATED, Json(ws)).into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

pub(super) async fn switch_workspace_handler(
    State(state): State<RestApiState>,
    Json(req): Json<SwitchWorkspaceRequest>,
) -> impl IntoResponse {
    match state.bridge.workspace_manager.switch_workspace(&req.id).await {
        Ok(_) => StatusCode::OK.into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

pub(super) async fn get_current_workspace_handler(
    State(state): State<RestApiState>,
) -> impl IntoResponse {
    match state.bridge.workspace_manager.get_current().await {
        Some(ws) => (StatusCode::OK, Json(ws)).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "No active workspace" })),
        )
            .into_response(),
    }
}

pub(super) async fn health_handler(State(state): State<RestApiState>) -> impl IntoResponse {
    let _ = state.bridge.kernel();
    Json(HealthResponse {
        status: "SERVING".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

/// Pure logic for the deep health check: classify each component as
/// `ok` / `degraded` / `absent` and return a `(all_ok, components_map)` pair
/// ready to be serialized. Extracted from `health_deep_handler` so the
/// classification rules can be unit-tested without spinning up an axum
/// router and a real `RestApiState`.
pub(super) fn health_deep_assess(
    session_db: &std::path::Path,
    shared_http_present: bool,
    token_budget: Option<&sisyphus_intel::token_budget::TokenBudgetSnapshot>,
) -> (bool, serde_json::Map<String, serde_json::Value>) {
    let mut components = serde_json::Map::new();
    let mut all_ok = true;

    // 1) Session DB: file 已存在 → ok；否则父目录存在 → ok；否则 degraded。
    let db_parent_exists = session_db.parent().is_some_and(|p| p.exists());
    let db_ok = session_db.exists() || db_parent_exists;
    components.insert(
        "session_db".into(),
        serde_json::json!({
            "status": if db_ok { "ok" } else { "degraded" },
            "path": session_db.display().to_string(),
        }),
    );
    all_ok &= db_ok;

    // 2) Shared HTTP client: 缺失在 CLI 模式下是预期的，不算降级。
    let http_status = if shared_http_present { "ok" } else { "absent" };
    components.insert(
        "shared_http".into(),
        serde_json::json!({ "status": http_status }),
    );

    // 3) Kernel handle: 构造时已注入，仅占位。
    let kernel_ok = true;
    components.insert(
        "kernel".into(),
        serde_json::json!({ "status": if kernel_ok { "ok" } else { "degraded" } }),
    );
    all_ok &= kernel_ok;
    // 4) Token budget（阶段 7.1 增强）：可选组件。
    // 无 cap（unbounded）→ ok；使用率 < 90% → ok；≥ 90% 或已耗尽 → degraded。
    if let Some(snap) = token_budget {
        let status = if snap.exhausted {
            "degraded"
        } else if let Some(cap) = snap.cap {
            let used_ratio = snap.usage as f64 / cap.max(1) as f64;
            if used_ratio >= 0.9 {
                "degraded"
            } else {
                "ok"
            }
        } else {
            "ok" // unbounded tracker
        };
        components.insert(
            "token_budget".into(),
            serde_json::json!({
                "status": status,
                "usage": snap.usage,
                "cap": snap.cap,
                "exhausted": snap.exhausted,
            }),
        );
        all_ok &= status == "ok";
    }

    (all_ok, components)
}

/// Deep health check: probe every startup dependency (session DB directory,
/// shared HTTP client presence, kernel handle) and report per-component
/// status. Returns 200 only when every check is OK; partial degradation
/// returns 207 Multi-Status with detailed per-component status. Useful for
/// K8s readiness probes and the TUI `:health` panel (阶段 7.1).
pub(super) async fn health_deep_handler(State(state): State<RestApiState>) -> impl IntoResponse {
    // Token budget snapshot（阶段 7.1 增强）：从 kernel.strategic_manager 读快照
    let token_snap = state.bridge.kernel().strategic_manager.token_budget.read().snapshot();
    let (all_ok, components) = health_deep_assess(
        &state.session_db,
        state.http_stats.is_some(),
        Some(&token_snap),
    );
    let body = serde_json::json!({
        // Previously this object listed "status" twice; serde_json silently keeps
        // the last, so the response looked right by accident.
        "status": if all_ok { "ok" } else { "degraded" },
        "version": env!("CARGO_PKG_VERSION"),
        "components": components,
    });
    let code = if all_ok {
        StatusCode::OK
    } else {
        StatusCode::MULTI_STATUS
    };
    (code, Json(body))
}

// ── 内核执行事件 → SSE 转发 ───────────────────────────────────────────────

/// 把内核 `ExecutionEvent` 流转发到 SSE 广播器
///
/// 内核通过 `mpsc::UnboundedSender<ExecutionEvent>` 推送执行进度，
/// 该 channel 与 SSE 的 `broadcast::Sender<SseEvent>` 是两套独立通道，
/// 必须显式桥接，否则事件在发送端即被丢弃（`send` 返回 `Err`，
/// 不 panic、不记日志 —— 客户端只能看到「执行完了但没有任何过程」）。
///
/// `sop_id` 由调用方补齐：内核事件本身只带 `node_name`，不知道归属哪个 SOP。
///
/// 返回 JoinHandle；调用方 drop 掉 sender 后应 await 它，确保缓冲区事件
/// 已全部推送（直接 abort 会丢掉执行末尾的完成事件）。
fn spawn_sse_event_forwarder(
    broadcaster: tokio::sync::broadcast::Sender<crate::http::sse_handlers::SseEvent>,
    mut rx: mpsc::UnboundedReceiver<sisyphus_types::ExecutionEvent>,
    sop_id: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            let sse_event =
                crate::http::sse_handlers::SseEvent::from_execution_event(&event, &sop_id);
            // send 失败仅表示当前无 SSE 订阅者，属正常情况（没人订阅时
            // 不该让内核执行失败），因此只忽略错误继续消费。
            let _ = broadcaster.send(sse_event);
        }
    })
}

/// 按 `event_tx` 请求参数决定是否接线内核事件到 SSE
///
/// 返回 `(内核 sender, 转发器句柄)`；两者同时为 `None` 表示不转发。
///
/// 抽为独立函数的原因：这段接线本身就是缺陷所在 —— 旧实现只取
/// `unbounded_channel()` 的 sender 而丢弃 receiver，内核 `send` 立即
/// 返回 `Err`，事件被静默吞掉。若把它内联在 handler 里，测试就只能
/// 测到转发器本身，handler 是否真正调用它无从验证（变异退回旧写法时
/// 全部测试仍会通过）。
///
/// `event_tx` 语义：`None` 或 `Some(true)` 表示转发（内核已产生事件，
/// 丢弃反而是错的）；`Some(false)` 是调用方明确不要，才完全不转发。
fn wire_execution_events(
    broadcaster: tokio::sync::broadcast::Sender<crate::http::sse_handlers::SseEvent>,
    event_tx: Option<bool>,
    sop_id: String,
) -> (
    Option<mpsc::UnboundedSender<sisyphus_types::ExecutionEvent>>,
    Option<tokio::task::JoinHandle<()>>,
) {
    if event_tx == Some(false) {
        return (None, None);
    }
    let (tx, rx) = mpsc::unbounded_channel();
    (
        Some(tx),
        Some(spawn_sse_event_forwarder(broadcaster, rx, sop_id)),
    )
}

// --- Goal Execution Handlers ---

#[instrument(
    name = "http.execute_goal",
    skip(state, req, workspace),
    fields(goal_len = req.goal.len(), sop_id = %req.sop_id, project_id)
)]
pub(super) async fn execute_goal_handler(
    State(state): State<RestApiState>,
    workspace: WorkspaceContext,
    Json(req): Json<GoalExecuteRequest>,
) -> impl IntoResponse {
    let kernel = state.bridge.kernel();
    let sop_id = req.sop_id.clone();

    // 真正把内核执行事件转发到 SSE。旧实现只取 sender 而丢弃 receiver，
    // 内核 send 立即返回 Err，事件被静默吞掉（不 panic、不记日志），
    // 客户端传 `event_tx: true` 却收不到任何东西。
    let (event_tx, event_forwarder) =
        wire_execution_events(state.sse_broadcaster.clone(), req.event_tx, sop_id.clone());

    let project_id =
        workspace.project_id.or(req.project_id).unwrap_or_else(|| "default".to_string());
    tracing::Span::current().record("project_id", project_id.as_str());

    let result = kernel.execute_goal(&req.goal, &sop_id, &project_id, event_tx, None).await;

    // 等待转发器排空。event_tx 已随上面的调用 move 进内核，执行结束时内核
    // 侧的 sender 全部释放 → channel 关闭 → 转发器推完缓冲区后自行退出。
    // 必须 await 而非 abort，否则执行末尾的完成/失败事件会丢失
    // （这正是旧实现「客户端收不到任何东西」的一部分）。
    if let Some(handle) = event_forwarder {
        let _ = handle.await;
    }

    match result {
        Ok((output, report)) => (
            StatusCode::OK,
            match serde_json::to_value(GoalExecuteResponse { output, report }) {
                Ok(val) => Json(val),
                Err(_) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": "Failed to serialize response" })),
                    )
                },
            },
        ),
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

pub(super) async fn goal_status_handler(
    State(state): State<RestApiState>,
    Path(goal_id): Path<String>,
) -> impl IntoResponse {
    let kernel = state.bridge.kernel();
    match kernel.execution_manager.active_executions.get(&goal_id) {
        Some(status) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": status.status,
                "current_node": status.current_node,
                "progress": status.progress,
                "thought_logs": status.thought_logs,
                "last_updated": status.last_updated,
            })),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(
                serde_json::json!({ "error": format!("Goal execution {} not found or completed", goal_id) }),
            ),
        ),
    }
}

pub(super) async fn goal_trace_handler(
    State(state): State<RestApiState>,
    workspace: WorkspaceContext,
    Path(sop_id): Path<String>,
) -> impl IntoResponse {
    let project_id = workspace.project_id.unwrap_or_else(|| "default".to_string());
    let store = &state.bridge.state_store;
    match store.get_traces_for_sop(&project_id, &sop_id).await {
        Ok(rows) => {
            let events = rows
                .into_iter()
                .map(
                    |(node_id, status, output, fuel, duration)| TraceEventDetail {
                        node_id,
                        status,
                        output: Some(output),
                        fuel_consumed: fuel,
                        duration_ms: duration,
                    },
                )
                .collect();
            (StatusCode::OK, Json(GoalTraceResponse { sop_id, events })).into_response()
        },
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub(super) async fn cognitive_trace_handler(
    State(state): State<RestApiState>,
    Query(params): Query<CognitiveTraceQuery>,
) -> impl IntoResponse {
    const MAX_LIMIT: usize = 200;
    let limit = params.limit.unwrap_or(100).clamp(1, MAX_LIMIT);
    let entries = state.bridge.kernel().execution_manager.cognitive_trace.recent(limit);
    (StatusCode::OK, Json(CognitiveTraceResponse { entries })).into_response()
}

/// 认知决策历史（SQL 持久化，跨会话），新在前。
pub(super) async fn cognitive_trace_history_handler(
    State(state): State<RestApiState>,
    workspace: WorkspaceContext,
    Query(params): Query<CognitiveTraceHistoryQuery>,
) -> impl IntoResponse {
    const MAX_LIMIT: usize = 1000;
    let project_id = workspace.project_id.unwrap_or_else(|| "default".to_string());
    let limit = params.limit.unwrap_or(100).clamp(1, MAX_LIMIT);
    match state
        .bridge
        .kernel()
        .execution_manager
        .cognitive_trace_history(&project_id, params.sop_id.as_deref(), limit)
        .await
    {
        Ok(entries) => (StatusCode::OK, Json(CognitiveTraceResponse { entries })).into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// 认知决策健康摘要（跨会话聚合），用于评估循环稳定性与收敛质量。
pub(super) async fn cognitive_trace_summary_handler(
    State(state): State<RestApiState>,
    workspace: WorkspaceContext,
    Query(params): Query<CognitiveTraceSummaryQuery>,
) -> impl IntoResponse {
    const MAX_LIMIT: usize = 1000;
    let project_id = workspace.project_id.unwrap_or_else(|| "default".to_string());
    let limit = params.limit.unwrap_or(1000).clamp(1, MAX_LIMIT);
    match state
        .bridge
        .kernel()
        .execution_manager
        .cognitive_trace_summary(&project_id, params.sop_id.as_deref(), limit)
        .await
    {
        Ok(summary) => (StatusCode::OK, Json(summary)).into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub(super) async fn goal_stream_handler(
    State(state): State<RestApiState>,
    workspace: WorkspaceContext,
    Path(goal): Path<String>,
) -> impl IntoResponse {
    let project_id = workspace.project_id.unwrap_or_else(|| "default".to_string());
    let (tx, rx) = mpsc::unbounded_channel::<sisyphus_types::ExecutionEvent>();
    let bridge_clone = state.bridge.clone();
    let goal_clone = goal.clone();
    let pid_clone = project_id.clone();

    tokio::spawn(async move {
        if let Err(e) = bridge_clone
            .kernel()
            .execute_goal_streaming(&goal_clone, "stream_task", &pid_clone, Some(tx), None)
            .await
        {
            tracing::error!(error = %e, "Goal streaming execution failed");
        }
    });

    let stream = stream::unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Some(event) => {
                let data = format!("data: {:?}\n\n", event);
                Some((Ok::<_, std::io::Error>(data.into_bytes()), rx))
            },
            None => None,
        }
    });

    match Response::builder()
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .body(Body::from_stream(stream))
    {
        Ok(res) => res,
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Response build failed: {e}"),
        )
            .into_response(),
    }
}

pub(super) async fn execute_composite_handler(
    State(state): State<RestApiState>,
    workspace: WorkspaceContext,
    Json(req): Json<CompositeExecuteRequest>,
) -> impl IntoResponse {
    let kernel = state.bridge.kernel();

    // composite 执行多个 goal，没有单一 sop_id 可归属。这里用请求级标识
    // 填充 sop_id 维度，订阅方以 `?sop_id=composite-<uuid>` 过滤本次执行。
    // 注意：单个 goal 的执行进度无法按其自身 sop_id 过滤 —— 复合图内
    // 各图由内核内部生成 sop_id，不在 REST 层可见。若需按单图过滤，
    // 应改用 `/api/execute/goal` 逐个执行。
    let composite_sop_id = format!("composite-{}", uuid::Uuid::new_v4());

    // 与 execute_goal 同样的接线：保留 receiver，否则事件在发送端被丢弃。
    let (event_tx, event_forwarder) = wire_execution_events(
        state.sse_broadcaster.clone(),
        req.event_tx,
        composite_sop_id.clone(),
    );

    let project_id = workspace.project_id.or(req.project_id);

    // 此前这里为每个 goal 硬塞 `SOPGraphConfig::default()`（空图），
    // 而 `SOPGraph::from_config` 拒绝零节点图，因此该端点在生产路径上
    // **永远** 返回 500。改为复用内核的合成管线：每个 goal 经
    // `synthesize_composite` 生成真实 SOP 图，再交由 `execute_composite`
    // 按依赖顺序执行。
    let goals = req.goals;
    let goal_refs: Vec<&str> = goals.iter().map(String::as_str).collect();
    let project = project_id.as_deref().unwrap_or("default");

    let result = kernel.execute_goals_with_events(&goal_refs, project, event_tx.clone()).await;

    // 等待转发器排空，确保执行末尾事件送达（理由同 execute_goal）
    drop(event_tx);
    if let Some(handle) = event_forwarder {
        let _ = handle.await;
    }

    match result {
        Ok(output) => {
            let response = CompositeExecuteResponse {
                results: output.results.into_iter().map(|(id, (out, _))| (id, out)).collect(),
                execution_order: output.execution_order,
                // 告知客户端本次执行的事件归属标识，否则它无从知道
                // 该用哪个 sop_id 去 /api/events 订阅
                sse_sop_id: Some(composite_sop_id),
            };
            match serde_json::to_value(response) {
                Ok(val) => (StatusCode::OK, Json(val)),
                Err(_) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "Failed to serialize response" })),
                ),
            }
        },
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            // 失败时事件已经广播出去了（转发器在 match 之前就已排空），
            // 而 composite 的 sop_id 由服务端生成、客户端无从推算。因此
            // 失败响应同样必须回传 `sse_sop_id`，否则客户端收到了事件却
            // 不知道该用哪个 sop_id 去订阅，事件流等于凭空消失。
            //
            // 对比 execute_goal：它的 sop_id 由客户端提供，失败时客户端
            // 本就知道，无需回传 —— 这是两条路径的实质差异。
            Json(serde_json::json!({
                "error": e.to_string(),
                "sse_sop_id": composite_sop_id,
            })),
        ),
    }
}

pub(super) async fn list_sops_handler(
    State(state): State<RestApiState>,
    workspace: WorkspaceContext,
) -> impl IntoResponse {
    let kernel = state.bridge.kernel();
    match kernel.execution_manager.list_sops(workspace.project_id.as_deref()).await {
        Ok(sops) => (StatusCode::OK, Json(sops)).into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

pub(super) async fn get_sop_handler(
    State(state): State<RestApiState>,
    workspace: WorkspaceContext,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let kernel = state.bridge.kernel();
    let project_id = match workspace.project_id {
        Some(pid) => pid,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "Missing X-Sisyphus-Workspace-Id header",
            )
                .into_response()
        },
    };
    match kernel.execution_manager.load_sop(&id, &project_id).await {
        Ok(sop) => {
            let config = SOPGraphConfig::from_graph(&sop).unwrap_or_default();
            (
                StatusCode::OK,
                Json(SopDetailResponse {
                    id,
                    name: "Sisyphus SOP".into(), // Name is not currently in SOPGraph
                    version: Some("1.0.0".into()),
                    config,
                }),
            )
                .into_response()
        },
        Err(e) => (StatusCode::NOT_FOUND, Json(e.to_string())).into_response(),
    }
}

pub(super) async fn save_sop_handler(
    State(state): State<RestApiState>,
    workspace: WorkspaceContext,
    Json(req): Json<SopSaveRequest>,
) -> impl IntoResponse {
    let project_id = match workspace.project_id {
        Some(pid) => pid,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "Missing X-Sisyphus-Workspace-Id header",
            )
                .into_response()
        },
    };
    let sop_store = &state.bridge.kernel().execution_manager.sop_store;
    match sop_store.save_sop(&req.id, &project_id, &req.name, req.version, &req.config).await {
        Ok(_) => StatusCode::CREATED.into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_error(&e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

pub(super) async fn delete_sop_handler(
    State(state): State<RestApiState>,
    workspace: WorkspaceContext,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let project_id = match workspace.project_id {
        Some(pid) => pid,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "Missing X-Sisyphus-Workspace-Id header",
            )
                .into_response()
        },
    };
    let sop_store = &state.bridge.kernel().execution_manager.sop_store;
    match sop_store.delete_sop(&id, &project_id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_error(&e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

pub(super) async fn experience_similar_handler(
    State(state): State<RestApiState>,
    Json(req): Json<ExperienceQueryRequest>,
) -> impl IntoResponse {
    let kernel = state.bridge.kernel();
    let query = sisyphus_types::experience::ExperienceQuery {
        query_text: req.query_text,
        context_hash: String::new(),
        project_id: req.project_id.filter(|p| !p.trim().is_empty()),
        action_id: req.action_id,
        visual_embedding: None,
        limit: req.limit.unwrap_or(5),
    };
    match kernel.experience_bank.query_similar(query).await {
        Ok(exps) => (
            StatusCode::OK,
            Json(ExperienceSimilarResponse { experiences: exps }),
        )
            .into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

pub(super) async fn strategy_weights_handler(
    State(state): State<RestApiState>,
) -> impl IntoResponse {
    // v0.4.0 Phase B（ADR-017）：返回本节点 + 已收到的对端策略快照的聚合视图。
    // 共享视图挂在 MySisyphusBridge 上（而非 RestApiState），避免改动其构造点。
    // 读守卫在语句结束即释放：`to_json()` 返回自有值，不存在跨 await 持有。
    let body = state.bridge.strategy_weights().read().to_json();
    (StatusCode::OK, Json(body)).into_response()
}

pub(super) async fn cluster_status_handler(State(state): State<RestApiState>) -> impl IntoResponse {
    // Phase 6.1：只读协调状态快照，不改变传播/合并/决策路径。
    let cluster_cfg = state.bridge.cluster_config();
    let (peers_json, peer_view) = match state.bridge.peer_manager() {
        Some(pm) => {
            let now = std::time::Instant::now();
            let peers: Vec<serde_json::Value> = pm
                .list_peers()
                .into_iter()
                .map(|p| {
                    let last_seen_secs_ago =
                        pm.last_seen(&p.node_id).map(|t| now.duration_since(t).as_secs());
                    serde_json::json!({
                        "node_id": p.node_id,
                        "grpc_addr": p.grpc_addr,
                        "joined_at": p.joined_at,
                        "last_seen_secs_ago": last_seen_secs_ago,
                    })
                })
                .collect();
            let view = serde_json::json!({
                "self_node_id": pm.self_node_id(),
                "leader_id": pm.leader_id(),
                "is_leader": pm.is_leader(),
                "cluster_size": pm.cluster_size(),
                "state_proposal_broadcasts": pm.state_proposal_broadcasts(),
                "experience_sync": {
                    "broadcasts": pm.experience_sync_broadcasts(),
                    "inbound_delivered": pm.experience_sync_inbound_delivered(),
                    "inbound_applied": pm.experience_sync_inbound_applied(),
                    "inbound_rejected": pm.experience_sync_inbound_rejected(),
                    "inbound_rejected_deserialize": pm.experience_sync_inbound_rejected_deserialize(),
                    "inbound_rejected_quality": pm.experience_sync_inbound_rejected_quality(),
                    "inbound_rejected_apply": pm.experience_sync_inbound_rejected_apply(),
                },
            });
            (serde_json::Value::Array(peers), view)
        },
        None => (
            serde_json::Value::Array(Vec::new()),
            serde_json::json!({
                "self_node_id": null,
                "leader_id": null,
                "is_leader": false,
                "cluster_size": 1,
                "state_proposal_broadcasts": 0,
                "experience_sync": {
                    "broadcasts": 0,
                    "inbound_delivered": 0,
                    "inbound_applied": 0,
                    "inbound_rejected": 0,
                    "inbound_rejected_deserialize": 0,
                    "inbound_rejected_quality": 0,
                    "inbound_rejected_apply": 0,
                },
            }),
        ),
    };

    // Consensus trait view（async）；生产装配为字典序 leader / proposer 可能为 None。
    let consensus_leader = state.bridge.consensus.get_leader().await;
    let consensus_is_leader = state.bridge.consensus.is_leader().await;

    let body = serde_json::json!({
        "enabled": cluster_cfg.enabled,
        "cluster_name": cluster_cfg.cluster_name,
        "config": {
            "node_id": cluster_cfg.node_id,
            "peer_ttl_secs": cluster_cfg.peer_ttl_secs,
            "join_timeout_secs": cluster_cfg.join_timeout_secs,
            "strategy_sync_interval_secs": cluster_cfg.strategy_sync_interval_secs,
            "static_peers": cluster_cfg.peers,
        },
        "peer_manager": peer_view,
        "consensus": {
            "leader_id": consensus_leader,
            "is_leader": consensus_is_leader,
        },
        "peers": peers_json,
    });
    (StatusCode::OK, Json(body)).into_response()
}

pub(super) async fn roadmap_project_handler(
    State(state): State<RestApiState>,
    method: axum::http::Method,
    Path(id): Path<String>,
    payload: Option<Json<CreateProjectRequest>>,
) -> impl IntoResponse {
    let rm = &state.bridge.kernel().roadmap_manager;
    match method {
        axum::http::Method::GET => match rm.get_project(&id).await {
            Ok(Some(p)) => (StatusCode::OK, Json(p)).into_response(),
            Ok(None) => StatusCode::NOT_FOUND.into_response(),
            Err(e) => (
                crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
                Json(e.to_string()),
            )
                .into_response(),
        },
        axum::http::Method::POST => {
            let Some(Json(req)) = payload else {
                return (StatusCode::BAD_REQUEST, "Missing payload").into_response();
            };
            match rm.create_project(&req.id, &req.name, &req.description).await {
                Ok(p) => (StatusCode::CREATED, Json(p)).into_response(),
                Err(e) => (
                    crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
                    Json(e.to_string()),
                )
                    .into_response(),
            }
        },
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

pub(super) async fn roadmap_milestone_handler(
    State(state): State<RestApiState>,
    Json(req): Json<AddMilestoneRequest>,
) -> impl IntoResponse {
    match state.bridge.kernel().roadmap_manager.add_milestone(&req.project_id, req.milestone).await
    {
        Ok(_) => StatusCode::CREATED.into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

pub(super) async fn roadmap_phase_handler(
    State(state): State<RestApiState>,
    Json(req): Json<AddPhaseRequest>,
) -> impl IntoResponse {
    match state
        .bridge
        .kernel()
        .roadmap_manager
        .add_phase(&req.project_id, &req.milestone_id, req.phase)
        .await
    {
        Ok(_) => StatusCode::CREATED.into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

pub(super) async fn roadmap_goal_handler(
    State(state): State<RestApiState>,
    Path(goal_id): Path<String>,
    method: axum::http::Method,
    payload: Option<Json<UpdateGoalStatusRequest>>,
) -> impl IntoResponse {
    let rm = &state.bridge.kernel().roadmap_manager;
    match method {
        axum::http::Method::PATCH => {
            let Some(Json(req)) = payload else {
                return (StatusCode::BAD_REQUEST, "Missing payload").into_response();
            };
            match rm
                .update_goal_status(
                    &req.project_id,
                    &req.milestone_id,
                    &req.phase_id,
                    &goal_id,
                    req.status,
                )
                .await
            {
                Ok(_) => StatusCode::OK.into_response(),
                Err(e) => (
                    crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
                    Json(e.to_string()),
                )
                    .into_response(),
            }
        },
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

pub(super) async fn roadmap_health_handler(
    State(state): State<RestApiState>,
    Path(project_id): Path<String>,
) -> impl IntoResponse {
    match state.bridge.kernel().roadmap_manager.evaluate_health(&project_id).await {
        Ok(health) => (StatusCode::OK, Json(health)).into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

// --- Roadmap Dashboard Handler ---

/// Aggregate a project's roadmap dashboard view: project metadata,
/// milestone/phase/goal structure, computed progress, and health snapshot.
///
/// Web dashboard 前端一次拉齐「项目 + 里程碑 + 进度 + 健康」四个信号，
/// 替代前端自行跨端点拼装。
pub(super) async fn roadmap_dashboard_handler(
    State(state): State<RestApiState>,
    Path(project_id): Path<String>,
) -> impl IntoResponse {
    let rm = &state.bridge.kernel().roadmap_manager;
    let project = match rm.get_project(&project_id).await {
        Ok(Some(p)) => p,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            return (
                crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
                Json(e.to_string()),
            )
                .into_response();
        },
    };

    let milestones = project.roadmap.milestones.clone();
    let progress = rm.calculate_progress(&project_id).await.unwrap_or(0.0);
    let health = rm.evaluate_health(&project_id).await.ok();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "project": project,
            "milestones": milestones,
            "progress": progress,
            "health": health,
        })),
    )
        .into_response()
}

// --- Roadmap Prediction Handlers ---

pub(super) async fn predict_completion_handler(
    State(state): State<RestApiState>,
    Json(req): Json<PredictCompletionRequest>,
) -> impl IntoResponse {
    match state.bridge.kernel().roadmap_manager.predict_completion_time(&req.project_id).await {
        Ok(prediction) => (StatusCode::OK, Json(prediction)).into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

pub(super) async fn check_delay_alerts_handler(
    State(state): State<RestApiState>,
    Json(req): Json<CheckDelayAlertsRequest>,
) -> impl IntoResponse {
    match state
        .bridge
        .kernel()
        .roadmap_manager
        .check_delay_alerts(&req.project_id, req.threshold_multiplier)
        .await
    {
        Ok(alerts) => (StatusCode::OK, Json(DelayAlertsResponse { alerts })).into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

// --- Strategic Proposal Handlers ---

pub(super) async fn project_costs_handler(
    State(state): State<RestApiState>,
    Json(req): Json<ProjectCostsRequest>,
) -> impl IntoResponse {
    match state
        .bridge
        .kernel()
        .roadmap_manager
        .project_costs(&req.project_id, req.total_goals, req.completed_goals)
        .await
    {
        Ok(projection) => (StatusCode::OK, Json(projection)).into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

pub(super) async fn find_bottlenecks_handler(
    State(state): State<RestApiState>,
    Json(req): Json<FindBottlenecksRequest>,
) -> impl IntoResponse {
    match state.bridge.kernel().roadmap_manager.find_bottlenecks(&req.project_id).await {
        Ok(bottlenecks) => (
            StatusCode::OK,
            Json(FindBottlenecksResponse { bottlenecks }),
        )
            .into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

pub(super) async fn suggest_parallel_execution_handler(
    State(state): State<RestApiState>,
    Json(req): Json<SuggestParallelExecutionRequest>,
) -> impl IntoResponse {
    match state.bridge.kernel().roadmap_manager.suggest_parallel_execution(&req.project_id).await {
        Ok(opportunities) => (
            StatusCode::OK,
            Json(SuggestParallelExecutionResponse { opportunities }),
        )
            .into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

pub(super) async fn estimate_goal_resources_handler(
    State(state): State<RestApiState>,
    Json(req): Json<EstimateGoalResourcesRequest>,
) -> impl IntoResponse {
    match state
        .bridge
        .kernel()
        .roadmap_manager
        .estimate_goal_resources(&req.project_id, &req.goal_ids)
        .await
    {
        Ok(estimates) => (
            StatusCode::OK,
            Json(EstimateGoalResourcesResponse { estimates }),
        )
            .into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

// --- Strategic Proposal Handlers ---

pub(super) async fn list_proposals_handler(
    State(state): State<RestApiState>,
    Path(project_id): Path<String>,
) -> impl IntoResponse {
    match state.bridge.kernel().roadmap_manager.proposal_manager().list_proposals(&project_id).await
    {
        Ok(proposals) => (StatusCode::OK, Json(ProposalListResponse { proposals })).into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

pub(super) async fn get_proposal_handler(
    State(state): State<RestApiState>,
    Path((project_id, proposal_id)): Path<(String, String)>,
) -> impl IntoResponse {
    match state
        .bridge
        .kernel()
        .roadmap_manager
        .proposal_manager()
        .get_proposal(&project_id, &proposal_id)
        .await
    {
        Ok(Some(proposal)) => (StatusCode::OK, Json(proposal)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Proposal not found" })),
        )
            .into_response(),
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

pub(super) async fn approve_proposal_handler(
    State(state): State<RestApiState>,
    Json(req): Json<ProposalActionRequest>,
) -> impl IntoResponse {
    match state
        .bridge
        .kernel()
        .roadmap_manager
        .proposal_manager()
        .approve_proposal(&req.project_id, &req.proposal_id, &req.reviewer)
        .await
    {
        Ok(_) => {
            // Log audit entry
            tracing::info!(
                project_id = %req.project_id,
                proposal_id = %req.proposal_id,
                reviewer = %req.reviewer,
                action = "approve",
                "Proposal approved"
            );
            StatusCode::OK.into_response()
        },
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

pub(super) async fn reject_proposal_handler(
    State(state): State<RestApiState>,
    Json(req): Json<ProposalActionRequest>,
) -> impl IntoResponse {
    match state
        .bridge
        .kernel()
        .roadmap_manager
        .proposal_manager()
        .reject_proposal(&req.project_id, &req.proposal_id, &req.reviewer)
        .await
    {
        Ok(_) => {
            // Log audit entry
            tracing::info!(
                project_id = %req.project_id,
                proposal_id = %req.proposal_id,
                reviewer = %req.reviewer,
                action = "reject",
                "Proposal rejected"
            );
            StatusCode::OK.into_response()
        },
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(e.to_string()),
        )
            .into_response(),
    }
}

pub(super) async fn execute_handler(
    State(state): State<RestApiState>,
    Json(req): Json<ExecuteRequest>,
) -> impl IntoResponse {
    let fuel = if req.fuel_limit > 0 {
        req.fuel_limit
    } else {
        state.bridge.default_fuel()
    };
    let args_bytes = req.args_json.into_bytes();
    match state
        .bridge
        .kernel()
        .execution_manager
        .tool_router
        .execute(&req.tool_id, args_bytes, fuel)
        .await
    {
        Ok(bytes) => {
            let result_str = String::from_utf8_lossy(&bytes).to_string();
            (
                StatusCode::OK,
                Json(ExecuteResponse {
                    status: "SUCCESS".into(),
                    result_json: result_str,
                }),
            )
        },
        Err(e) => (
            crate::http::handlers_platform::status_for_typed(Some(&e), &e.to_string()),
            Json(ExecuteResponse {
                status: format!("ERROR: {e}"),
                result_json: String::new(),
            }),
        ),
    }
}

pub(super) async fn list_sessions_handler(State(state): State<RestApiState>) -> impl IntoResponse {
    match sisyphus_tools_session::session_list(&state.session_db) {
        Ok(sessions) => {
            let summary: Vec<SessionSummary> = sessions
                .into_iter()
                .map(|s| SessionSummary {
                    id: s.id,
                    name: s.name,
                    message_count: s.message_count,
                    created_at: s.created_at,
                    updated_at: s.updated_at,
                })
                .collect();
            (StatusCode::OK, Json(summary))
        },
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Vec::<SessionSummary>::new()),
        ),
    }
}

pub(super) async fn create_session_handler(
    State(state): State<RestApiState>,
    Json(req): Json<CreateSessionRequest>,
) -> impl IntoResponse {
    match sisyphus_tools_session::session_create(&state.session_db, &req.name) {
        Ok(session) => {
            let detail = SessionDetail {
                id: session.id,
                name: session.name,
                message_count: session.message_count,
                created_at: session.created_at,
                updated_at: session.updated_at,
                messages: session
                    .messages
                    .into_iter()
                    .map(|m| MessageDto {
                        role: m.role,
                        content: m.content,
                        created_at: m.created_at,
                        estimated_tokens: m.estimated_tokens,
                    })
                    .collect(),
            };
            (StatusCode::CREATED, Json(detail))
        },
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(SessionDetail {
                id: String::new(),
                name: String::new(),
                message_count: 0,
                created_at: String::new(),
                updated_at: String::new(),
                messages: vec![],
            }),
        ),
    }
}

pub(super) async fn get_session_handler(
    State(state): State<RestApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match sisyphus_tools_session::session_get(&state.session_db, &id) {
        Ok(session) => {
            let detail = SessionDetail {
                id: session.id,
                name: session.name,
                message_count: session.message_count,
                created_at: session.created_at,
                updated_at: session.updated_at,
                messages: session
                    .messages
                    .into_iter()
                    .map(|m| MessageDto {
                        role: m.role,
                        content: m.content,
                        created_at: m.created_at,
                        estimated_tokens: m.estimated_tokens,
                    })
                    .collect(),
            };
            (StatusCode::OK, Json(detail))
        },
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(SessionDetail {
                id: String::new(),
                name: String::new(),
                message_count: 0,
                created_at: String::new(),
                updated_at: String::new(),
                messages: vec![],
            }),
        ),
    }
}

pub(super) async fn add_message_handler(
    State(state): State<RestApiState>,
    Path(id): Path<String>,
    Json(req): Json<AddMessageRequest>,
) -> impl IntoResponse {
    match sisyphus_tools_session::session_add_message(
        &state.session_db,
        &id,
        &req.role,
        &req.content,
    ) {
        Ok(session) => {
            let detail = SessionDetail {
                id: session.id,
                name: session.name,
                message_count: session.message_count,
                created_at: session.created_at,
                updated_at: session.updated_at,
                messages: session
                    .messages
                    .into_iter()
                    .map(|m| MessageDto {
                        role: m.role,
                        content: m.content,
                        created_at: m.created_at,
                        estimated_tokens: m.estimated_tokens,
                    })
                    .collect(),
            };
            (StatusCode::OK, Json(detail))
        },
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(SessionDetail {
                id: String::new(),
                name: String::new(),
                message_count: 0,
                created_at: String::new(),
                updated_at: String::new(),
                messages: vec![],
            }),
        ),
    }
}

pub(super) async fn list_tools_handler(State(state): State<RestApiState>) -> impl IntoResponse {
    let tools = state.bridge.tool_registry().list();
    let entries: Vec<ToolEntry> = tools
        .into_iter()
        .map(|(tool_id, info)| {
            let source = match &info.source {
                sisyphus_tools_mcp::tool_registry::ToolSource::Builtin => "builtin",
                sisyphus_tools_mcp::tool_registry::ToolSource::Mcp { .. } => "mcp",
                sisyphus_tools_mcp::tool_registry::ToolSource::Strategic => "strategic",
                sisyphus_tools_mcp::tool_registry::ToolSource::Wasm => "wasm",
            };
            ToolEntry {
                tool_id,
                name: info.name,
                description: info.description,
                source: source.into(),
            }
        })
        .collect();
    (StatusCode::OK, Json(entries))
}

pub(super) async fn list_agents_handler(State(state): State<RestApiState>) -> impl IntoResponse {
    let agents = state.bridge.agent_registry().list();
    let entries: Vec<AgentEntry> = agents
        .into_iter()
        .map(|(agent_id, info)| {
            let description = if !info.tool_prefixes.is_empty() {
                let tools = state.bridge.tool_registry().list_for_prefixes(&info.tool_prefixes);
                let tool_names: Vec<&str> = tools.iter().map(|(_, t)| t.name.as_str()).collect();
                if tool_names.is_empty() {
                    info.description
                } else {
                    format!("{} | Tools: {}", info.description, tool_names.join(", "))
                }
            } else {
                info.description
            };
            AgentEntry {
                agent_id,
                display_name: info.display_name,
                description,
                tool_prefixes: info.tool_prefixes,
                max_concurrency: info.max_concurrency as u32,
            }
        })
        .collect();
    (StatusCode::OK, Json(entries))
}

pub(super) async fn list_skills_handler(State(state): State<RestApiState>) -> impl IntoResponse {
    let skills = state.bridge.skill_loader().load_all();
    let entries: Vec<SkillEntry> = skills
        .into_iter()
        .map(|s| SkillEntry {
            name: s.name,
            description: s.description,
            version: s.version,
            author: s.author.unwrap_or_default(),
            tags: s.tags,
            source: match s.source {
                sisyphus_tools_session::skill::SkillSource::BuiltIn => "builtin",
                sisyphus_tools_session::skill::SkillSource::User => "user",
                sisyphus_tools_session::skill::SkillSource::Project => "project",
            }
            .into(),
        })
        .collect();
    (StatusCode::OK, Json(entries))
}

// =====================================================================
// 实时认知地图 SSE 流（EVOLUTION_PLAN §3.2 第一步）
// ---------------------------------------------------------------------
// 端点：`GET /api/cognitive/map/:sop_id/stream`
//
// 行为：
// 1. 客户端订阅后，server 立即推送一次当前 CogMap（如果存在）
// 2. 之后每 `poll_ms`（默认 500ms）轮询 `cognitive_maps`，
//    若 iteration 变化或 active_node 变化则推送 `update` 事件
// 3. 推送 `heartbeat` 事件保持连接活跃
// 4. 客户端断开则退出循环
//
// 端点位置与现有 `/api/cognitive/map/:sop_id` 互补：一次性查询用前者，
// 实时观察用后者。
pub(super) async fn stream_cognitive_map_handler(
    State(state): State<RestApiState>,
    Path(sop_id): Path<String>,
) -> impl IntoResponse {
    use axum::response::sse::{Event, KeepAlive, Sse};

    use std::time::Duration;

    let kernel = state.bridge.kernel().clone();
    let sop_id_owned = sop_id.clone();

    let sse_stream = async_stream::stream! {
        // 第一次立即推送（如果存在）
        let initial = kernel
            .execution_manager
            .cognitive_maps
            .get(&sop_id_owned)
            .map(|m| m.clone());
        match initial {
            Some(map) => {
                let payload = serde_json::to_string(&map).unwrap_or_default();
                yield Ok::<Event, std::convert::Infallible>(
                    Event::default()
                        .event("snapshot")
                        .data(payload),
                );
            }
            None => {
                // 必须用 serde_json 序列化而非 format! 拼接：sop_id 来自 URL 路径，
                // 含引号/反斜杠/控制字符时会产出**非法 JSON**，客户端 JSON.parse
                // 直接抛异常（webui 的 catch 是空的，表现为静默失败）。
                let payload = serde_json::json!({
                    "sop_id": sop_id_owned,
                    "message": "No active cognitive map",
                })
                .to_string();
                yield Ok::<Event, std::convert::Infallible>(
                    Event::default().event("absent").data(payload),
                );
            }
        }

        // 轮询循环：仅在 iteration / active_node 变化时推送
        let mut last_iteration: Option<usize> = None;
        let mut last_active: Option<Option<String>> = None;
        let poll_interval = Duration::from_millis(500);
        let mut heartbeat = std::time::Instant::now();
        let heartbeat_interval = Duration::from_secs(15);

        loop {
            tokio::time::sleep(poll_interval).await;

            let current = kernel
                .execution_manager
                .cognitive_maps
                .get(&sop_id_owned)
                .map(|m| m.clone());

            if let Some(map) = current {
                let active = map.active_node.clone();
                let iter = map.iteration;
                if last_iteration != Some(iter) || last_active.as_ref() != Some(&active) {
                    last_iteration = Some(iter);
                    last_active = Some(active.clone());
                    let payload = serde_json::to_string(&map).unwrap_or_default();
                    yield Ok::<Event, std::convert::Infallible>(
                        Event::default()
                            .event("update")
                            .data(payload),
                    );
                }
            }

            if heartbeat.elapsed() >= heartbeat_interval {
                heartbeat = std::time::Instant::now();
                yield Ok::<Event, std::convert::Infallible>(
                    Event::default()
                        .event("heartbeat")
                        .data(format!(
                            r#"{{"sop_id":"{sop_id_owned}","ts":{}}}"#,
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs())
                                .unwrap_or(0)
                        )),
                );
            }
        }
    };

    Sse::new(Box::pin(sse_stream)).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

// ── 认知映射实时干预端点 (P1-2) ────────────────────────────────────────────

/// POST /api/cognitive/map/:sop_id/intervene
///
/// 实时干预正在运行的 SOP。请求体：
/// ```json
/// { "action": "abort" | "pause" | "resume" | "redirect", "target_node": "optional-node-id", "reason": "optional" }
/// ```
///
/// 实现：
/// 1. 通过 `sse_broadcaster` 广播 `intervention` SSE 事件，所有订阅此 sop_id 的客户端立即收到。
/// 2. 更新 `ExecutionManager.active_executions` 中的状态（abort → 移除条目；pause/resume → 更新 status；redirect → 记录目标节点）。
/// 3. 实时干预通过 `cogmap_subscribers` 通道触发内核的运行循环（若可用）。
#[derive(serde::Deserialize)]
pub(super) struct InterventionRequest {
    pub action: String,
    #[serde(default)]
    pub target_node: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

pub(super) async fn intervene_cognitive_map_handler(
    State(state): State<RestApiState>,
    Path(sop_id): Path<String>,
    Json(req): Json<InterventionRequest>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    let kernel = state.bridge.kernel().clone();
    let action = req.action.to_lowercase();

    // 1) 更新 active_executions（如果存在）
    let mut applied = false;
    match action.as_str() {
        "abort" => {
            // 直接移除条目；运行循环下次检测时退出
            if kernel.execution_manager.active_executions.remove(&sop_id).is_some() {
                applied = true;
            }
        },
        "pause" | "resume" => {
            if let Some(mut entry) = kernel.execution_manager.active_executions.get_mut(&sop_id) {
                entry.status = if action == "pause" {
                    "paused".to_string()
                } else {
                    "running".to_string()
                };
                applied = true;
            }
        },
        "redirect" => {
            if let Some(mut entry) = kernel.execution_manager.active_executions.get_mut(&sop_id) {
                if let Some(target) = &req.target_node {
                    entry.current_node = Some(target.clone());
                    applied = true;
                }
            }
        },
        _ => {},
    }

    // 2) 广播 SSE 事件给所有订阅者
    let payload = serde_json::json!({
        "sop_id": sop_id,
        "action": action,
        "target_node": req.target_node,
        "reason": req.reason,
        "applied": applied,
        "ts": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
    });
    let event = crate::http::sse_handlers::SseEvent::custom("intervention".to_string(), payload);
    let _ = state.sse_broadcaster.send(event);

    let status = if applied {
        StatusCode::OK
    } else {
        StatusCode::ACCEPTED // 状态已广播，但目标 sop 未在 active_executions 中（可能已完成）
    };

    (
        status,
        axum::Json(serde_json::json!({
            "sop_id": sop_id,
            "action": action,
            "applied": applied,
            "message": if applied { "intervention applied" } else { "intervention broadcast but no active execution found" },
        })),
    )
}

// ── K8s 标准健康度端点（阶段 8.2）────────────────────────
// liveness: 始终 200（进程能响应 HTTP 即存活）
// readiness: 200 当所有关键依赖 OK；503 当任一 degraded
// 用法（K8s Deployment.yaml）：
//   livenessProbe:  httpGet /health/live
//   readinessProbe: httpGet /health/ready

/// Liveness probe: 始终返回 200，只要 HTTP server 能响应即视为存活。
/// 不检查任何依赖（避免依赖抖动触发 pod 重启）。
pub(super) async fn health_live_handler() -> impl IntoResponse {
    let body = serde_json::json!({
        "status": "ok",
        "kind": "liveness",
        "version": env!("CARGO_PKG_VERSION"),
    });
    (StatusCode::OK, Json(body))
}

/// Readiness probe: 200 当所有关键依赖 OK；503 当任一 degraded。
/// 复用 `health_deep_assess`（无 token_budget 维度，保持轻量）。
pub(super) async fn health_ready_handler(State(state): State<RestApiState>) -> impl IntoResponse {
    let (all_ok, components) =
        health_deep_assess(&state.session_db, state.http_stats.is_some(), None);
    let body = serde_json::json!({
        "status": if all_ok { "ok" } else { "degraded" },
        "kind": "readiness",
        "version": env!("CARGO_PKG_VERSION"),
        "components": components,
    });
    let code = if all_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (code, Json(body))
}

// ---------------------------------------------------------------------------
// 单元测试：health_deep_assess 纯函数（无需完整桥接服务）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod wiring_tests {
    use super::wire_execution_events;
    use crate::http::sse_handlers::SseEvent;
    use sisyphus_types::ExecutionEvent;

    /// 核心回归测试：接线必须产生一个**活的**转发器，
    /// 内核通过返回的 sender 发送的事件必须真的到达 SSE 订阅者。
    ///
    /// 这正是旧实现失败的地方 —— 它只返回 sender 而丢弃 receiver，
    /// 内核 send 立即 Err。此测试若退回旧写法必然失败。
    #[tokio::test]
    async fn wired_sender_delivers_events_to_sse() {
        for (label, event_tx_param) in [("未传", None), ("显式 true", Some(true))] {
            let (sse_tx, mut sse_rx) = tokio::sync::broadcast::channel::<SseEvent>(16);
            let (kern_tx, handle) =
                wire_execution_events(sse_tx, event_tx_param, "sop-w".to_string());

            let kern_tx = kern_tx.expect(
                "event_tx 未传或为 true 时必须返回内核 sender —— \
                 旧实现虽返回 sender 但丢弃 receiver，事件无处可去",
            );
            kern_tx
                .send(ExecutionEvent::NodeStarted {
                    node_name: "n".to_string(),
                })
                .expect("返回的 sender 必须仍然有效（receiver 不得被丢弃）");

            let got = sse_rx.recv().await.expect("SSE 订阅者应收到事件");
            assert_eq!(got.event_type(), "node_started", "case: {label}");
            assert_eq!(got.sop_id(), Some("sop-w"), "case: {label}");

            drop(kern_tx);
            let handle = handle.expect("转发器句柄");
            tokio::time::timeout(std::time::Duration::from_secs(5), handle)
                .await
                .expect("转发器应退出")
                .expect("不得 panic");
        }
    }

    /// 显式 `event_tx: false` 是调用方明确不要，必须完全不建转发器。
    #[tokio::test]
    async fn explicit_false_disables_forwarding() {
        let (sse_tx, _rx) = tokio::sync::broadcast::channel::<SseEvent>(16);
        let (kern_tx, handle) = wire_execution_events(sse_tx, Some(false), "sop-off".to_string());

        assert!(kern_tx.is_none(), "event_tx=false 时不应创建内核 sender");
        assert!(handle.is_none(), "event_tx=false 时不应创建转发器");
    }

    /// 转发器句柄必须被 await（而非被丢弃/abort），否则执行末尾的事件会丢。
    /// 本测试确认：sender 释放后，转发器在有限时间内自行结束 —— 这是
    /// handler 里 `handle.await` 能够正常返回的前提。
    #[tokio::test]
    async fn forwarder_handle_resolves_after_sender_dropped() {
        let (sse_tx, mut sse_rx) = tokio::sync::broadcast::channel::<SseEvent>(16);
        let (kern_tx, handle) = wire_execution_events(sse_tx, None, "sop-q".to_string());
        let kern_tx = kern_tx.expect("应有 sender");

        kern_tx
            .send(ExecutionEvent::ExecutionComplete {
                result: sisyphus_types::NodeOutput::from(serde_json::json!("done")),
            })
            .expect("sender 应有效");

        // 末尾事件必须已经送达，handle 才可能结束
        let _ = sse_rx.recv().await.expect("应收到 execution_complete");

        let handle = handle.expect("转发器句柄");
        drop(kern_tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .expect("sender 释放后转发器必须自行退出，否则 handler 的 await 会挂起")
            .expect("不得 panic");
    }
}

#[cfg(test)]
mod sse_forwarder_tests {
    use super::spawn_sse_event_forwarder;
    use crate::http::sse_handlers::{matches_filters, SseEvent, SseFilters};
    use sisyphus_types::ExecutionEvent;
    use tokio::sync::mpsc;

    /// 端到端验证转发器：内核 mpsc 里的 ExecutionEvent 必须真的到达
    /// SSE 广播订阅者。旧实现丢弃 receiver，此链路完全不存在。
    #[tokio::test]
    async fn execution_events_reach_sse_subscribers() {
        let (sse_tx, mut sse_rx) = tokio::sync::broadcast::channel::<SseEvent>(32);
        let (kern_tx, kern_rx) = mpsc::unbounded_channel::<ExecutionEvent>();

        let handle = spawn_sse_event_forwarder(sse_tx, kern_rx, "sop-live".to_string());

        // 模拟内核在执行过程中推送的典型事件序列
        kern_tx
            .send(ExecutionEvent::NodeStarted {
                node_name: "fetch".to_string(),
            })
            .expect("转发器应仍在运行");
        kern_tx
            .send(ExecutionEvent::NodeCompleted {
                node_name: "fetch".to_string(),
                output: sisyphus_types::NodeOutput::from(serde_json::json!("rows")),
                fuel_consumed: 5,
                duration_ms: 50,
            })
            .expect("转发器应仍在运行");
        kern_tx
            .send(ExecutionEvent::ExecutionComplete {
                result: sisyphus_types::NodeOutput::from(serde_json::json!("all done")),
            })
            .expect("转发器应仍在运行");

        // 顺序与数量都必须保真
        let first = sse_rx.recv().await.expect("应收到 node_started");
        let second = sse_rx.recv().await.expect("应收到 node_completed");
        let third = sse_rx.recv().await.expect("应收到 execution_complete");

        assert_eq!(first.event_type(), "node_started");
        assert_eq!(second.event_type(), "node_completed");
        assert_eq!(third.event_type(), "execution_complete");

        // 每个事件都必须归属正确 sop，否则无法按 sop 订阅
        for ev in [&first, &second, &third] {
            assert_eq!(ev.sop_id(), Some("sop-live"));
            assert!(matches_filters(
                ev,
                &SseFilters {
                    sop_id: Some("sop-live".to_string()),
                    ..Default::default()
                }
            ));
            assert!(!matches_filters(
                ev,
                &SseFilters {
                    sop_id: Some("other-sop".to_string()),
                    ..Default::default()
                }
            ));
        }

        // 关闭 sender 后转发器应自行退出（不能泄漏 task）
        drop(kern_tx);
        tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .expect("转发器应在 channel 关闭后退出，不得挂起")
            .expect("转发器 task 不应 panic");
    }

    /// 无 SSE 订阅者时，转发器必须安静地丢弃事件而不是让内核执行失败。
    #[tokio::test]
    async fn forwarder_survives_absent_subscribers() {
        let (sse_tx, sse_rx) = tokio::sync::broadcast::channel::<SseEvent>(8);
        drop(sse_rx); // 模拟无人订阅

        let (kern_tx, kern_rx) = mpsc::unbounded_channel::<ExecutionEvent>();
        let handle = spawn_sse_event_forwarder(sse_tx, kern_rx, "sop-x".to_string());

        for _ in 0..20 {
            kern_tx
                .send(ExecutionEvent::NodeStarted {
                    node_name: "n".to_string(),
                })
                .expect("发送不应因无订阅者而中断");
        }
        drop(kern_tx);

        tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .expect("无订阅者时也应正常退出")
            .expect("不得 panic");
    }

    /// 转发器必须把缓冲区里已排队的事件全部推完才退出。
    /// 若实现为「收到即 abort」，执行末尾的完成事件会在客户端侧丢失。
    #[tokio::test]
    async fn forwarder_drains_buffered_events_before_exit() {
        let (sse_tx, mut sse_rx) = tokio::sync::broadcast::channel::<SseEvent>(64);
        let (kern_tx, kern_rx) = mpsc::unbounded_channel::<ExecutionEvent>();

        // 先灌满缓冲，再启动转发器 —— 事件必须一条不落地送达
        for _ in 0..10 {
            kern_tx
                .send(ExecutionEvent::NodeStarted {
                    node_name: "n".to_string(),
                })
                .expect("转发器未启动，channel 仍有容量");
        }
        let handle = spawn_sse_event_forwarder(sse_tx, kern_rx, "sop-drain".to_string());
        drop(kern_tx);

        let mut received = 0;
        while let Ok(_ev) = sse_rx.try_recv() {
            received += 1;
        }
        // 等转发器退出后再数，确保不是"还没推完"
        tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .expect("应退出")
            .expect("不得 panic");

        let mut total = received;
        while sse_rx.try_recv().is_ok() {
            total += 1;
        }
        assert_eq!(
            total, 10,
            "已排队的 10 个事件必须全部送达，实际只收到 {total} 个"
        );
    }
}

#[cfg(test)]
mod cognitive_map_payload_tests {
    /// `stream_cognitive_map_handler` 在无活跃映射时推送 `absent` 事件。
    /// sop_id 来自 URL 路径，构造 payload 时必须保证输出恒为合法 JSON。
    fn absent_payload(sop_id: &str) -> String {
        serde_json::json!({
            "sop_id": sop_id,
            "message": "No active cognitive map",
        })
        .to_string()
    }

    #[test]
    fn absent_payload_is_valid_json_for_hostile_sop_ids() {
        // 这些字符都能出现在 URL 路径中。用 format! 拼接会产出非法 JSON，
        // 客户端 JSON.parse 抛异常；webui 的 catch 是空的，表现为静默失败。
        for hostile in ["normal", "a\"b", "x\\y", "a\nb", "{\"evil\":1}", "中文项目"] {
            let raw = absent_payload(hostile);
            let parsed: serde_json::Value = serde_json::from_str(&raw)
                .unwrap_or_else(|e| panic!("sop_id={hostile:?} 产出非法 JSON {raw:?}: {e}"));

            // 不只是「能解析」——值必须原样往返，不能被截断或改写
            assert_eq!(
                parsed["sop_id"], hostile,
                "sop_id={hostile:?} 未原样往返，实际 {raw:?}"
            );
            assert_eq!(parsed["message"], "No active cognitive map");
        }
    }

    /// 回归防护：确认 format! 拼接确实会产生非法 JSON。
    /// 若本测试失败，说明该反例已不成立，可移除。
    #[test]
    fn format_string_concatenation_would_produce_invalid_json() {
        let hostile = "a\"b";
        let broken = format!(r#"{{"sop_id":"{hostile}","message":"No active cognitive map"}}"#);
        assert!(
            serde_json::from_str::<serde_json::Value>(&broken).is_err(),
            "format! 拼接本应产生非法 JSON，若现在合法说明反例需重新选择"
        );
    }
}

#[cfg(test)]
mod execution_event_tx_tests {
    use super::*;
    use sisyphus_types::ExecutionEvent;

    /// 当前 `execute_goal_handler` / `execute_composite_handler` 的写法：
    /// 只取 `unbounded_channel()` 的 sender，receiver 立即被丢弃。
    /// 本测试固定该行为的语义后果，作为接线前的诊断基线。
    #[test]
    fn dropping_receiver_silently_swallows_execution_events() {
        let tx = mpsc::unbounded_channel::<ExecutionEvent>().0;

        // receiver 已随 tuple 丢弃，send 返回 Err —— 事件被静默吞掉，
        // 调用方无法察觉，且不会 panic 或记录日志。
        let res = tx.send(ExecutionEvent::NodeStarted {
            node_name: "fetch".to_string(),
        });
        assert!(
            res.is_err(),
            "丢弃 receiver 后 send 必须失败，否则事件流向不明"
        );

        // 后续事件同样静默失败，说明这是稳定的丢失而非偶发
        assert!(tx
            .send(ExecutionEvent::NodeCompleted {
                node_name: "fetch".to_string(),
                output: sisyphus_types::NodeOutput::from(serde_json::json!("ok")),
                fuel_consumed: 0,
                duration_ms: 1,
            })
            .is_err());
    }

    /// 保留 receiver 时事件可被消费 —— 与上一测试对照，说明缺陷在丢弃而非发送端。
    #[tokio::test]
    async fn retained_receiver_receives_execution_events() {
        let (tx, mut rx) = mpsc::unbounded_channel::<ExecutionEvent>();
        tx.send(ExecutionEvent::NodeStarted {
            node_name: "fetch".to_string(),
        })
        .expect("receiver 存活时 send 应成功");

        let got = rx.recv().await.expect("应收到事件");
        match got {
            ExecutionEvent::NodeStarted { node_name } => assert_eq!(node_name, "fetch"),
            other => panic!("期望 NodeStarted，实际: {other:?}"),
        }
    }
}

#[cfg(test)]
mod health_deep_tests {
    use super::health_deep_assess;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 每次测试分配一个唯一子目录，避免与并行测试冲突。
    /// 测试结束后用 best-effort 方式清理临时文件。
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_tmp_dir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("sisyphus_health_deep_test_{pid}_{n}"));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn cleanup(dir: &std::path::Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn session_db_file_present_is_ok() {
        let dir = unique_tmp_dir();
        let db = dir.join("sessions.db");
        std::fs::write(&db, b"").unwrap();
        let (all_ok, components) = health_deep_assess(&db, true, None);
        assert!(all_ok);
        assert_eq!(components["session_db"]["status"], "ok");
        assert_eq!(components["shared_http"]["status"], "ok");
        assert_eq!(components["kernel"]["status"], "ok");
        cleanup(&dir);
    }

    #[test]
    fn session_db_parent_exists_is_ok() {
        // DB 未创建但父目录存在 → ok（首次启动场景）
        let dir = unique_tmp_dir();
        let (all_ok, components) = health_deep_assess(&dir.join("nonexistent.db"), true, None);
        assert!(all_ok);
        assert_eq!(components["session_db"]["status"], "ok");
        cleanup(&dir);
    }

    #[test]
    fn session_db_path_nonexistent_parent_is_degraded() {
        // 父目录不存在 → degraded
        let db = PathBuf::from("/this/path/should/never/exist/abc/sessions.db");
        let (all_ok, components) = health_deep_assess(&db, true, None);
        assert!(!all_ok);
        assert_eq!(components["session_db"]["status"], "degraded");
    }

    #[test]
    fn shared_http_absent_does_not_count_as_degraded() {
        // CLI/dev 模式无 LLM 客户端：absent 不触发 degraded
        let dir = unique_tmp_dir();
        let db = dir.join("sessions.db");
        std::fs::write(&db, b"").unwrap();
        let (all_ok, components) = health_deep_assess(&db, false, None);
        assert!(all_ok, "absent shared_http must not degrade overall status");
        assert_eq!(components["shared_http"]["status"], "absent");
        cleanup(&dir);
    }
    #[test]
    fn components_map_has_three_required_keys() {
        let (all_ok, components) = health_deep_assess(
            std::path::Path::new("/tmp/should_work/db.sqlite"),
            false,
            None,
        );
        // 父目录 /tmp/should_work/ 不存在 → degraded，但仍应包含三个组件键
        assert!(!all_ok);
        assert!(components.contains_key("session_db"));
        assert!(components.contains_key("shared_http"));
        assert!(components.contains_key("kernel"));
    }

    // ── Token budget 集成（阶段 7.1 增强）──────────────────────
    // 纯函数 health_deep_assess 的 token_budget 行为：
    //   - exhausted → degraded（即使 cap 充足）
    //   - usage / cap ≥ 0.9 → degraded
    //   - usage / cap < 0.9 且未耗尽 → ok
    //   - cap = None（unbounded tracker）→ ok
    //   - token_budget = None → 不添加该组件键

    use sisyphus_intel::token_budget::{TokenBudgetSnapshot, TokenBudgetTracker};

    fn snap(usage: u64, cap: Option<u64>, exhausted: bool) -> TokenBudgetSnapshot {
        TokenBudgetSnapshot {
            usage,
            cap,
            exhausted,
        }
    }

    #[test]
    fn token_budget_unbounded_is_ok() {
        let s = snap(50_000, None, false);
        let (all_ok, components) =
            health_deep_assess(std::path::Path::new("/tmp/x.db"), true, Some(&s));
        assert!(all_ok);
        assert_eq!(components["token_budget"]["status"], "ok");
        assert_eq!(components["token_budget"]["usage"], 50_000);
        assert!(components["token_budget"]["cap"].is_null());
    }

    #[test]
    fn token_budget_low_usage_is_ok() {
        // 50% 使用率
        let s = snap(500, Some(1000), false);
        let (all_ok, components) =
            health_deep_assess(std::path::Path::new("/tmp/x.db"), true, Some(&s));
        assert!(all_ok);
        assert_eq!(components["token_budget"]["status"], "ok");
    }

    #[test]
    fn token_budget_high_usage_is_degraded() {
        // 95% 使用率（≥ 90% 阈值）
        let s = snap(950, Some(1000), false);
        let (all_ok, components) =
            health_deep_assess(std::path::Path::new("/tmp/x.db"), true, Some(&s));
        assert!(!all_ok);
        assert_eq!(components["token_budget"]["status"], "degraded");
    }

    #[test]
    fn token_budget_exhausted_is_degraded() {
        // 即使 cap 充足、usage < 90%，exhausted=true 也要 degraded
        let s = snap(500, Some(1000), true);
        let (all_ok, components) =
            health_deep_assess(std::path::Path::new("/tmp/x.db"), true, Some(&s));
        assert!(!all_ok);
        assert_eq!(components["token_budget"]["status"], "degraded");
    }

    #[test]
    fn token_budget_absent_when_none() {
        // token_budget = None → 不添加到 components map
        let (_, components) = health_deep_assess(std::path::Path::new("/tmp/x.db"), true, None);
        assert!(!components.contains_key("token_budget"));
    }

    #[test]
    fn token_budget_tracker_snapshot_integration() {
        // 端到端：从 TokenBudgetTracker 取 snapshot 喂入 health_deep_assess
        let tracker = TokenBudgetTracker::new(1000, 0.0);
        tracker.record(950); // 95% → 应为 degraded
        let s = tracker.snapshot();
        assert!(s.cap.is_some());
        assert!(!s.exhausted);
        let (all_ok, components) =
            health_deep_assess(std::path::Path::new("/tmp/x.db"), true, Some(&s));
        assert!(!all_ok);
        assert_eq!(components["token_budget"]["usage"], 950);
    }
}

// ---------------------------------------------------------------------------
// /health/deep 契约固定件（跨 crate）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod health_deep_contract_tests {
    use super::health_deep_assess;

    fn fixture_path(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../sisyphus-types/test-fixtures")
            .join(name)
    }

    fn load(name: &str) -> serde_json::Value {
        let path = fixture_path(name);
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read fixture {}: {e}", path.display()));
        serde_json::from_str(&raw).expect("fixture must be valid JSON")
    }

    /// 固定件里的组件名就是本 crate 必须产出的组件名。
    ///
    /// 固定件放在 `sisyphus-types/test-fixtures`，TUI 侧的
    /// `parse_health_deep_response` 测试读同一份文件。两侧任一改动形状而
    /// 忘记另一侧，两个 crate 的测试都会同时变红——这正是过去 TUI 读错
    /// `all_ok` 字段时没有任何测试拦下来的原因。
    #[test]
    fn emitted_components_match_the_shared_fixture() {
        let ok = load("health_deep_ok.json");
        let expected: Vec<String> = ok["components"]
            .as_object()
            .expect("fixture components must be an object")
            .keys()
            .cloned()
            .collect();

        let dir =
            std::env::temp_dir().join(format!("sisyphus-health-contract-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        // A real file inside an existing parent => session_db is ok.
        let db = dir.join("sessions.db");
        std::fs::write(&db, b"").expect("write db");

        let (all_ok, components) = health_deep_assess(&db, false, None);
        let mut emitted: Vec<String> = components.keys().cloned().collect();
        emitted.sort();
        let mut want = expected.clone();
        want.sort();

        // `token_budget` is the one optional component: the lightweight path
        // passes no snapshot, so it is legitimately absent here.
        let optional = ["token_budget"];
        let mut emitted_required: Vec<String> =
            emitted.iter().filter(|c| !optional.contains(&c.as_str())).cloned().collect();
        let mut want_required: Vec<String> =
            want.iter().filter(|c| !optional.contains(&c.as_str())).cloned().collect();
        emitted_required.sort();
        want_required.sort();

        assert_eq!(
            emitted_required, want_required,
            "the components this handler emits have drifted from the shared \
             fixture. Both this crate and sisyphus-cli parse that fixture, so a \
             one-sided change would silently break the TUI health panel."
        );
        assert!(all_ok, "a healthy session db must not degrade the verdict");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
