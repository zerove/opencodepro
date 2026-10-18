//! REST handlers backing the `/api/state`, `/api/vector` and `/api/auth`
//! families. These existed only on the SDK/WebUI side (as `/api/v1/...`) with no
//! bridge route behind them, so every caller 404'd.

use super::types::*;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use sisyphus_types::StateStore;
use std::collections::HashMap;
use tracing::instrument;

// ── /api/state ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct StateEntry {
    pub key: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub(super) struct ProjectQuery {
    pub project_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct SetStateBody {
    pub project_id: Option<String>,
    pub value: serde_json::Value,
}

#[instrument(skip(state), fields(project_id))]
pub(super) async fn get_state_handler(
    State(state): State<RestApiState>,
    Query(q): Query<ProjectQuery>,
) -> impl IntoResponse {
    let project_id = q.project_id.unwrap_or_else(|| "default".to_string());
    let store = state.bridge.concrete_store.clone();

    let keys = match store.list_keys_with_prefix(Some(&project_id), "").await {
        Ok(k) => k,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        },
    };

    let mut entries = Vec::with_capacity(keys.len());
    for key in keys {
        if let Ok(Some(value)) = store.get_state_json(&project_id, &key).await {
            entries.push(StateEntry { key, value });
        }
    }
    (StatusCode::OK, Json(entries)).into_response()
}

