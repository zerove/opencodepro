//! REST API for Sisyphus Bridge (axum HTTP layer alongside gRPC).
use crate::MySisyphusBridge;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Sse},
    routing::{get, post, patch},
    Json, Router,
};
use futures_util::stream::{self, Stream};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

// ── Response / Request Types ───────────────────────────────────────────────

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

// --- Goal Execution Types ---

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
    pub event_tx: Option<bool>,
}

#[derive(Serialize)]
pub struct CompositeExecuteResponse {
    pub results: std::collections::HashMap<String, sisyphus_types::NodeOutput>,
    pub execution_order: Vec<String>,
}

// --- Roadmap Management Types ---

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
pub(crate) struct RestApiState {
    pub bridge: Arc<MySisyphusBridge>,
    pub session_db: PathBuf,
}

// ── Handlers ──────────────────────────────────────────────────────────────

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
    Json(req): Json<GoalExecuteRequest>,
) -> impl IntoResponse {
    let kernel = state.bridge.kernel();
    let event_tx = if req.event_tx.unwrap_or(false) {
        Some(mpsc::unbounded_channel().0)
    } else {
        None
    };

    let result = if let Some(project_id) = req.project_id {
        kernel.execute_goal_with_project_context(
            &req.goal,
            &req.sop_id,
            &project_id,
            event_tx,
        ).await
    } else {
        kernel.execute_goal(&req.goal, &req.sop_id, event_tx).await
    };

    match result {
        Ok((output, report)) => (
            StatusCode::OK,
            Json(serde_json::to_value(GoalExecuteResponse { output, report }).unwrap()),
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
    match kernel.active_executions.get(&goal_id) {
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

async fn goal_stream_handler(
    State(state): State<RestApiState>,
    Path(goal_id): Path<String>,
) -> impl IntoResponse {
    let kernel = state.bridge.kernel();
    let (tx, rx) = mpsc::unbounded_channel();
    
    // Subscribe to the kernel's event system
    kernel.event_subscribers.entry(goal_id.clone())
        .or_insert_with(Vec::new)
        .push(tx);

    let stream = UnboundedReceiverStream::new(rx).map(|event| {
        Sse::new(SseEvent::default()
            .event("execution_event")
            .data(serde_json::to_string(&event).unwrap()))
    });

    Sse::new(stream).keep_alive(axum::response::Sse::default().keep_alive_interval())
}

async fn execute_composite_handler(
    State(state): State<RestApiState>,
    Json(req): Json<CompositeExecuteRequest>,
) -> impl IntoResponse {
    let kernel = state.bridge.kernel();
    let event_tx = if req.event_tx.unwrap_or(false) {
        Some(mpsc::unbounded_channel().0)
    } else {
        None
    };

    let composite = sisyphus_types::CompositeGraph {
        graphs: req.goals.into_iter().map(|g| (g.clone(), sisyphus_types::SOPGraphConfig::default())).collect(),
        dependencies: vec![],
        execution_order: vec![],
    };

    match kernel.execute_composite(&composite, event_tx).await {
        Ok(output) => (
            StatusCode::OK,
            Json(serde_json::to_value(CompositeExecuteResponse {
                results: output.results.into_iter().map(|(id, (out, _))| (id, out)).collect(),
                execution_order: output.execution_order,
            }).unwrap()),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

// --- Roadmap Management Handlers ---

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
        .tool_executor
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
pub(crate) fn build_router(state: RestApiState) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/api/execute/goal", post(execute_goal_handler))
        .route("/api/execute/goal/{goal_id}/status", get(goal_status_handler))
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
