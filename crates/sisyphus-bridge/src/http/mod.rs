//! REST API for Sisyphus Bridge (axum HTTP layer alongside gRPC).
//!
//! 模块结构：
//! - `types`：请求/响应 DTO 与共享状态（`RestApiState`）
//! - `handlers`：基础处理器（认知映射、技能、工作区、目标执行、路线图、会话）
//! - `handlers_ext`：扩展处理器（技能市场/对等注册表、工具执行、混沌工程、插件）

pub(crate) mod handlers;
pub(crate) mod handlers_ext;
pub mod types;

pub use types::RestApiState;

use axum::{
    routing::{delete, get, patch, post},
    Router,
};
use tower_http::services::ServeDir;

use super::http_openapi;

use handlers::*;
use handlers_ext::*;

/// Build the axum Router with all REST API routes.
pub fn build_router(state: RestApiState) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .nest_service("/dashboard", ServeDir::new("static/dashboard"))
        .route("/api/skills/available", get(list_available_skills_handler))
        .route("/api/skills/install", post(install_remote_skill_handler))
        .route("/api/skills/installed", get(list_installed_skills_handler))
        .route("/api/skills/search", get(search_skills_handler))
        .route("/api/skills/publish", post(publish_skill_handler))
        .route(
            "/api/skills/remote/:id",
            delete(uninstall_remote_skill_handler),
        )
        .route(
            "/peer/v1/skills",
            get(peer_list_skills_handler).post(peer_publish_skill_handler),
        )
        .route("/peer/v1/skills/:id", get(peer_get_skill_handler))
        .route("/api/cognitive/map/:sop_id", get(get_cognitive_map_handler))
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
            "/api/sops/:id",
            get(get_sop_handler).delete(delete_sop_handler),
        )
        .route("/api/experience/similar", post(experience_similar_handler))
        .route("/api/execute/goal", post(execute_goal_handler))
        .route(
            "/api/execute/goal/:goal_id/status",
            get(goal_status_handler),
        )
        .route("/api/execute/goal/:goal_id/trace", get(goal_trace_handler))
        .route(
            "/api/execute/goal/:goal_id/stream",
            get(goal_stream_handler),
        )
        .route("/api/execute/composite", post(execute_composite_handler))
        .route(
            "/api/roadmap/project/:id",
            get(roadmap_project_handler).post(roadmap_project_handler),
        )
        .route("/api/roadmap/milestone", post(roadmap_milestone_handler))
        .route("/api/roadmap/phase", post(roadmap_phase_handler))
        .route("/api/roadmap/goal/:goal_id", patch(roadmap_goal_handler))
        .route(
            "/api/roadmap/health/:project_id",
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
            "/api/roadmap/proposals/:project_id",
            get(list_proposals_handler),
        )
        .route(
            "/api/roadmap/proposals/:project_id/:proposal_id",
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
        .route("/api/sessions/:id", get(get_session_handler))
        .route("/api/sessions/:id/messages", post(add_message_handler))
        .route("/api/tools", get(list_tools_handler))
        .route("/api/agents", get(list_agents_handler))
        .route("/api/skills", get(list_skills_handler))
        .route("/api/skills/:id", get(show_skill_handler))
        .route("/api/tools/execute", post(execute_tool_handler))
        .route(
            "/api/cognitive/delegate",
            post(delegate_cognitive_task_handler),
        )
        .route("/api/feedback", post(submit_feedback_handler))
        .route("/api/experience/capture", post(capture_experience_handler))
        .route("/api/experience/share", post(share_experience_handler))
        .route("/api/experience/unshare", post(unshare_experience_handler))
        .route(
            "/api/experience/shared",
            get(list_shared_experiences_handler),
        )
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
        .route("/api/workspaces/:id", delete(delete_workspace_handler))
        .route("/openapi.json", get(http_openapi::openapi_json))
        .route("/swagger-ui/*rest", get(http_openapi::swagger_ui_handler))
        .with_state(state)
}
