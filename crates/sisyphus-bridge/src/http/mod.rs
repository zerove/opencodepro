//! REST API for Sisyphus Bridge (axum HTTP layer alongside gRPC).
//!
//! 模块结构：
//! - `types`：请求/响应 DTO 与共享状态（`RestApiState`）
//! - `handlers`：基础处理器（认知映射、技能、工作区、目标执行、路线图、会话）
//! - `handlers_ext`：扩展处理器（技能市场/对等注册表、工具执行、混沌工程、插件）
//! - `sse_handlers`：Server-Sent Events 实时流式端点

pub(crate) mod handlers;
pub(crate) mod handlers_ext;
pub(crate) mod handlers_platform;
pub(crate) mod sse_handlers;
pub mod types;

pub use types::RestApiState;

use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
    Router,
};
use std::convert::Infallible;
use tower_http::services::ServeDir;
use tracing::Instrument;

use super::http_openapi;

use handlers::*;
use handlers_ext::*;
use handlers_platform::*;
use sse_handlers::*;

/// Trace id 中间件（T4.4）。
///
/// 生成或透传 trace id，写入 request extensions、tracing span 与响应头，
/// 使「入口请求 → 子步骤（SOP / 工具 / LLM / 事件流）」可按同一 id 串联查询。
async fn trace_id_middleware(mut request: Request, next: Next) -> Response {
    let trace_id = request
        .headers()
        .get(sisyphus_types::trace::TRACE_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(sisyphus_types::trace::sanitize_trace_id)
        .unwrap_or_else(sisyphus_types::trace::generate_trace_id);

    request.extensions_mut().insert(sisyphus_types::trace::TraceId(trace_id.clone()));

    let span = tracing::info_span!("http_request", trace_id = %trace_id);
    let mut response = next.run(request).instrument(span).await;

    if let Ok(value) = axum::http::HeaderValue::from_str(&trace_id) {
        response.headers_mut().insert(
            axum::http::HeaderName::from_static(sisyphus_types::trace::TRACE_ID_HEADER),
            value,
        );
    }
    response
}

/// JWT authentication middleware for the REST API.
/// Validates Bearer token using JWT if jwt_config is configured.
async fn jwt_auth_middleware(
    State(state): State<RestApiState>,
    request: Request,
    next: Next,
) -> Result<Response, Infallible> {
    if let Some(jwt_config) = &state.jwt_config {
        if jwt_config.secret.is_empty() {
            // JWT config exists but secret is empty - treat as no auth
            return Ok(next.run(request).await);
        }

        // Try to create JWT service to validate
        let jwt_service = match crate::auth_jwt::JwtService::new(jwt_config.clone()) {
            Ok(s) => s,
            Err(_) => {
                let body = axum::Json(serde_json::json!({
                    "error": "Internal server error: JWT configuration invalid",
                    "code": 500
                }));
                return Ok((StatusCode::INTERNAL_SERVER_ERROR, body).into_response());
            },
        };

        let provided = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|s| s.trim());

        match provided {
            Some(token) => match jwt_service.validate_access_token(token) {
                Ok(_) => Ok(next.run(request).await),
                Err(_) => {
                    let body = axum::Json(serde_json::json!({
                        "error": "Unauthorized: Invalid or expired token",
                        "code": 401
                    }));
                    Ok((StatusCode::UNAUTHORIZED, body).into_response())
                },
            },
            None => {
                let body = axum::Json(serde_json::json!({
                    "error": "Unauthorized: Missing Authorization header",
                    "code": 401
                }));
                Ok((StatusCode::UNAUTHORIZED, body).into_response())
            },
        }
    } else {
        // No JWT config - pass through
        Ok(next.run(request).await)
    }
}

