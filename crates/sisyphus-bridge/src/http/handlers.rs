//! REST API 处理器（一）：认知映射、技能、工作区、目标执行、路线图、会话。

use super::types::*;
use axum::{
    body::Body,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use futures_util::stream::{self};
use sisyphus_types::{graph::SOPGraphConfig, ToolExecutor};
use tokio::sync::mpsc;

pub(super) async fn get_cognitive_map_handler(
    State(state): State<RestApiState>,
    Path(sop_id): Path<String>,
) -> impl IntoResponse {
    match state
        .bridge
        .kernel()
        .execution_manager
        .cognitive_maps
        .get(&sop_id)
    {
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
    match state
        .bridge
        .skill_registry
        .install_skill(&req.skill_id)
        .await
    {
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
    match state
        .bridge
        .workspace_manager
        .switch_workspace(&req.id)
        .await
    {
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

// --- Goal Execution Handlers ---

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

    let project_id = workspace
        .project_id
        .or(req.project_id)
        .unwrap_or_else(|| "default".to_string());

    let result = kernel
        .execute_goal(&req.goal, &req.sop_id, &project_id, event_tx, None)
        .await;

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
                }
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
    let project_id = workspace
        .project_id
        .unwrap_or_else(|| "default".to_string());
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
        }
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
    let project_id = workspace
        .project_id
        .unwrap_or_else(|| "default".to_string());
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
            }
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
                results: output
                    .results
                    .into_iter()
                    .map(|(id, (out, _))| (id, out))
                    .collect(),
                execution_order: output.execution_order,
            };
            match serde_json::to_value(response) {
                Ok(val) => (StatusCode::OK, Json(val)),
                Err(_) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "Failed to serialize response" })),
                ),
            }
        }
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
    match kernel
        .execution_manager
        .list_sops(workspace.project_id.as_deref())
        .await
    {
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
        }
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
        }
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
        }
    };
    let sop_store = &state.bridge.kernel().execution_manager.sop_store;
    match sop_store
        .save_sop(&req.id, &project_id, &req.name, req.version, &req.config)
        .await
    {
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
        }
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
            match rm
                .create_project(&req.id, &req.name, &req.description)
                .await
            {
                Ok(p) => (StatusCode::CREATED, Json(p)).into_response(),
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
            }
        }
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

pub(super) async fn roadmap_milestone_handler(
    State(state): State<RestApiState>,
    Json(req): Json<AddMilestoneRequest>,
) -> impl IntoResponse {
    match state
        .bridge
        .kernel()
        .roadmap_manager
        .add_milestone(&req.project_id, req.milestone)
        .await
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
        }
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

pub(super) async fn roadmap_health_handler(
    State(state): State<RestApiState>,
    Path(project_id): Path<String>,
) -> impl IntoResponse {
    match state
        .bridge
        .kernel()
        .roadmap_manager
        .evaluate_health(&project_id)
        .await
    {
        Ok(health) => (StatusCode::OK, Json(health)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

// --- Roadmap Prediction Handlers ---

pub(super) async fn predict_completion_handler(
    State(state): State<RestApiState>,
    Json(req): Json<PredictCompletionRequest>,
) -> impl IntoResponse {
    match state
        .bridge
        .kernel()
        .roadmap_manager
        .predict_completion_time(&req.project_id)
        .await
    {
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
    match state
        .bridge
        .kernel()
        .roadmap_manager
        .find_bottlenecks(&req.project_id)
        .await
    {
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
    match state
        .bridge
        .kernel()
        .roadmap_manager
        .suggest_parallel_execution(&req.project_id)
        .await
    {
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
    match state
        .bridge
        .kernel()
        .roadmap_manager
        .proposal_manager()
        .list_proposals(&project_id)
        .await
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
        }
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
        }
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
        }
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
        }
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
        }
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
        }
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
        }
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
                let tools = state
                    .bridge
                    .tool_registry()
                    .list_for_prefixes(&info.tool_prefixes);
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
