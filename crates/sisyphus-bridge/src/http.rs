//! REST API for Sisyphus Bridge (axum HTTP layer alongside gRPC).
use crate::MySisyphusBridge;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    body::Body,
    response::{IntoResponse, Response},
    routing::{get, post, patch, delete},
    Json, Router,
};
use axum::extract::FromRequestParts;
use async_trait::async_trait;
use tower_http::services::ServeDir;
use futures_util::stream::{self};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use sisyphus_types::{ToolExecutor, graph::SOPGraphConfig};

// ── Response / Request Types ───────────────────────────────────────────────

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

// --- Goal Execution Types ---

#[derive(Serialize)]
pub struct TraceEventDetail {
    pub node_id: String,
    pub status: String,
    pub output: Option<String>,
    pub fuel_consumed: i64,
    pub duration_ms: i64,
}

#[derive(Serialize)]
pub struct GoalTraceResponse {
    pub sop_id: String,
    pub events: Vec<TraceEventDetail>,
}

#[derive(Serialize)]
pub struct GoalStatusResponse {
    pub status: String,
    pub current_node: Option<String>,
    pub progress: f32,
    pub thought_logs: Vec<String>,
    pub last_updated: u64,
}

#[derive(Serialize, Deserialize)]
pub struct GoalExecuteRequest {
    pub goal: String,
    pub sop_id: String,
    pub project_id: Option<String>,
    pub event_tx: Option<bool>,
}

#[derive(Serialize)]
pub struct GoalExecuteResponse {
    pub output: sisyphus_types::NodeOutput,
    pub report: String,
}

#[derive(Serialize, Deserialize)]
pub struct CompositeExecuteRequest {
    pub goals: Vec<String>,
    pub project_id: Option<String>,
    pub event_tx: Option<bool>,
}

#[derive(Serialize)]
pub struct CompositeExecuteResponse {
    pub results: std::collections::HashMap<String, sisyphus_types::NodeOutput>,
    pub execution_order: Vec<String>,
}

// --- Skill Registry Types ---

#[derive(Serialize)]
pub struct RemoteSkillEntry {
    pub id: String,
    pub metadata: sisyphus_types::skill::SkillMetadata,
}

#[derive(Deserialize)]
pub struct InstallSkillRequest {
    pub skill_id: String,
}


#[derive(Clone, Debug)]
pub struct WorkspaceContext {
    pub project_id: Option<String>,
}

#[async_trait]
impl<S> FromRequestParts<S> for WorkspaceContext
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut axum::http::request::Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let header_value = parts
            .headers
            .get("X-Sisyphus-Workspace-Id")
            .and_then(|v| v.to_str().ok());

        Ok(WorkspaceContext {
            project_id: header_value.map(|s| s.to_string()),
        })
    }
}

#[derive(Deserialize)]
pub struct SwitchWorkspaceRequest {
    pub id: String,
}

#[derive(Deserialize)]
pub struct CreateWorkspaceRequest {
    pub name: String,
    pub project_id: String,
    pub root_dir: String,
}

// --- SOP Management Types ---
#[derive(Serialize, Deserialize)]
pub struct SopSaveRequest {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub config: sisyphus_types::graph::SOPGraphConfig,
}

#[derive(Serialize)]
pub struct SopDetailResponse {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub config: sisyphus_types::graph::SOPGraphConfig,
}

// --- Cognitive Map Types ---

#[derive(Serialize)]
pub struct CogMapResponse {
    pub map: Option<sisyphus_types::CogMap>,
    pub status: String,
}
#[derive(Serialize, Deserialize)]
pub struct ExperienceQueryRequest {
    pub query_text: String,
    pub action_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Serialize)]
pub struct ExperienceSimilarResponse {
    pub experiences: Vec<sisyphus_types::experience::ExperienceTuple>,
}

#[derive(Serialize, Deserialize)]
pub struct CreateProjectRequest {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Serialize, Deserialize)]
pub struct UpdateGoalStatusRequest {
    pub project_id: String,
    pub milestone_id: String,
    pub phase_id: String,
    pub status: sisyphus_types::MilestoneStatus,
}

