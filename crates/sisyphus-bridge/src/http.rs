//! REST API for Sisyphus Bridge (axum HTTP layer alongside gRPC).
use crate::MySisyphusBridge;
use async_trait::async_trait;
use axum::extract::FromRequestParts;
use axum::{
    body::Body,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
    Json, Router,
};
use futures_util::stream::{self};
use serde::{Deserialize, Serialize};
use sisyphus_roadmap::strategic_reflection::StrategicProposal;
use sisyphus_roadmap::{Bottleneck, DelayAlert, GoalResourceEstimate, ParallelizationOpportunity};
use sisyphus_types::{graph::SOPGraphConfig, ToolExecutor};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tower_http::services::ServeDir;
use uuid::Uuid;

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

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
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
pub struct ProposalActionRequest {
    pub project_id: String,
    pub proposal_id: String,
    pub reviewer: String,
}

#[derive(Serialize)]
pub struct ProposalListResponse {
    pub proposals: Vec<StrategicProposal>,
}

#[derive(Serialize, Deserialize)]
pub struct PredictCompletionRequest {
    pub project_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct CheckDelayAlertsRequest {
    pub project_id: String,
    #[serde(default)]
    pub threshold_multiplier: Option<f64>,
}

#[derive(Serialize, Deserialize)]
pub struct DelayAlertsResponse {
    pub alerts: Vec<DelayAlert>,
}

#[derive(Serialize, Deserialize)]
pub struct ProjectCostsRequest {
    pub project_id: String,
    pub total_goals: usize,
    pub completed_goals: usize,
}

#[derive(Serialize, Deserialize)]
pub struct FindBottlenecksRequest {
    pub project_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct SuggestParallelExecutionRequest {
    pub project_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct EstimateGoalResourcesRequest {
    pub project_id: String,
    pub goal_ids: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct EstimateGoalResourcesResponse {
    pub estimates: Vec<GoalResourceEstimate>,
}

#[derive(Serialize, Deserialize)]
pub struct FindBottlenecksResponse {
    pub bottlenecks: Vec<Bottleneck>,
}

#[derive(Serialize, Deserialize)]
pub struct SuggestParallelExecutionResponse {
    pub opportunities: Vec<ParallelizationOpportunity>,
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

// --- Tool Execution Types ---
#[derive(Deserialize)]
pub struct ExecuteToolRequest {
    pub tool_id: String,
    pub args_json: String,
    pub fuel_limit: Option<u64>,
}

#[derive(Serialize)]
pub struct ExecuteToolResponse {
    pub status: String,
    pub result_json: String,
}

// --- Cognitive Delegation Types ---
#[derive(Deserialize)]
pub struct DelegateCognitiveTaskRequest {
    pub goal: String,
    pub context: String,
}

#[derive(Serialize)]
pub struct DelegateCognitiveTaskResponse {
    pub success: bool,
    pub error: String,
    pub plan_json: String,
}

// --- Feedback Types ---
#[derive(Deserialize)]
pub struct SubmitFeedbackRequest {
    pub feedback_json: String,
}

#[derive(Serialize)]
pub struct SubmitFeedbackResponse {
    pub success: bool,
    pub error: String,
    pub evolution_json: String,
}

// --- Experience Capture Types ---
#[derive(Deserialize)]
pub struct CaptureExperienceRequest {
    pub plan_id: String,
    pub plan_json: String,
}

#[derive(Serialize)]
pub struct CaptureExperienceResponse {
    pub success: bool,
    pub error: String,
}

// --- Skill Detail Types ---
#[derive(Serialize)]
pub struct SkillDetailResponse {
    pub found: bool,
    pub skill: Option<sisyphus_types::skill::SkillMetadata>,
}

// --- Benchmark Types ---
#[derive(Serialize)]
pub struct BenchmarkEntry {
    pub name: String,
    pub duration_secs: f64,
    pub timestamp: u64,
    pub success: bool,
    pub metrics_json: String,
}

#[derive(Deserialize)]
pub struct ListBenchmarksRequest {
    pub name_filter: Option<String>,
}

#[derive(Serialize)]
pub struct ListBenchmarksResponse {
    pub benchmarks: Vec<BenchmarkEntry>,
}

// --- Circuit Breaker Types ---
#[derive(Serialize)]
pub struct BreakerStatus {
    pub model_name: String,
    pub state: String,
    pub failure_count: u32,
    pub success_count: u32,
}

#[derive(Deserialize)]
pub struct BreakerMetricsRequest {
    pub model_name: Option<String>,
}

#[derive(Serialize)]
pub struct BreakerMetricsResponse {
    pub breakers: Vec<BreakerStatus>,
}

// --- Learning Insights Types ---
#[derive(Deserialize)]
pub struct LearningInsightsRequest {
    pub strategy_filter: Option<String>,
}

#[derive(Serialize)]
pub struct StrategyInsight {
    pub strategy: String,
    pub total_uses: u64,
    pub successes: u64,
    pub failures: u64,
    pub avg_duration_secs: f64,
    pub success_rate: f64,
}

#[derive(Serialize)]
pub struct LearningInsightsResponse {
    pub total_samples: u64,
    pub overall_success_rate: f64,
    pub strategies: Vec<StrategyInsight>,
}

// --- Plugin Types ---
#[derive(Serialize)]
pub struct PluginDescriptor {
    pub name: String,
    pub version: String,
    pub wasm_path: String,
    pub tool_id: String,
    pub fuel_limit: u64,
    pub checksum: String,
}

#[derive(Deserialize)]
pub struct LoadPluginRequest {
    pub name: String,
    pub version: String,
    pub wasm_path: String,
    pub tool_id: String,
    pub fuel_limit: Option<u64>,
}

#[derive(Serialize)]
pub struct LoadPluginResponse {
    pub success: bool,
    pub error: String,
}

#[derive(Deserialize)]
pub struct UnloadPluginRequest {
    pub name: String,
}

#[derive(Serialize)]
pub struct UnloadPluginResponse {
    pub success: bool,
    pub error: String,
}

// --- JCode Call Types ---
#[derive(Deserialize)]
pub struct CallJcodeRequest {
    pub task_id: String,
    pub goal: String,
    pub context_json: String,
}

#[derive(Serialize)]
pub struct CallJcodeResponse {
    pub success: bool,
    pub result: String,
    pub error: String,
}

// --- Chaos Types ---
#[derive(Deserialize)]
pub struct ChaosStartRequest {
    pub name: String,
    pub fault_types: Vec<String>,
    pub fault_probability: f64,
    pub duration_seconds: u64,
    pub target_operations: Vec<String>,
}

#[derive(Serialize)]
pub struct ChaosStartResponse {
    pub success: bool,
    pub experiment_id: String,
    pub error: String,
}

#[derive(Deserialize)]
pub struct ChaosStopRequest {
    pub experiment_id: String,
}

#[derive(Serialize)]
pub struct ChaosExperimentResult {
    pub name: String,
    pub operations_attempted: u64,
    pub operations_succeeded: u64,
    pub operations_failed: u64,
    pub faults_injected: u64,
    pub success_rate: f64,
}

#[derive(Serialize)]
pub struct ChaosStopResponse {
    pub success: bool,
    pub result: ChaosExperimentResult,
    pub error: String,
}

#[derive(Deserialize)]
pub struct ChaosStatusRequest {
    pub experiment_id: String,
}

#[derive(Serialize)]
pub struct ChaosStatusResponse {
    pub running: bool,
    pub result: ChaosExperimentResult,
}

// --- Anomaly Types ---
#[derive(Serialize)]
pub struct AnomalyReport {
    pub node_name: String,
    pub severity: String,
    pub metric: String,
    pub observed: f64,
    pub expected: f64,
    pub description: String,
    pub timestamp: u64,
}

#[derive(Deserialize)]
pub struct AnomalyRequest {
    pub sop_id: String,
}

// --- Delete Workspace ---
#[derive(Deserialize)]
pub struct DeleteWorkspaceRequest {
    pub workspace_id: String,
}

#[derive(Serialize)]
pub struct DeleteWorkspaceResponse {
    pub success: bool,
    pub error: String,
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

async fn list_available_skills_handler(State(state): State<RestApiState>) -> impl IntoResponse {
    match state.bridge.skill_registry.fetch_available_skills().await {
        Ok(skills) => (StatusCode::OK, Json(skills)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

async fn install_remote_skill_handler(
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

async fn list_installed_skills_handler(State(state): State<RestApiState>) -> impl IntoResponse {
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

async fn list_workspaces_handler(State(state): State<RestApiState>) -> impl IntoResponse {
    let workspaces = state.bridge.workspace_manager.list_workspaces().await;
    (StatusCode::OK, Json(workspaces)).into_response()
}

async fn create_workspace_handler(
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

async fn switch_workspace_handler(
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

async fn get_current_workspace_handler(State(state): State<RestApiState>) -> impl IntoResponse {
    match state.bridge.workspace_manager.get_current().await {
        Some(ws) => (StatusCode::OK, Json(ws)).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "No active workspace" })),
        )
            .into_response(),
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
            Json(
                serde_json::json!({ "error": format!("Goal execution {} not found or completed", goal_id) }),
            ),
        ),
    }
}

async fn goal_trace_handler(
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

async fn goal_stream_handler(
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

async fn list_sops_handler(
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

async fn get_sop_handler(
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

async fn save_sop_handler(
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

async fn delete_sop_handler(
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
        Ok(exps) => (
            StatusCode::OK,
            Json(ExperienceSimilarResponse { experiences: exps }),
        )
            .into_response(),
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

async fn roadmap_milestone_handler(
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

async fn roadmap_phase_handler(
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

async fn roadmap_health_handler(
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

async fn predict_completion_handler(
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

async fn check_delay_alerts_handler(
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

async fn project_costs_handler(
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

async fn find_bottlenecks_handler(
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

async fn suggest_parallel_execution_handler(
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

async fn estimate_goal_resources_handler(
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

async fn list_proposals_handler(
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

async fn get_proposal_handler(
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

async fn approve_proposal_handler(
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

async fn reject_proposal_handler(
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

// ── Missing gRPC Service Handlers ────────────────────────────────────────────

async fn show_skill_handler(
    State(state): State<RestApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.bridge.skill_registry.get_skill(&id).await {
        Ok(Some(skill)) => (
            StatusCode::OK,
            Json(SkillDetailResponse {
                found: true,
                skill: Some(skill.metadata),
            }),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(SkillDetailResponse {
                found: false,
                skill: None,
            }),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

async fn execute_tool_handler(
    State(state): State<RestApiState>,
    Json(req): Json<ExecuteToolRequest>,
) -> impl IntoResponse {
    let fuel = req.fuel_limit.unwrap_or(state.bridge.default_fuel);
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
                Json(ExecuteToolResponse {
                    status: "SUCCESS".into(),
                    result_json: result_str,
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ExecuteToolResponse {
                status: format!("ERROR: {e}"),
                result_json: String::new(),
            }),
        )
            .into_response(),
    }
}

async fn delegate_cognitive_task_handler(
    State(state): State<RestApiState>,
    Json(req): Json<DelegateCognitiveTaskRequest>,
) -> impl IntoResponse {
    let project_id = "default"; // TODO: extract from header
    let ctx = sisyphus_types::ExecutionContext::new();
    ctx.write_notepad("context".into(), req.context);
    match state
        .bridge
        .kernel()
        .intelligence_manager
        .synthesize_goal(
            &req.goal,
            &ctx,
            project_id,
            sisyphus_types::WorkMode::Standard,
            None,
        )
        .await
    {
        Ok(result) => {
            let plan = sisyphus_types::SOPGraphConfig::from_graph(&result.graph)
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(DelegateCognitiveTaskResponse {
                            success: false,
                            error: format!("Graph error: {e}"),
                            plan_json: "".into(),
                        }),
                    )
                        .into_response()
                })
                .unwrap()
                .into_cognitive_plan(req.goal.clone(), req.goal.clone());
            let _ = state.bridge.kernel().cache_plan(&plan.plan_id, &plan).await;
            let plan_json = serde_json::to_string(&plan)
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(DelegateCognitiveTaskResponse {
                            success: false,
                            error: format!("Serialization failed: {e}"),
                            plan_json: "".into(),
                        }),
                    )
                        .into_response()
                })
                .unwrap();
            (
                StatusCode::OK,
                Json(DelegateCognitiveTaskResponse {
                    success: true,
                    error: String::new(),
                    plan_json,
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(DelegateCognitiveTaskResponse {
                success: false,
                error: e.to_string(),
                plan_json: "".into(),
            }),
        )
            .into_response(),
    }
}

async fn submit_feedback_handler(
    State(state): State<RestApiState>,
    Json(req): Json<SubmitFeedbackRequest>,
) -> impl IntoResponse {
    let project_id = "default"; // TODO: extract from header
    let feedback: sisyphus_types::ExecutionFeedback = match serde_json::from_str(&req.feedback_json)
    {
        Ok(f) => f,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(SubmitFeedbackResponse {
                    success: false,
                    error: format!("Invalid feedback JSON: {e}"),
                    evolution_json: "".into(),
                }),
            )
                .into_response()
        }
    };
    match state
        .bridge
        .kernel()
        .handle_feedback(feedback, project_id)
        .await
    {
        Ok(evolution) => {
            let evolution_json = serde_json::to_string(&evolution).unwrap_or_default();
            (
                StatusCode::OK,
                Json(SubmitFeedbackResponse {
                    success: true,
                    error: String::new(),
                    evolution_json,
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(SubmitFeedbackResponse {
                success: false,
                error: e.to_string(),
                evolution_json: "".into(),
            }),
        )
            .into_response(),
    }
}

async fn capture_experience_handler(
    State(state): State<RestApiState>,
    Json(req): Json<CaptureExperienceRequest>,
) -> impl IntoResponse {
    let project_id = "default"; // TODO: extract from header
    let plan: sisyphus_types::CognitivePlan = match serde_json::from_str(&req.plan_json) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(CaptureExperienceResponse {
                    success: false,
                    error: format!("Invalid plan JSON: {e}"),
                }),
            )
                .into_response()
        }
    };
    match state
        .bridge
        .kernel()
        .capture_success(&req.plan_id, project_id, plan)
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(CaptureExperienceResponse {
                success: true,
                error: String::new(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(CaptureExperienceResponse {
                success: false,
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

async fn list_benchmarks_handler(
    State(_state): State<RestApiState>,
    _req: axum::extract::Query<ListBenchmarksRequest>,
) -> impl IntoResponse {
    // TODO: Implement benchmark listing from sisyphus-metrics
    (
        StatusCode::OK,
        Json(ListBenchmarksResponse { benchmarks: vec![] }),
    )
        .into_response()
}

async fn get_breaker_metrics_handler(
    State(_state): State<RestApiState>,
    _req: axum::extract::Query<BreakerMetricsRequest>,
) -> impl IntoResponse {
    // TODO: Implement circuit breaker metrics from sisyphus-tools-llm
    (
        StatusCode::OK,
        Json(BreakerMetricsResponse { breakers: vec![] }),
    )
        .into_response()
}

async fn get_learning_insights_handler(
    State(state): State<RestApiState>,
    req: axum::extract::Query<LearningInsightsRequest>,
) -> impl IntoResponse {
    let filter = req.strategy_filter.clone().unwrap_or_default();
    let ml = state.bridge.kernel().meta_learner.lock();
    let insight = ml.learn();
    let strategies: Vec<StrategyInsight> = insight
        .strategies
        .iter()
        .filter(|s| filter.is_empty() || s.strategy == filter)
        .map(|s| StrategyInsight {
            strategy: s.strategy.clone(),
            total_uses: s.total_uses,
            successes: s.successes,
            failures: s.failures,
            avg_duration_secs: s.avg_duration_secs,
            success_rate: s.success_rate,
        })
        .collect();
    (
        StatusCode::OK,
        Json(LearningInsightsResponse {
            total_samples: insight.total_samples,
            overall_success_rate: insight.overall_success_rate,
            strategies,
        }),
    )
        .into_response()
}

async fn list_plugins_handler(State(state): State<RestApiState>) -> impl IntoResponse {
    let plugins = state.bridge.plugin_manager.list_plugins();
    let entries: Vec<PluginDescriptor> = plugins
        .into_iter()
        .map(|p| PluginDescriptor {
            name: p.name,
            version: p.version,
            wasm_path: p.wasm_path.to_string_lossy().to_string(),
            tool_id: p.tool_id,
            fuel_limit: p.fuel_limit,
            checksum: p.checksum.unwrap_or_default(),
        })
        .collect();
    (StatusCode::OK, Json(entries)).into_response()
}

async fn load_plugin_handler(
    State(state): State<RestApiState>,
    Json(req): Json<LoadPluginRequest>,
) -> impl IntoResponse {
    let descriptor = sisyphus_types::PluginDescriptor {
        name: req.name,
        version: req.version,
        wasm_path: req.wasm_path.into(),
        tool_id: req.tool_id,
        fuel_limit: req.fuel_limit.unwrap_or(0),
        checksum: None,
    };
    match state
        .bridge
        .plugin_manager
        .register_plugin(descriptor)
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(LoadPluginResponse {
                success: true,
                error: String::new(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(LoadPluginResponse {
                success: false,
                error: e,
            }),
        )
            .into_response(),
    }
}

async fn unload_plugin_handler(
    State(state): State<RestApiState>,
    Json(req): Json<UnloadPluginRequest>,
) -> impl IntoResponse {
    match state.bridge.plugin_manager.unload_plugin(&req.name) {
        Ok(_) => (
            StatusCode::OK,
            Json(UnloadPluginResponse {
                success: true,
                error: String::new(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(UnloadPluginResponse {
                success: false,
                error: e,
            }),
        )
            .into_response(),
    }
}

async fn call_jcode_handler(
    State(state): State<RestApiState>,
    Json(req): Json<CallJcodeRequest>,
) -> impl IntoResponse {
    let args: Vec<u8> = match serde_json::to_vec(&serde_json::json!({
        "task_id": req.task_id,
        "goal": req.goal,
        "context": req.context_json
    })) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(CallJcodeResponse {
                    success: false,
                    result: String::new(),
                    error: format!("Invalid context JSON: {e}"),
                }),
            )
                .into_response()
        }
    };
    match state
        .bridge
        .jcode_caller
        .call_tool("jcode/call", args)
        .await
    {
        Ok(result) => {
            let result_str = String::from_utf8_lossy(&result).to_string();
            (
                StatusCode::OK,
                Json(CallJcodeResponse {
                    success: true,
                    result: result_str,
                    error: String::new(),
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(CallJcodeResponse {
                success: false,
                result: String::new(),
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

async fn chaos_start_handler(
    State(state): State<RestApiState>,
    Json(req): Json<ChaosStartRequest>,
) -> impl IntoResponse {
    // Convert string fault types to FaultType enum
    let fault_types: Vec<sisyphus_core::FaultType> = req
        .fault_types
        .iter()
        .filter_map(|s| match s.as_str() {
            "LlmTimeout" => Some(sisyphus_core::FaultType::LlmTimeout),
            "LlmGarbage" => Some(sisyphus_core::FaultType::LlmGarbage),
            "ToolCrash" => Some(sisyphus_core::FaultType::ToolCrash),
            "ToolHang" => Some(sisyphus_core::FaultType::ToolHang),
            "StorageWriteFailure" => Some(sisyphus_core::FaultType::StorageWriteFailure),
            "StorageReadFailure" => Some(sisyphus_core::FaultType::StorageReadFailure),
            "NetworkPartition" => Some(sisyphus_core::FaultType::NetworkPartition),
            _ if s.starts_with("RandomDelay(") && s.ends_with(")") => {
                let inner = s["RandomDelay(".len()..s.len() - 1].parse().ok()?;
                Some(sisyphus_core::FaultType::RandomDelay(inner))
            }
            _ => None,
        })
        .collect();

    let experiment = sisyphus_core::ChaosExperiment {
        name: req.name,
        fault_types,
        fault_probability: req.fault_probability,
        duration: std::time::Duration::from_secs(req.duration_seconds),
        target_operations: if req.target_operations.is_empty() {
            None
        } else {
            Some(req.target_operations)
        },
    };
    state.bridge.chaos_runner.start_experiment(experiment);
    // ChaosExperiment doesn't return an ID, we generate one
    let experiment_id = Uuid::new_v4().to_string();
    (
        StatusCode::OK,
        Json(ChaosStartResponse {
            success: true,
            experiment_id,
            error: String::new(),
        }),
    )
        .into_response()
}

async fn chaos_stop_handler(
    State(state): State<RestApiState>,
    Json(_req): Json<ChaosStopRequest>,
) -> impl IntoResponse {
    let result = state.bridge.chaos_runner.stop_experiment();
    match result {
        Some(result) => {
            let name = result.name.clone();
            let operations_attempted = result.operations_attempted;
            let operations_succeeded = result.operations_succeeded;
            let operations_failed = result.operations_failed;
            let faults_injected = result.faults_injected;
            let success_rate = result.success_rate();
            let exp_result = ChaosExperimentResult {
                name,
                operations_attempted,
                operations_succeeded,
                operations_failed,
                faults_injected,
                success_rate,
            };
            (
                StatusCode::OK,
                Json(ChaosStopResponse {
                    success: true,
                    result: exp_result,
                    error: String::new(),
                }),
            )
                .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(ChaosStopResponse {
                success: false,
                result: ChaosExperimentResult {
                    name: String::new(),
                    operations_attempted: 0,
                    operations_succeeded: 0,
                    operations_failed: 0,
                    faults_injected: 0,
                    success_rate: 0.0,
                },
                error: "No running experiment".into(),
            }),
        )
            .into_response(),
    }
}

async fn chaos_status_handler(
    State(state): State<RestApiState>,
    _req: axum::extract::Query<ChaosStatusRequest>,
) -> impl IntoResponse {
    // Return current experiment status based on enabled flag
    let running = state.bridge.chaos_runner.is_running();
    let exp_result = if running {
        ChaosExperimentResult {
            name: "active".to_string(),
            operations_attempted: 0,
            operations_succeeded: 0,
            operations_failed: 0,
            faults_injected: 0,
            success_rate: 0.0,
        }
    } else {
        ChaosExperimentResult {
            name: String::new(),
            operations_attempted: 0,
            operations_succeeded: 0,
            operations_failed: 0,
            faults_injected: 0,
            success_rate: 0.0,
        }
    };
    (
        StatusCode::OK,
        Json(ChaosStatusResponse {
            running,
            result: exp_result,
        }),
    )
        .into_response()
}

async fn anomaly_report_handler(
    State(_state): State<RestApiState>,
    _req: axum::extract::Query<AnomalyRequest>,
) -> impl IntoResponse {
    // TODO: Implement anomaly detection from sisyphus-intel
    (StatusCode::OK, Json(Vec::<AnomalyReport>::new())).into_response()
}

async fn delete_workspace_handler(
    State(state): State<RestApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.bridge.workspace_manager.delete_workspace(&id).await {
        Ok(_) => (
            StatusCode::OK,
            Json(DeleteWorkspaceResponse {
                success: true,
                error: String::new(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(DeleteWorkspaceResponse {
                success: false,
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
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
        .route(
            "/api/skills/remote/{id}",
            delete(uninstall_remote_skill_handler),
        )
        .route(
            "/api/cognitive/map/{sop_id}",
            get(get_cognitive_map_handler),
        )
        .route(
            "/api/workspaces",
            get(list_workspaces_handler).post(create_workspace_handler),
        )
        .route("/api/workspaces/switch", post(switch_workspace_handler))
        .route(
            "/api/workspaces/current",
            get(get_current_workspace_handler),
        )
        .route("/api/sops", get(list_sops_handler).post(save_sop_handler))
        .route(
            "/api/sops/{id}",
            get(get_sop_handler).delete(delete_sop_handler),
        )
        .route("/api/experience/similar", post(experience_similar_handler))
        .route("/api/execute/goal", post(execute_goal_handler))
        .route(
            "/api/execute/goal/{goal_id}/status",
            get(goal_status_handler),
        )
        .route("/api/execute/goal/{goal_id}/trace", get(goal_trace_handler))
        .route(
            "/api/execute/goal/{goal_id}/stream",
            get(goal_stream_handler),
        )
        .route("/api/execute/composite", post(execute_composite_handler))
        .route(
            "/api/roadmap/project/{id}",
            get(roadmap_project_handler).post(roadmap_project_handler),
        )
        .route("/api/roadmap/milestone", post(roadmap_milestone_handler))
        .route("/api/roadmap/phase", post(roadmap_phase_handler))
        .route("/api/roadmap/goal/{goal_id}", patch(roadmap_goal_handler))
        .route(
            "/api/roadmap/health/{project_id}",
            get(roadmap_health_handler),
        )
        .route("/api/roadmap/predict", post(predict_completion_handler))
        .route(
            "/api/roadmap/delay-alerts",
            post(check_delay_alerts_handler),
        )
        .route("/api/roadmap/project-costs", post(project_costs_handler))
        .route("/api/roadmap/bottlenecks", post(find_bottlenecks_handler))
        .route(
            "/api/roadmap/parallel-execution",
            post(suggest_parallel_execution_handler),
        )
        .route(
            "/api/roadmap/goal-resources",
            post(estimate_goal_resources_handler),
        )
        .route(
            "/api/roadmap/proposals/{project_id}",
            get(list_proposals_handler),
        )
        .route(
            "/api/roadmap/proposals/{project_id}/{proposal_id}",
            get(get_proposal_handler),
        )
        .route(
            "/api/roadmap/proposals/approve",
            post(approve_proposal_handler),
        )
        .route(
            "/api/roadmap/proposals/reject",
            post(reject_proposal_handler),
        )
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
        .route("/api/skills/{id}", get(show_skill_handler))
        .route("/api/tools/execute", post(execute_tool_handler))
        .route(
            "/api/cognitive/delegate",
            post(delegate_cognitive_task_handler),
        )
        .route("/api/feedback", post(submit_feedback_handler))
        .route("/api/experience/capture", post(capture_experience_handler))
        .route("/api/benchmarks", get(list_benchmarks_handler))
        .route("/api/breakers", get(get_breaker_metrics_handler))
        .route("/api/insights", get(get_learning_insights_handler))
        .route("/api/plugins", get(list_plugins_handler))
        .route("/api/plugins/load", post(load_plugin_handler))
        .route("/api/plugins/unload", post(unload_plugin_handler))
        .route("/api/jcode/call", post(call_jcode_handler))
        .route("/api/chaos/start", post(chaos_start_handler))
        .route("/api/chaos/stop", post(chaos_stop_handler))
        .route("/api/chaos/status", get(chaos_status_handler))
        .route("/api/anomaly", get(anomaly_report_handler))
        .route("/api/workspaces/{id}", delete(delete_workspace_handler))
        .route("/openapi.json", get(http_openapi::openapi_json))
        .route("/swagger-ui/*rest", get(http_openapi::swagger_ui_handler))
        .with_state(state)
}

// ── OpenAPI helpers ───────────────────────────────────────────────────────

use super::http_openapi;