/// Bearer-token authentication middleware for the REST API.
///
/// When `RestApiState.auth_token` is `Some`, every request under the wrapped
/// router must carry `Authorization: Bearer <token>`. Otherwise the request is
/// rejected with 401. When the token is `None` (development mode), requests
/// pass through untouched.
async fn bearer_auth_middleware(
    State(state): State<RestApiState>,
    request: Request,
    next: Next,
) -> Result<Response, Infallible> {
    if let Some(expected) = state.auth_token.as_deref() {
        let provided = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|s| s.trim());

        match provided {
            Some(actual) if actual == expected => Ok(next.run(request).await),
            _ => {
                let body = axum::Json(serde_json::json!({
                    "error": "Unauthorized",
                    "code": 401
                }));
                Ok((StatusCode::UNAUTHORIZED, body).into_response())
            },
        }
    } else {
        Ok(next.run(request).await)
    }
}

/// Build the axum Router with all REST API routes.
pub fn build_router(state: RestApiState) -> Router {
    let public_routes = Router::new()
        .route("/health", get(health_handler))
        .route("/health/live", get(health_live_handler))
        .route("/health/ready", get(health_ready_handler))
        .route("/health/deep", get(health_deep_handler))
        .route("/openapi.json", get(http_openapi::openapi_json))
        .route("/swagger-ui/*rest", get(http_openapi::swagger_ui_handler))
        .route("/api/events/health", get(sse_health_handler))
        .route("/api/metrics", get(metrics_handler))
        // 凭据探测端点必须公开可达：JWT 模式下调用方尚无有效 token，
        // 若置于受保护路由则中间件先于 handler 拒绝，探测永远失败。
        .route("/api/auth/validate", post(auth_validate_handler));

    let protected_routes = Router::new()
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
        .route("/api/cognitive/map/:sop_id/stream", get(stream_cognitive_map_handler))
        .route("/api/cognitive/map/:sop_id/intervene", post(intervene_cognitive_map_handler))
        .route("/api/cognitive/trace", get(cognitive_trace_handler))
        .route(
            "/api/cognitive/trace/history",
            get(cognitive_trace_history_handler),
        )
        .route(
            "/api/cognitive/summary",
            get(cognitive_trace_summary_handler),
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
            "/api/state",
            get(get_state_handler),
        )
        .route(
            "/api/state/:key",
            get(get_state_key_handler).put(put_state_key_handler),
        )
        .route("/api/vector/search", post(vector_search_handler))
        .route(
            "/api/sops/:id",
            get(get_sop_handler).delete(delete_sop_handler),
        )
        .route("/api/experience/similar", post(experience_similar_handler))
        .route("/api/strategy/weights", get(strategy_weights_handler))
        .route("/api/cluster/status", get(cluster_status_handler))
        .route(
            "/api/cluster/control/leases/:key",
            get(cluster_control_lease_get_handler),
        )
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
        .route(
            "/api/roadmap/dashboard/:project_id",
            get(roadmap_dashboard_handler),
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
        .route("/api/mcp/resources", get(list_mcp_resources_handler))
        .route("/api/mcp/prompts", get(list_mcp_prompts_handler))
        .route("/api/mcp/resources/read", get(mcp_read_resource_handler))
        .route("/api/mcp/prompts/get", post(mcp_get_prompt_handler))
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
        // SSE endpoints
        .route("/api/events", get(sse_handler))
        .route("/api/events/broadcast", post(broadcast_event_handler));

    // Apply auth middleware: JWT takes precedence over static bearer token
    let router = if state.jwt_config.is_some() {
        protected_routes
            .route_layer(middleware::from_fn_with_state(
                state.clone(),
                jwt_auth_middleware,
            ))
            .merge(public_routes)
    } else if state.auth_token.is_some() {
        protected_routes
            .route_layer(middleware::from_fn_with_state(
                state.clone(),
                bearer_auth_middleware,
            ))
            .merge(public_routes)
    } else {
        protected_routes.merge(public_routes)
    };

    router
        .layer(middleware::from_fn(trace_id_middleware))
        .nest_service("/dashboard", ServeDir::new("static/dashboard"))
        .with_state(state)
}

#[cfg(test)]
#[path = "trace_middleware_tests.rs"]
mod trace_middleware_tests;