#[instrument(skip(state), fields(project_id, key = %key))]
pub(super) async fn get_state_key_handler(
    State(state): State<RestApiState>,
    Path(key): Path<String>,
    Query(q): Query<ProjectQuery>,
) -> impl IntoResponse {
    let project_id = q.project_id.unwrap_or_else(|| "default".to_string());
    let store = state.bridge.concrete_store.clone();

    match store.get_state_json(&project_id, &key).await {
        Ok(Some(value)) => (StatusCode::OK, Json(StateEntry { key, value })).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[instrument(skip(state), fields(project_id, key = %key))]
pub(super) async fn put_state_key_handler(
    State(state): State<RestApiState>,
    Path(key): Path<String>,
    Json(body): Json<SetStateBody>,
) -> impl IntoResponse {
    let project_id = body.project_id.unwrap_or_else(|| "default".to_string());
    let store = state.bridge.concrete_store.clone();

    match store.set_state_json(&project_id, &key, body.value).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ── /api/vector/search ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct VectorSearchBody {
    pub query: String,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
}

fn default_top_k() -> usize {
    5
}

#[derive(Debug, Serialize)]
pub(super) struct VectorSearchHit {
    pub key: String,
    pub score: f32,
    pub metadata: serde_json::Value,
}

#[instrument(skip(state), fields(top_k = %body.top_k))]
pub(super) async fn vector_search_handler(
    State(state): State<RestApiState>,
    Json(body): Json<VectorSearchBody>,
) -> impl IntoResponse {
    if body.query.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "query must not be empty"})),
        )
            .into_response();
    }

    let embedding = match state.bridge.embedding_client.embed(&body.query).await {
        Ok(v) if !v.is_empty() => v,
        Ok(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "embedding provider returned an empty vector"})),
            )
                .into_response()
        },
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        },
    };

    let top_k = body.top_k.clamp(1, 100);
    match state.bridge.vector_store.search_similar_filtered(&embedding, top_k, None).await {
        Ok(hits) => {
            let out: Vec<VectorSearchHit> = hits
                .into_iter()
                .map(|h| VectorSearchHit {
                    key: h.key,
                    score: h.score,
                    metadata: h.record.metadata,
                })
                .collect();
            (StatusCode::OK, Json(out)).into_response()
        },
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ── /api/auth/validate ───────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(super) struct AuthValidation {
    pub valid: bool,
    pub message: String,
}

/// Validates a static bearer token / JWT the same way the auth middleware does,
/// so SDK callers can probe credentials before issuing real requests.
#[instrument(skip(state, body))]
pub(super) async fn auth_validate_handler(
    State(state): State<RestApiState>,
    Json(body): Json<HashMap<String, String>>,
) -> impl IntoResponse {
    let Some(api_key) = body.get("api_key") else {
        return (
            StatusCode::BAD_REQUEST,
            Json(AuthValidation {
                valid: false,
                message: "missing api_key".into(),
            }),
        )
            .into_response();
    };

    let valid = if let Some(jwt_config) = &state.jwt_config {
        // JWT 模式：api_key 按 access token 校验，与 jwt_auth_middleware 同路径。
        // 空 secret 与中间件语义一致（视为未启用认证）。
        // 旧行为在 JWT 模式恒 valid=true，SDK 探测到坏凭据也被告知可用。
        if jwt_config.secret.is_empty() {
            true
        } else {
            crate::auth_jwt::JwtService::new(jwt_config.clone())
                .map(|service| service.validate_access_token(api_key).is_ok())
                .unwrap_or(false)
        }
    } else if let Some(expected) = &state.auth_token {
        api_key == expected
    } else {
        true
    };

    let message = if valid {
        "credentials accepted".to_string()
    } else {
        "invalid credentials".to_string()
    };
    let status = if valid {
        StatusCode::OK
    } else {
        StatusCode::UNAUTHORIZED
    };
    (status, Json(AuthValidation { valid, message })).into_response()
}

// ── Error classification ─────────────────────────────────────────────────────

/// Classify a tool/handler failure into an HTTP status.
///
/// Handlers previously answered every failure with 500, which told a caller
/// nothing actionable: "unknown tool" and "server bug" were indistinguishable.
/// Classification is deliberately message-based — the tool layer returns
/// `TypesError::ToolExecutionError` with a `details` string and no error kind,
/// so parsing is the only signal available without changing every tool.
pub(crate) fn status_for_error(msg: &str) -> StatusCode {
    status_for_typed(None, msg)
}

/// Map a failure to an HTTP status, preferring the typed tool cause.
///
/// `err.tool_source()` is `Some` only for [`sisyphus_types::TypesError::Tool`],
/// which keeps its `ToolError` instead of flattening it to a message. Falling
/// back to text matching keeps older call sites working.
pub(crate) fn status_for_typed(
    err: Option<&(dyn std::error::Error + 'static)>,
    msg: &str,
) -> StatusCode {
    let typed = err
        .and_then(|e| e.downcast_ref::<sisyphus_types::TypesError>())
        .and_then(sisyphus_types::TypesError::tool_source);
    if let Some(source) = typed {
        use sisyphus_tools_core::ToolError;
        return match source {
            ToolError::NotFound(_) => StatusCode::NOT_FOUND,
            ToolError::PermissionDenied(_) => StatusCode::FORBIDDEN,
            ToolError::Timeout(_) => StatusCode::GATEWAY_TIMEOUT,
            ToolError::GlobPattern(_) | ToolError::GrepError(_) | ToolError::InvalidInput(_) => {
                StatusCode::BAD_REQUEST
            },
            ToolError::EditNoMatch | ToolError::EditAmbiguous => StatusCode::BAD_REQUEST,
            ToolError::Io(_)
            | ToolError::Generic(_)
            | ToolError::WebError(_)
            | ToolError::TodoError(_)
            | ToolError::CommandFailed { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        };
    }
    // Sandbox failures carry their own category, which says more than the prose
    // does. The split that matters: a *limit* is tunable (the caller can raise a
    // budget), while a `GuestTrap` is the guest's own bug and retrying it fails
    // identically -- so it must not be reported as if more budget would help.
    let sandbox = err
        .and_then(|e| e.downcast_ref::<sisyphus_types::TypesError>())
        .and_then(sisyphus_types::TypesError::sandbox_kind);
    if let Some(kind) = sandbox {
        use sisyphus_types::SandboxErrorKind;
        return match kind {
            // The tool simply is not there (no `.wasm`, no `execute` export).
            SandboxErrorKind::ToolLoadFailed | SandboxErrorKind::MissingExport => {
                StatusCode::NOT_FOUND
            },
            // Matches the long-standing answer for "fuel limit exhausted" in
            // the text path below. 429 would arguably describe a compute budget
            // better, but the two paths must not disagree for the same failure,
            // and 413 is the behaviour callers already depend on.
            SandboxErrorKind::FuelExhausted => StatusCode::PAYLOAD_TOO_LARGE,
            SandboxErrorKind::WallClockExceeded => StatusCode::GATEWAY_TIMEOUT,
            SandboxErrorKind::SandboxUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            SandboxErrorKind::Transport => StatusCode::BAD_GATEWAY,
            // The guest misbehaved; more budget changes nothing.
            SandboxErrorKind::GuestTrap | SandboxErrorKind::Unknown => {
                StatusCode::INTERNAL_SERVER_ERROR
            },
        };
    }
    let m = msg.to_ascii_lowercase();
    if m.contains("unknown builtin tool")
        || m.contains("unknown tool")
        || m.contains("not found")
        // `std::io::Error` renders ENOENT this way; without it a missing tool
        // binary or unreadable path reads as an internal fault.
        || m.contains("no such file or directory")
    {
        return StatusCode::NOT_FOUND;
    }
    if m.contains("already exists") || m.contains("duplicate") || m.contains("conflict") {
        return StatusCode::CONFLICT;
    }
    if m.contains("permission denied")
        || m.contains("forbidden")
        || m.contains("not allowed")
        || m.contains("path traversal")
        || m.contains("absolute path")
    {
        return StatusCode::FORBIDDEN;
    }
    if m.contains("timed out") || m.contains("timeout") {
        return StatusCode::GATEWAY_TIMEOUT;
    }
    if m.contains("fuel") && (m.contains("exhaust") || m.contains("limit")) {
        return StatusCode::PAYLOAD_TOO_LARGE;
    }
    if m.contains("invalid")
        || m.contains("missing")
        || m.contains("required")
        || m.contains("bad request")
        || m.contains("parse")
        || m.contains("deserialize")
    {
        return StatusCode::BAD_REQUEST;
    }
    StatusCode::INTERNAL_SERVER_ERROR
}

#[cfg(test)]
mod error_mapping_tests {
    use super::*;

    #[test]
    fn unknown_tool_is_404_not_500() {
        assert_eq!(
            status_for_error("unknown builtin tool: nope/does_not_exist"),
            StatusCode::NOT_FOUND
        );
    }

    /// 回归：类型化路径必须优先于文本匹配。同一条消息在两条路径下都应得到
    /// 404——否则说明 `ToolError::NotFound` 没被识别，退化成了猜字符串。
    #[test]
    fn typed_tool_error_beats_string_matching() {
        let typed = sisyphus_types::TypesError::tool(
            "read",
            sisyphus_tools_core::ToolError::NotFound("a/b.txt".into()),
        );
        let msg = typed.to_string();
        assert_eq!(status_for_typed(Some(&typed), &msg), StatusCode::NOT_FOUND);

        let denied = sisyphus_types::TypesError::tool(
            "shell",
            sisyphus_tools_core::ToolError::PermissionDenied("disabled".into()),
        );
        assert_eq!(
            status_for_typed(Some(&denied), &denied.to_string()),
            StatusCode::FORBIDDEN
        );
    }

    /// 回归：调用方给错参数必须是 400，不能是 500。
    ///
    /// `InvalidInput` 与 `Generic` 的渲染文本高度相似，只有类型能把两者
    /// 分开：文本匹配看不出「缺参数」和「内部故障」的区别，类型能。
    #[test]
    fn invalid_input_is_a_client_error_not_a_server_error() {
        let bad_input = sisyphus_types::TypesError::tool(
            "json_query",
            sisyphus_tools_core::ToolError::InvalidInput(
                "Missing required param: path (string)".into(),
            ),
        );
        assert_eq!(
            status_for_typed(Some(&bad_input), &bad_input.to_string()),
            StatusCode::BAD_REQUEST
        );

        // 判别性：同样的文案走 Generic 就是 500，所以上面那条不是巧合。
        let generic = sisyphus_types::TypesError::tool(
            "json_query",
            sisyphus_tools_core::ToolError::Generic("Missing required param: path (string)".into()),
        );
        assert_eq!(
            status_for_typed(Some(&generic), &generic.to_string()),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    /// 回归：sandbox 的类别必须驱动 HTTP 状态，而不是靠文案猜。
    ///
    /// 判别性用例故意让类型与文案矛盾：文本说「fuel exhausted」（可调预算，
    /// 应属 429），但类型是 `GuestTrap`——guest 自己撞墙，再给多少燃料都
    /// 一模一样地失败。只有真的信任类型才会得到 500。
    #[test]
    fn sandbox_kind_beats_its_own_rendered_text() {
        let trap = sisyphus_types::TypesError::sandbox(
            "confused",
            sisyphus_types::SandboxErrorKind::GuestTrap,
            "ResourceExhausted: confused stopped the guest (fuel exhausted): wasm trap",
        );
        assert_eq!(
            status_for_typed(Some(&trap), &trap.to_string()),
            StatusCode::INTERNAL_SERVER_ERROR,
            "guest trap must not be reported as a tunable budget problem"
        );
    }

    /// 同一个错误，类别换成真限额时就该是可调的 413——证明上一条不是因为
    /// 所有 sandbox 错误都落到 500。同时钉住两条路径必须给出同一个答案。
    #[test]
    fn fuel_exhaustion_is_a_tunable_budget() {
        let fuel = sisyphus_types::TypesError::sandbox(
            "hog",
            sisyphus_types::SandboxErrorKind::FuelExhausted,
            "ResourceExhausted: hog stopped the guest (fuel exhausted)",
        );
        assert_eq!(
            status_for_typed(Some(&fuel), &fuel.to_string()),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        // 类型路径与文案路径必须一致，否则同一个故障会返回两种状态码。
        assert_eq!(
            status_for_error("fuel limit exhausted"),
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }

    /// 缺 `.wasm` 是 404，而不是 500：工具压根不存在。
    #[test]
    fn typed_tool_load_failure_is_404_not_500() {
        let missing = sisyphus_types::TypesError::sandbox(
            "json_query",
            sisyphus_types::SandboxErrorKind::ToolLoadFailed,
            "Failed to read WASM binary 'json_query' (tools/json_query.wasm): nope",
        );
        assert_eq!(
            status_for_typed(Some(&missing), &missing.to_string()),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn untyped_error_still_falls_back_to_text() {
        let legacy = sisyphus_types::TypesError::ToolExecutionError {
            tool_id: "t".into(),
            details: "unknown builtin tool: x/y".into(),
        };
        assert_eq!(
            status_for_typed(Some(&legacy), &legacy.to_string()),
            StatusCode::NOT_FOUND
        );
    }

    /// Live shape seen from the sandbox when a tool id has no compiled binary.
    #[test]
    fn missing_wasm_binary_is_404() {
        assert_eq!(
            status_for_error(
                "Failed to read WASM binary 'bash' (tools/bash.wasm): No such file or directory"
            ),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn invalid_input_is_400() {
        assert_eq!(
            status_for_error("invalid args: missing field `path`"),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn permission_failures_are_403() {
        assert_eq!(
            status_for_error("permission denied: shell tool disabled"),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            status_for_error("absolute path not allowed: /etc/passwd"),
            StatusCode::FORBIDDEN
        );
    }

    #[test]
    fn timeouts_are_504() {
        assert_eq!(
            status_for_error("Operation timed out: 30s"),
            StatusCode::GATEWAY_TIMEOUT
        );
    }

    #[test]
    fn fuel_exhaustion_is_413() {
        assert_eq!(
            status_for_error("fuel limit exhausted"),
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }

    #[test]
    fn unknown_failures_stay_500() {
        assert_eq!(
            status_for_error("something nobody has classified"),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