#[derive(Serialize, Deserialize)]
pub struct AddMilestoneRequest {
    pub project_id: String,
    pub milestone: sisyphus_types::Milestone,
}

#[derive(Serialize, Deserialize)]
pub struct AddPhaseRequest {
    pub project_id: String,
    pub milestone_id: String,
    pub phase: sisyphus_types::Phase,
}

#[derive(Serialize, Deserialize)]
pub struct AddGoalRequest {
    pub project_id: String,
    pub milestone_id: String,
    pub phase_id: String,
    pub goal: sisyphus_types::GoalRef,
}

#[derive(Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub name: String,
    pub message_count: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Deserialize)]
pub struct SessionDetail {
    pub id: String,
    pub name: String,
    pub message_count: u64,
    pub created_at: String,
    pub updated_at: String,
    pub messages: Vec<MessageDto>,
}

#[derive(Serialize, Deserialize)]
pub struct MessageDto {
    pub role: String,
    pub content: String,
    pub created_at: String,
    pub estimated_tokens: u64,
}

#[derive(Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub name: String,
}

#[derive(Serialize, Deserialize)]
pub struct AddMessageRequest {
    pub role: String,
    pub content: String,
}

#[derive(Serialize, Deserialize)]
pub struct ExecuteRequest {
    pub tool_id: String,
    #[serde(default)]
    pub args_json: String,
    #[serde(default)]
    pub fuel_limit: u64,
}

#[derive(Serialize)]
pub struct ExecuteResponse {
    pub status: String,
    pub result_json: String,
}

#[derive(Serialize)]
pub struct ToolEntry {
    pub tool_id: String,
    pub name: String,
    pub description: String,
    pub source: String,
}

#[derive(Serialize)]
pub struct AgentEntry {
    pub agent_id: String,
    pub display_name: String,
    pub description: String,
    pub tool_prefixes: Vec<String>,
    pub max_concurrency: u32,
}

#[derive(Serialize)]
pub struct SkillEntry {
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: String,
    pub tags: Vec<String>,
    pub source: String,
}

// ── Shared State ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct RestApiState {
    pub bridge: Arc<MySisyphusBridge>,
    pub session_db: PathBuf,
}

async fn get_cognitive_map_handler(
    State(state): State<RestApiState>,
    Path(sop_id): Path<String>,
) -> impl IntoResponse {
    match state.bridge.kernel().execution_manager.cognitive_maps.get(&sop_id) {
        Some(map) => (StatusCode::OK, Json(map.clone())).into_response(),
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "No active cognitive map for this SOP" }))).into_response(),
    }
}

