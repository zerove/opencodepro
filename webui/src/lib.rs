use askama::Template;
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::services::ServeDir;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub bridge_url: String,
    pub client: reqwest::Client,
}

// ── Templates ─────────────────────────────────

#[derive(Template)]
#[template(
    ext = "html",
    source = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Sisyphus — Cognitive Engine</title>
  <script src="https://unpkg.com/htmx.org@2.0.4"></script>
  <style>
    * { margin: 0; padding: 0; box-sizing: border-box; }
    body { font-family: system-ui, sans-serif; background: #0d1117; color: #c9d1d9; min-height: 100vh; }
    .container { max-width: 960px; margin: 0 auto; padding: 40px 20px; }
    h1 { font-size: 2.5rem; margin-bottom: 8px; color: #58a6ff; }
    .subtitle { color: #8b949e; margin-bottom: 40px; }
    .card { background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 24px; margin-bottom: 16px; }
    .card h2 { color: #58a6ff; margin-bottom: 12px; font-size: 1.2rem; }
    .stat { display: flex; justify-content: space-between; padding: 8px 0; border-bottom: 1px solid #21262d; }
    .stat:last-child { border-bottom: none; }
    .label { color: #8b949e; }
    .value { color: #f0f6fc; font-weight: 600; }
    .status-ok { color: #3fb950; }
    .status-err { color: #f85149; }
    button { background: #238636; color: #fff; border: none; padding: 8px 16px; border-radius: 6px; cursor: pointer; font-size: 14px; }
    button:hover { background: #2ea043; }
    input { background: #0d1117; border: 1px solid #30363d; color: #c9d1d9; padding: 8px 12px; border-radius: 6px; font-size: 14px; width: 300px; }
  </style>
</head>
<body>
  <div class="container">
    <h1>&#9889; Sisyphus</h1>
    <p class="subtitle">Cognitive Execution Engine — v{{ version }}</p>
    <div class="card">
      <h2>Status</h2>
      <div hx-get="/api/health" hx-trigger="load, every 10s" hx-swap="innerHTML">Loading...</div>
    </div>
    <div class="card">
      <h2>Sessions</h2>
      <div hx-get="/api/sessions" hx-trigger="load, every 5s" hx-swap="innerHTML">Loading...</div>
    </div>
  </div>
</body>
</html>"#
)]
struct IndexTemplate {
    version: String,
}

// ── API Types ─────────────────────────────────

#[derive(Serialize, Deserialize)]
struct HealthResponse {
    status: String,
    version: String,
    uptime_secs: u64,
}

#[derive(Serialize, Deserialize)]
struct Session {
    id: String,
    name: String,
    message_count: usize,
}

#[derive(Deserialize)]
struct SearchRequest {
    query: String,
    top_k: Option<usize>,
}

#[derive(Serialize, Deserialize)]
struct SearchResult {
    id: String,
    score: f64,
    content: String,
}

#[derive(Deserialize)]
struct ExecuteRequest {
    sop_id: String,
}

#[derive(Serialize, Deserialize)]
struct ExecuteResult {
    sop_id: String,
    status: String,
    iterations: u32,
    deviation: f64,
}

// ── Page Handlers ─────────────────────────────

async fn index() -> Html<String> {
    let tmpl = IndexTemplate {
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    Html(tmpl.render().unwrap_or_else(|_| "Template error".to_string()))
}

async fn dashboard(State(state): State<Arc<AppState>>) -> Response {
    let client = &state.client;
    let base = &state.bridge_url;

    let health = async {
        let resp = client
            .get(format!("{base}/health"))
            .send()
            .await
            .ok()?;
        resp.json::<HealthResponse>().await.ok()
    }
    .await;

    let sessions = async {
        let resp = client
            .get(format!("{base}/api/v1/sessions"))
            .send()
            .await
            .ok()?;
        resp.json::<Vec<Session>>().await.ok()
    }
    .await;

    let h = health.unwrap_or(HealthResponse {
        status: "unreachable".into(),
        version: "unknown".into(),
        uptime_secs: 0,
    });
    let s = sessions.unwrap_or_default();

    let html = format!(
        r#"<div class="stat"><span class="label">Status</span><span class="value">{}</span></div>
<div class="stat"><span class="label">Version</span><span class="value">{}</span></div>
<div class="stat"><span class="label">Uptime</span><span class="value">{}s</span></div>
<div class="stat"><span class="label">Sessions</span><span class="value">{}</span></div>"#,
        h.status, h.version, h.uptime_secs, s.len()
    );

    Html(html).into_response()
}

// ── API Handlers ──────────────────────────────

async fn api_health(
    State(state): State<Arc<AppState>>,
) -> Result<Json<HealthResponse>, StatusCode> {
    let resp = state
        .client
        .get(format!("{}/health", state.bridge_url))
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let health: HealthResponse = resp.json().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(health))
}

async fn api_sessions(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<Session>>, StatusCode> {
    let resp = state
        .client
        .get(format!("{}/api/v1/sessions", state.bridge_url))
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let sessions: Vec<Session> = resp.json().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(sessions))
}

async fn api_search(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SearchRequest>,
) -> Result<Json<Vec<SearchResult>>, StatusCode> {
    let body = serde_json::json!({
        "query": req.query,
        "top_k": req.top_k.unwrap_or(10),
    });

    let resp = state
        .client
        .post(format!("{}/api/v1/vector/search", state.bridge_url))
        .json(&body)
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let results: Vec<SearchResult> = resp.json().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(results))
}

async fn api_execute(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ExecuteRequest>,
) -> Result<Json<ExecuteResult>, StatusCode> {
    let body = serde_json::json!({
        "sop_id": req.sop_id,
        "context": {},
    });

    let resp = state
        .client
        .post(format!("{}/api/v1/execute", state.bridge_url))
        .json(&body)
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let result: ExecuteResult = resp.json().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(result))
}

// ── Router ────────────────────────────────────

pub fn app(bridge_url: String) -> Router {
    let state = Arc::new(AppState {
        bridge_url,
        client: reqwest::Client::new(),
    });

    Router::new()
        .route("/", get(index))
        .route("/dashboard", get(dashboard))
        .route("/api/health", get(api_health))
        .route("/api/sessions", get(api_sessions))
        .route("/api/search", post(api_search))
        .route("/api/execute", post(api_execute))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state)
}
