//! API request handlers.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::Value;
use sqlx::Row;
use std::sync::Arc;
use utoipa::{IntoParams, OpenApi};

use crate::AppState;
use crate::models::*;

/// OpenAPI schema
#[derive(OpenApi)]
#[openapi(
    paths(
        health_check,
        get_state,
        set_state,
        delete_state,
        list_state,
    ),
    components(schemas(
        HealthResponse,
        StateResponse,
        SetStateRequest,
        ListStateResponse,
        ErrorResponse,
    )),
    tags(
        (name = "health", description = "Health check endpoints"),
        (name = "state", description = "State management endpoints"),
    )
)]
pub struct ApiDoc;

pub fn openapi() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}

/// Health check
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses(
        (status = 200, description = "Service healthy", body = HealthResponse)
    )
)]
pub async fn health_check() -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// Get state by key
#[utoipa::path(
    get,
    path = "/api/v1/state/{key}",
    tag = "state",
    params(
        ("key" = String, Path, description = "State key")
    ),
    responses(
        (status = 200, description = "State found", body = StateResponse),
        (status = 404, description = "State not found")
    )
)]
pub async fn get_state(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> Result<Json<StateResponse>, StatusCode> {
    let row = sqlx::query("SELECT key, value, created_at, updated_at FROM state WHERE key = ?")
        .bind(&key)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match row {
        Some(row) => {
            let value_str: String = row.get("value");
            let value: Value = serde_json::from_str(&value_str).unwrap_or(Value::Null);
            Ok(Json(StateResponse {
                key: row.get("key"),
                value,
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            }))
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// Set state
#[utoipa::path(
    post,
    path = "/api/v1/state/{key}",
    tag = "state",
    params(
        ("key" = String, Path, description = "State key")
    ),
    request_body = SetStateRequest,
    responses(
        (status = 200, description = "State set", body = StateResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn set_state(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
    Json(req): Json<SetStateRequest>,
) -> Result<Json<StateResponse>, StatusCode> {
    let now = chrono::Utc::now().timestamp();
    let value_str = serde_json::to_string(&req.value).map_err(|_| StatusCode::BAD_REQUEST)?;

    sqlx::query(
        r#"INSERT INTO state (key, value, created_at, updated_at) 
           VALUES (?, ?, ?, ?)
           ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at"#
    )
    .bind(&key)
    .bind(&value_str)
    .bind(now)
    .bind(now)
    .execute(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(StateResponse {
        key,
        value: req.value,
        created_at: now,
        updated_at: now,
    }))
}

/// Delete state
#[utoipa::path(
    delete,
    path = "/api/v1/state/{key}",
    tag = "state",
    params(
        ("key" = String, Path, description = "State key")
    ),
    responses(
        (status = 204, description = "State deleted"),
        (status = 404, description = "State not found")
    )
)]
pub async fn delete_state(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let result = sqlx::query("DELETE FROM state WHERE key = ?")
        .bind(&key)
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if result.rows_affected() > 0 {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// List state params
#[derive(Deserialize, IntoParams)]
pub struct ListStateParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// List state
#[utoipa::path(
    get,
    path = "/api/v1/state",
    tag = "state",
    params(
        ListStateParams
    ),
    responses(
        (status = 200, description = "State list", body = ListStateResponse)
    )
)]
pub async fn list_state(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListStateParams>,
) -> Result<Json<ListStateResponse>, StatusCode> {
    let limit = params.limit.unwrap_or(100);
    let offset = params.offset.unwrap_or(0);

    let rows = sqlx::query("SELECT key, value, created_at, updated_at FROM state ORDER BY updated_at DESC LIMIT ? OFFSET ?")
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let count_row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM state")
        .fetch_one(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let items: Vec<StateResponse> = rows
        .into_iter()
        .map(|row| {
            let value_str: String = row.get("value");
            let value: Value = serde_json::from_str(&value_str).unwrap_or(Value::Null);
            StateResponse {
                key: row.get("key"),
                value,
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            }
        })
        .collect();

    Ok(Json(ListStateResponse {
        items,
        total: count_row.0,
    }))
}