async fn list_available_skills_handler(
    State(state): State<RestApiState>,
) -> impl IntoResponse {
    match state.bridge.skill_registry.fetch_available_skills().await {
        Ok(skills) => (StatusCode::OK, Json(skills)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

async fn install_remote_skill_handler(
    State(state): State<RestApiState>,
    Json(req): Json<InstallSkillRequest>,
) -> impl IntoResponse {
    match state.bridge.skill_registry.install_skill(&req.skill_id).await {
        Ok(_) => StatusCode::CREATED.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

async fn list_installed_skills_handler(
    State(state): State<RestApiState>,
) -> impl IntoResponse {
    match state.bridge.skill_registry.list_installed_skills().await {
        Ok(skills) => (StatusCode::OK, Json(skills)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

async fn uninstall_remote_skill_handler(
    State(state): State<RestApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.bridge.skill_registry.uninstall_skill(&id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

// --- Workspace Management Handlers ---

async fn list_workspaces_handler(
    State(state): State<RestApiState>,
) -> impl IntoResponse {
    let workspaces = state.bridge.workspace_manager.list_workspaces().await;
    (StatusCode::OK, Json(workspaces)).into_response()
}

async fn create_workspace_handler(
    State(state): State<RestApiState>,
    Json(req): Json<CreateWorkspaceRequest>,
) -> impl IntoResponse {
    match state.bridge.workspace_manager.create_workspace(req.name, req.project_id, req.root_dir).await {
        Ok(ws) => (StatusCode::CREATED, Json(ws)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

async fn switch_workspace_handler(
    State(state): State<RestApiState>,
    Json(req): Json<SwitchWorkspaceRequest>,
) -> impl IntoResponse {
    match state.bridge.workspace_manager.switch_workspace(&req.id).await {
        Ok(_) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

async fn get_current_workspace_handler(
    State(state): State<RestApiState>,
) -> impl IntoResponse {
    match state.bridge.workspace_manager.get_current().await {
        Some(ws) => (StatusCode::OK, Json(ws)).into_response(),
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "No active workspace" }))).into_response(),
    }
}

async fn health_handler(State(state): State<RestApiState>) -> impl IntoResponse {
    let _ = state.bridge.kernel();
    Json(HealthResponse {
        status: "SERVING".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

// --- Goal Execution Handlers ---

async fn execute_goal_handler(
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

    let project_id = workspace.project_id.or(req.project_id).unwrap_or_else(|| "default".to_string());

    let result = kernel.execute_goal(
        &req.goal,
        &req.sop_id,
        &project_id,
        event_tx,
        None,
    ).await;

    match result {
        Ok((output, report)) => (
            StatusCode::OK,
            match serde_json::to_value(GoalExecuteResponse { output, report }) {
                Ok(val) => Json(val),
                Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": "Failed to serialize response" }))),
            },
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

async fn goal_status_handler(
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
            Json(serde_json::json!({ "error": format!("Goal execution {} not found or completed", goal_id) })),
        ),
    }
}

async fn goal_trace_handler(
    State(state): State<RestApiState>,
    workspace: WorkspaceContext,
    Path(sop_id): Path<String>,
) -> impl IntoResponse {
    let project_id = workspace.project_id.unwrap_or_else(|| "default".to_string());
    let store = &state.bridge.state_store;
    match store.get_traces_for_sop(&project_id, &sop_id).await {
        Ok(rows) => {
            let events = rows.into_iter().map(|(node_id, status, output, fuel, duration)| {
                TraceEventDetail {
                    node_id,
                    status,
                    output: Some(output),
                    fuel_consumed: fuel,
                    duration_ms: duration,
                }
            }).collect();
            (StatusCode::OK, Json(GoalTraceResponse { sop_id, events })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

async fn goal_stream_handler(
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
        if let Err(e) = bridge_clone.kernel().execute_goal_streaming(&goal_clone, "stream_task", &pid_clone, Some(tx), None).await {
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Response build failed: {e}")).into_response(),
    }
}

async fn execute_composite_handler(
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
        graphs: req.goals.into_iter().map(|g| (g.clone(), sisyphus_types::SOPGraphConfig::default())).collect(),
        dependencies: vec![],
        execution_order: vec![],
    };

    match kernel.execute_composite(&composite, event_tx, project_id.as_deref().unwrap_or("default")).await {
        Ok(output) => {
            let response = CompositeExecuteResponse {
                results: output.results.into_iter().map(|(id, (out, _))| (id, out)).collect(),
                execution_order: output.execution_order,
            };
            match serde_json::to_value(response) {
                Ok(val) => (StatusCode::OK, Json(val)),
                Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": "Failed to serialize response" }))),
            }
        },
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

async fn list_sops_handler(
    State(state): State<RestApiState>,
    workspace: WorkspaceContext,
) -> impl IntoResponse {
    let kernel = state.bridge.kernel();
    match kernel.execution_manager.list_sops(workspace.project_id.as_deref()).await {
        Ok(sops) => (StatusCode::OK, Json(sops)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

async fn get_sop_handler(
    State(state): State<RestApiState>,
    workspace: WorkspaceContext,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let kernel = state.bridge.kernel();
    let project_id = match workspace.project_id {
        Some(pid) => pid,
        None => return (StatusCode::BAD_REQUEST, "Missing X-Sisyphus-Workspace-Id header").into_response(),
    };
    match kernel.execution_manager.load_sop(&id, &project_id).await {
        Ok(sop) => {
            let config = SOPGraphConfig::from_graph(&sop);
            (StatusCode::OK, Json(SopDetailResponse {
                id,
                name: "Sisyphus SOP".into(), // Name is not currently in SOPGraph
                version: Some("1.0.0".into()),
                config
            })).into_response()
        }
        Err(e) => (StatusCode::NOT_FOUND, Json(e.to_string())).into_response(),
    }
}

async fn save_sop_handler(
    State(state): State<RestApiState>,
    workspace: WorkspaceContext,
    Json(req): Json<SopSaveRequest>,
) -> impl IntoResponse {
    let project_id = match workspace.project_id {
        Some(pid) => pid,
        None => return (StatusCode::BAD_REQUEST, "Missing X-Sisyphus-Workspace-Id header").into_response(),
    };
    let sop_store = &state.bridge.kernel().execution_manager.sop_store;
    match sop_store.save_sop(&req.id, &project_id, &req.name, req.version, &req.config).await {
        Ok(_) => StatusCode::CREATED.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

async fn delete_sop_handler(
    State(state): State<RestApiState>,
    workspace: WorkspaceContext,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let project_id = match workspace.project_id {
        Some(pid) => pid,
        None => return (StatusCode::BAD_REQUEST, "Missing X-Sisyphus-Workspace-Id header").into_response(),
    };
    let sop_store = &state.bridge.kernel().execution_manager.sop_store;
    match sop_store.delete_sop(&id, &project_id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

async fn experience_similar_handler(
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
        Ok(exps) => (StatusCode::OK, Json(ExperienceSimilarResponse { experiences: exps })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

async fn roadmap_project_handler(
    State(state): State<RestApiState>,
    method: axum::http::Method,
    Path(id): Path<String>,
    payload: Option<Json<CreateProjectRequest>>,
) -> impl IntoResponse {
    let rm = &state.bridge.kernel().roadmap_manager;
    match method {
        axum::http::Method::GET => {
            match rm.get_project(&id).await {
                Ok(Some(p)) => (StatusCode::OK, Json(p)).into_response(),
                Ok(None) => StatusCode::NOT_FOUND.into_response(),
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
            }
        }
        axum::http::Method::POST => {
            let Some(Json(req)) = payload else {
                return (StatusCode::BAD_REQUEST, "Missing payload").into_response();
            };
            match rm.create_project(&req.id, &req.name, &req.description).await {
                Ok(p) => (StatusCode::CREATED, Json(p)).into_response(),
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
            }
        }
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

async fn roadmap_milestone_handler(
    State(state): State<RestApiState>,
    Json(req): Json<AddMilestoneRequest>,
) -> impl IntoResponse {
    match state.bridge.kernel().roadmap_manager.add_milestone(&req.project_id, req.milestone).await {
        Ok(_) => StatusCode::CREATED.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

async fn roadmap_phase_handler(
    State(state): State<RestApiState>,
    Json(req): Json<AddPhaseRequest>,
) -> impl IntoResponse {
    match state.bridge.kernel().roadmap_manager.add_phase(&req.project_id, &req.milestone_id, req.phase).await {
        Ok(_) => StatusCode::CREATED.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

async fn roadmap_goal_handler(
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
            match rm.update_goal_status(&req.project_id, &req.milestone_id, &req.phase_id, &goal_id, req.status).await {
                Ok(_) => StatusCode::OK.into_response(),
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
            }
        }
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

async fn roadmap_health_handler(
    State(state): State<RestApiState>,
    Path(project_id): Path<String>,
) -> impl IntoResponse {
    match state.bridge.kernel().roadmap_manager.evaluate_health(&project_id).await {
        Ok(health) => (StatusCode::OK, Json(health)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

async fn execute_handler(
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
        .execution_manager.tool_router
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

async fn list_sessions_handler(State(state): State<RestApiState>) -> impl IntoResponse {
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

async fn create_session_handler(
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

async fn get_session_handler(
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

async fn add_message_handler(
    State(state): State<RestApiState>,
    Path(id): Path<String>,
    Json(req): Json<AddMessageRequest>,
) -> impl IntoResponse {
    match sisyphus_tools_session::session_add_message(&state.session_db, &id, &req.role, &req.content) {
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

async fn list_tools_handler(State(state): State<RestApiState>) -> impl IntoResponse {
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

async fn list_agents_handler(State(state): State<RestApiState>) -> impl IntoResponse {
    let agents = state.bridge.agent_registry().list();
    let entries: Vec<AgentEntry> = agents
        .into_iter()
        .map(|(agent_id, info)| {
            let description = if !info.tool_prefixes.is_empty() {
                let tools = state
                    .bridge
                    .tool_registry()
                    .list_for_prefixes(&info.tool_prefixes);
                let tool_names: Vec<&str> =
                    tools.iter().map(|(_, t)| t.name.as_str()).collect();
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

async fn list_skills_handler(State(state): State<RestApiState>) -> impl IntoResponse {
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

// ── Router Builder ────────────────────────────────────────────────────────

impl RestApiState {
    pub fn new(bridge: Arc<MySisyphusBridge>, data_dir: PathBuf) -> Self {
        let session_db = data_dir.join("sessions.json");
        Self { bridge, session_db }
    }
}

/// Build the axum Router with all REST API routes.
pub fn build_router(state: RestApiState) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .nest_service("/dashboard", ServeDir::new("static/dashboard"))
        .route("/api/skills/available", get(list_available_skills_handler))
        .route("/api/skills/install", post(install_remote_skill_handler))
        .route("/api/skills/installed", get(list_installed_skills_handler))
        .route("/api/skills/remote/{id}", delete(uninstall_remote_skill_handler))
        .route("/api/cognitive/map/{sop_id}", get(get_cognitive_map_handler))
        .route("/api/workspaces", get(list_workspaces_handler).post(create_workspace_handler))
        .route("/api/workspaces/switch", post(switch_workspace_handler))
        .route("/api/workspaces/current", get(get_current_workspace_handler))
        .route("/api/sops", get(list_sops_handler).post(save_sop_handler))
        .route("/api/sops/{id}", get(get_sop_handler).delete(delete_sop_handler))
        .route("/api/experience/similar", post(experience_similar_handler))
        .route("/api/execute/goal", post(execute_goal_handler))
        .route("/api/execute/goal/{goal_id}/status", get(goal_status_handler))
        .route("/api/execute/goal/{goal_id}/trace", get(goal_trace_handler))
        .route("/api/execute/goal/{goal_id}/stream", get(goal_stream_handler))
        .route("/api/execute/composite", post(execute_composite_handler))
        .route(
            "/api/roadmap/project/:id",
            get(roadmap_project_handler).post(roadmap_project_handler),
        )
        .route("/api/roadmap/milestone", post(roadmap_milestone_handler))
        .route("/api/roadmap/phase", post(roadmap_phase_handler))
        .route("/api/roadmap/goal/:goal_id", patch(roadmap_goal_handler))
        .route("/api/roadmap/health/:project_id", get(roadmap_health_handler))
        .route("/api/execute", post(execute_handler))
        .route(
            "/api/sessions",
            get(list_sessions_handler).post(create_session_handler),
        )
        .route("/api/sessions/{id}", get(get_session_handler))
        .route("/api/sessions/{id}/messages", post(add_message_handler))
        .route("/api/tools", get(list_tools_handler))
        .route("/api/agents", get(list_agents_handler))
        .route("/api/skills", get(list_skills_handler))
        .route("/openapi.json", get(http_openapi::openapi_json))
        .route("/swagger-ui/*rest", get(http_openapi::swagger_ui_handler))
        .with_state(state)
}

// ── OpenAPI helpers ───────────────────────────────────────────────────────

#[path = "http_openapi.rs"]
mod http_openapi;
