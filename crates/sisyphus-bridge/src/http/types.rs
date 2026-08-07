//! REST API 请求/响应类型定义（DTO）与共享状态。

use async_trait::async_trait;
use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sisyphus_roadmap::strategic_reflection::StrategicProposal;
use sisyphus_roadmap::{Bottleneck, DelayAlert, GoalResourceEstimate, ParallelizationOpportunity};
use std::path::PathBuf;
use std::sync::Arc;

use crate::MySisyphusBridge;

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
    /// Optional project ID (default: "default"). Can be overridden by `x-project-id` header.
    pub project_id: Option<String>,
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
    /// Optional project ID (default: "default"). Can be overridden by `x-project-id` header.
    pub project_id: Option<String>,
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
    /// Optional project ID (default: "default"). Can be overridden by `x-project-id` header.
    pub project_id: Option<String>,
}

#[derive(Serialize)]
pub struct CaptureExperienceResponse {
    pub success: bool,
    pub error: String,
}

// --- Experience Sharing Types (multi-workspace knowledge transfer) ---
#[derive(Deserialize)]
pub struct ShareExperienceRequest {
    /// Storage key of the experience, e.g. "exp:1700000000".
    pub exp_key: String,
    /// Target project for unshare; ignored when sharing.
    pub project_id: Option<String>,
}

#[derive(Serialize)]
pub struct ShareExperienceResponse {
    pub success: bool,
    pub error: String,
}

#[derive(Serialize)]
pub struct SharedExperiencesResponse {
    pub experiences: Vec<sisyphus_types::experience::ExperienceTuple>,
}

// --- Skill Detail Types ---
#[derive(Serialize)]
pub struct SkillDetailResponse {
    pub found: bool,
    pub skill: Option<sisyphus_types::skill::SkillMetadata>,
}

// --- Skill Market / Peer Registry Types ---
#[derive(Deserialize)]
pub struct PublishSkillRequest {
    /// Remote registry base URL, e.g. "https://registry.sisyphus.ai".
    pub registry_url: String,
    /// ID of the locally installed skill to publish.
    pub skill_id: String,
}

#[derive(Serialize)]
pub struct PublishSkillResponse {
    pub success: bool,
    pub entry: Option<sisyphus_mem::skill_market::SkillRegistryEntry>,
    pub error: String,
}

#[derive(Serialize)]
pub struct SkillSearchResponse {
    pub results: Vec<sisyphus_mem::skill_market::SkillRegistryEntry>,
}

/// Response for the peer protocol used by other Sisyphus instances.
#[derive(Serialize)]
pub struct PeerSkillListResponse {
    pub skills: Vec<sisyphus_types::skill::SkillMetadata>,
}

#[derive(Deserialize)]
pub struct SearchSkillsRequest {
    pub q: String,
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

use sisyphus_types::config::JwtConfig;

#[derive(Clone)]
pub struct RestApiState {
    pub bridge: Arc<MySisyphusBridge>,
    pub session_db: PathBuf,
    /// Optional static bearer token protecting all `/api/*` routes.
    /// `None` = no authentication (development mode).
    pub auth_token: Option<String>,
    /// JWT configuration for token-based authentication.
    /// If `jwt.secret` is set, JWT authentication is enabled for REST API.
    pub jwt_config: Option<JwtConfig>,
}

impl RestApiState {
    pub fn new(bridge: Arc<MySisyphusBridge>, data_dir: PathBuf) -> Self {
        let session_db = data_dir.join("sessions.json");
        Self {
            bridge,
            session_db,
            auth_token: None,
            jwt_config: None,
        }
    }

    /// Create state with optional bearer-token authentication for the REST API.
    pub fn new_with_auth(
        bridge: Arc<MySisyphusBridge>,
        data_dir: PathBuf,
        auth_token: Option<String>,
    ) -> Self {
        let session_db = data_dir.join("sessions.json");
        Self {
            bridge,
            session_db,
            auth_token,
            jwt_config: None,
        }
    }

    /// Create state with JWT configuration for token-based authentication.
    pub fn new_with_jwt(
        bridge: Arc<MySisyphusBridge>,
        data_dir: PathBuf,
        jwt_config: JwtConfig,
    ) -> Self {
        let session_db = data_dir.join("sessions.json");
        Self {
            bridge,
            session_db,
            auth_token: None,
            jwt_config: Some(jwt_config),
        }
    }
}
