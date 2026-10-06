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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

pub(super) async fn install_remote_skill_handler(
    State(state): State<RestApiState>,
    Json(req): Json<InstallSkillRequest>,
) -> impl IntoResponse {
    match state.bridge.skill_registry.install_skill(&req.skill_id).await {
        Ok(_) => StatusCode::CREATED.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

pub(super) async fn list_installed_skills_handler(
    State(state): State<RestApiState>,
) -> impl IntoResponse {
    match state.bridge.skill_registry.list_installed_skills().await {
        Ok(skills) => (StatusCode::OK, Json(skills)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

pub(super) async fn uninstall_remote_skill_handler(
    State(state): State<RestApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.bridge.skill_registry.uninstall_skill(&id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

pub(super) async fn switch_workspace_handler(
    State(state): State<RestApiState>,
    Json(req): Json<SwitchWorkspaceRequest>,
) -> impl IntoResponse {
    match state.bridge.workspace_manager.switch_workspace(&req.id).await {
        Ok(_) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
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
        state.shared_http.is_some(),
        Some(&token_snap),
    );
    let body = serde_json::json!({
        "status": if all_ok { "ok" } else { "degraded" },
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
    let event_tx = if req.event_tx.unwrap_or(false) {
        Some(mpsc::unbounded_channel().0)
    } else {
        None
    };

    let project_id =
        workspace.project_id.or(req.project_id).unwrap_or_else(|| "default".to_string());
    tracing::Span::current().record("project_id", project_id.as_str());

    let result = kernel.execute_goal(&req.goal, &req.sop_id, &project_id, event_tx, None).await;

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
            StatusCode::INTERNAL_SERVER_ERROR,
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
            StatusCode::INTERNAL_SERVER_ERROR,
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
            StatusCode::INTERNAL_SERVER_ERROR,
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
            StatusCode::INTERNAL_SERVER_ERROR,
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
    let event_tx = if req.event_tx.unwrap_or(false) {
        Some(mpsc::unbounded_channel().0)
    } else {
        None
    };

    let project_id = workspace.project_id.or(req.project_id);

    let composite = sisyphus_types::CompositeGraph {
        graphs: req
            .goals
            .into_iter()
            .map(|g| (g.clone(), sisyphus_types::SOPGraphConfig::default()))
            .collect(),
        dependencies: vec![],
        execution_order: vec![],
    };

    match kernel
        .execute_composite(
            &composite,
            event_tx,
            project_id.as_deref().unwrap_or("default"),
        )
        .await
    {
        Ok(output) => {
            let response = CompositeExecuteResponse {
                results: output.results.into_iter().map(|(id, (out, _))| (id, out)).collect(),
                execution_order: output.execution_order,
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
            Json(serde_json::json!({ "error": e.to_string() })),
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
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
        project_id: Some(String::new()),
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
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
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
        },
        axum::http::Method::POST => {
            let Some(Json(req)) = payload else {
                return (StatusCode::BAD_REQUEST, "Missing payload").into_response();
            };
            match rm.create_project(&req.id, &req.name, &req.description).await {
                Ok(p) => (StatusCode::CREATED, Json(p)).into_response(),
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
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
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

// --- Roadmap Prediction Handlers ---

pub(super) async fn predict_completion_handler(
    State(state): State<RestApiState>,
    Json(req): Json<PredictCompletionRequest>,
) -> impl IntoResponse {
    match state.bridge.kernel().roadmap_manager.predict_completion_time(&req.project_id).await {
        Ok(prediction) => (StatusCode::OK, Json(prediction)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
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
            StatusCode::INTERNAL_SERVER_ERROR,
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
                yield Ok::<Event, std::convert::Infallible>(
                    Event::default()
                        .event("absent")
                        .data(format!(r#"{{"sop_id":"{sop_id_owned}","message":"No active cognitive map"}}"#)),
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
        health_deep_assess(&state.session_db, state.shared_http.is_some(), None);
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
