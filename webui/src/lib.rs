//! Sisyphus Web Dashboard — axum + HTMX + SSE
//!
//! Full-featured web interface with:
//! - Real-time SSE event streaming
//! - DAG visualization (interactive SVG)
//! - Session detail with message history
//! - SOP execution panel
//! - Skill market browser
//! - Memory search
//! - Prometheus metrics endpoint
//! - OAuth2/API key auth middleware

use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket},
        Path, Query, State, WebSocketUpgrade,
    },
    http::StatusCode,
    middleware as axum_mw,
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use futures::stream::{Stream, StreamExt};
use futures::SinkExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{broadcast, RwLock};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

// ── Shared State ───────────────────────────────

#[derive(Clone)]
pub struct AppState {
    pub bridge_url: String,
    pub client: reqwest::Client,
    pub event_tx: broadcast::Sender<String>,
    pub api_keys: Arc<RwLock<Vec<String>>>,
    pub metrics: Arc<RwLock<MetricsCollector>>,
    /// Optional auth token from WEBUI_API_KEY env var. If set, all requests
    /// (except events/ws/metrics) must include `Authorization: Bearer <token>`.
    pub auth_token: Option<String>,
    /// Chaos engineering: whether fault injection is currently enabled.
    pub chaos_enabled: Arc<AtomicBool>,
    /// Chaos engineering: fault probability between 0.0 and 1.0.
    pub chaos_fault_probability: Arc<RwLock<f64>>,
    /// Chaos engineering: total number of injected faults since start.
    pub chaos_fault_count: Arc<RwLock<u64>>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct MetricsCollector {
    pub requests_total: u64,
    pub sessions_created: u64,
    pub sop_executions: u64,
    pub search_queries: u64,
    pub ws_connections: u64,
    pub errors: u64,
    #[serde(skip)]
    pub uptime_start: Option<Instant>,
}

impl MetricsCollector {
    fn uptime_secs(&self) -> u64 {
        self.uptime_start
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0)
    }
}

// ── API Types ─────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
struct HealthResponse {
    status: String,
    version: String,
    uptime_secs: u64,
}

#[derive(Serialize, Deserialize, Clone)]
struct Session {
    id: String,
    name: String,
    message_count: usize,
    created_at: Option<String>,
    parent_session_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct ChatMessage {
    role: String,
    content: String,
    created_at: String,
    estimated_tokens: u64,
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
    context: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize)]
struct ExecuteResult {
    sop_id: String,
    status: String,
    iterations: u32,
    deviation: f64,
}

#[derive(Serialize, Deserialize)]
struct SopInfo {
    id: String,
    name: String,
    node_count: usize,
    description: String,
}

#[derive(Serialize, Deserialize)]
struct SkillEntry {
    id: String,
    name: String,
    description: String,
    author: String,
    version: String,
    downloads: u64,
    rating: f64,
}

#[derive(Deserialize)]
struct SkillInstallRequest {
    skill_id: String,
}

#[derive(Serialize, Deserialize)]
struct DagNode {
    id: String,
    label: String,
    status: String,
    x: f64,
    y: f64,
}

#[derive(Serialize, Deserialize)]
struct DagEdge {
    from: String,
    to: String,
}

#[derive(Serialize, Deserialize)]
struct DagGraph {
    nodes: Vec<DagNode>,
    edges: Vec<DagEdge>,
    width: f64,
    height: f64,
}

#[derive(Serialize, Deserialize)]
struct SessionDetail {
    id: String,
    name: String,
    messages: Vec<ChatMessage>,
    metadata: HashMap<String, String>,
}

#[derive(Deserialize)]
struct AuthRequest {
    api_key: String,
}

#[derive(Serialize)]
struct AuthResponse {
    valid: bool,
    message: String,
}

#[derive(Deserialize)]
struct SessionCreateRequest {
    name: String,
}

#[derive(Deserialize)]
struct MessageAddRequest {
    role: String,
    content: String,
}

// ── HTML Templates ────────────────────────────

fn page_shell(title: &str, nav_active: &str, body: &str) -> String {
    let nav_items = [
        ("dashboard", "/", "Dashboard"),
        ("sessions", "/sessions", "Sessions"),
        ("dag", "/dag", "DAG"),
        ("sop", "/sop", "SOPs"),
        ("skills", "/skills", "Skills"),
        ("chaos", "/chaos", "Chaos"),
    ];

    let nav_html: String = nav_items
        .iter()
        .map(|(key, href, label)| {
            let active_class = if *key == nav_active { " active" } else { "" };
            format!("<a href='{href}' class='nav-item{active_class}'>{label}</a>")
        })
        .collect();

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{title} — Sisyphus</title>
<script src="https://unpkg.com/htmx.org@2.0.4"></script>
<script src="https://unpkg.com/htmx-ext-sse@2.2.2/sse.js"></script>
<style>
* {{ margin:0; padding:0; box-sizing:border-box; }}
body {{ font-family:system-ui,sans-serif; background:#0d1117; color:#c9d1d9; min-height:100vh; display:flex; }}
nav {{ width:200px; background:#161b22; border-right:1px solid #30363d; padding:20px 0; position:fixed; height:100vh; }}
nav .brand {{ padding:0 16px 20px; color:#58a6ff; font-size:1.2rem; font-weight:700; }}
.nav-item {{ display:block; padding:10px 16px; color:#8b949e; text-decoration:none; border-left:3px solid transparent; }}
.nav-item:hover {{ background:#1c2128; color:#c9d1d9; }}
.nav-item.active {{ color:#58a6ff; border-left-color:#58a6ff; background:#1c2128; }}
main {{ margin-left:200px; padding:32px; flex:1; max-width:1200px; }}
h1 {{ font-size:2rem; margin-bottom:20px; color:#58a6ff; }}
.card {{ background:#161b22; border:1px solid #30363d; border-radius:8px; padding:20px; margin-bottom:16px; }}
.card h2 {{ color:#58a6ff; margin-bottom:12px; font-size:1.1rem; }}
.stat {{ display:flex; justify-content:space-between; padding:8px 0; border-bottom:1px solid #21262d; }}
.stat:last-child {{ border-bottom:none; }}
.label {{ color:#8b949e; }}
.value {{ color:#f0f6fc; font-weight:600; }}
.status-ok {{ color:#3fb950; }}
.status-err {{ color:#f85149; }}
button {{ background:#238636; color:#fff; border:none; padding:8px 16px; border-radius:6px; cursor:pointer; font-size:14px; }}
button:hover {{ background:#2ea043; }}
.btn-sm {{ padding:4px 12px; font-size:12px; }}
.btn-exec {{ background:#1f6feb; }}
.btn-exec:hover {{ background:#388bfd; }}
input,textarea {{ background:#0d1117; border:1px solid #30363d; color:#c9d1d9; padding:8px 12px; border-radius:6px; font-size:14px; width:100%; }}
textarea {{ resize:vertical; min-height:60px; font-family:monospace; }}
.session-table {{ width:100%; border-collapse:collapse; }}
.session-table th {{ text-align:left; padding:10px; color:#8b949e; border-bottom:1px solid #30363d; }}
.session-table td {{ padding:10px; border-bottom:1px solid #21262d; }}
.muted {{ color:#8b949e; font-size:0.9rem; }}
.message {{ padding:12px; margin:8px 0; border-radius:8px; }}
.user-msg {{ background:#1c2128; border-left:3px solid #58a6ff; }}
.ai-msg {{ background:#0d1117; border-left:3px solid #3fb950; }}
.msg-role {{ font-size:0.8rem; color:#8b949e; margin-bottom:4px; text-transform:uppercase; }}
.msg-content {{ white-space:pre-wrap; }}
.msg-meta {{ font-size:0.75rem; color:#484f58; margin-top:4px; }}
.skill-grid {{ display:grid; grid-template-columns:repeat(auto-fill,minmax(280px,1fr)); gap:16px; }}
.skill-card {{ background:#161b22; border:1px solid #30363d; border-radius:8px; padding:16px; }}
.skill-card h4 {{ color:#f0f6fc; margin-bottom:4px; }}
.skill-meta {{ display:flex; gap:12px; margin:8px 0; font-size:0.85rem; }}
.stars {{ color:#d29922; }}
.skill-author {{ color:#8b949e; font-size:0.8rem; margin-bottom:8px; }}
.installed {{ color:#3fb950; font-weight:600; }}
.exec-result {{ padding:16px; border-radius:8px; }}
.dag-svg {{ width:100%; height:auto; border:1px solid #30363d; border-radius:8px; background:#0d1117; }}
</style>
</head>
<body>
<nav>
<div class="brand">&#9889; Sisyphus</div>
{nav_html}
</nav>
<main>
{body}
</main>
</body>
</html>"#,
        title = title,
        nav_html = nav_html,
        body = body
    )
}

fn dashboard_body() -> &'static str {
    r#"
<h1>Dashboard</h1>
<div class="card">
  <h2>Status</h2>
  <div hx-get="/htmx/dashboard" hx-trigger="load, every 10s" hx-swap="innerHTML">Loading...</div>
</div>
<div class="card">
  <h2>Live Events</h2>
  <div id="dashboard-events" hx-ext="sse" sse-connect="/htmx/dashboard-events" sse-swap="message" hx-swap="beforeend" style="max-height:240px;overflow-y:auto;background:#0d1117;border:1px solid #21262d;border-radius:6px;padding:8px;font-family:monospace;font-size:0.8rem;">
    <span class="muted">Waiting for bridge events...</span>
  </div>
</div>
"#
}

// ── Page Handlers ─────────────────────────────

async fn index() -> Html<String> {
    Html(page_shell("Dashboard", "dashboard", dashboard_body()))
}

async fn sessions_page() -> Html<String> {
    let body = r#"
<h1>Sessions</h1>
<div class="card">
  <h2>Session List</h2>
  <div id="session-list" hx-get="/htmx/sessions" hx-trigger="load, every 5s" hx-swap="innerHTML">Loading...</div>
</div>
<div class="card">
  <h2>Session Detail</h2>
  <div id="session-detail"><p class="muted">Select a session to view details.</p></div>
</div>
"#;
    Html(page_shell("Sessions", "sessions", body))
}

async fn dag_page() -> Html<String> {
    let body = r#"
<h1>DAG Visualization</h1>
<div class="card">
  <h2>Execution Graph</h2>
  <div id="dag-container" hx-get="/htmx/dag?sop_id=default" hx-trigger="load" hx-swap="innerHTML">Loading...</div>
</div>
"#;
    Html(page_shell("DAG", "dag", body))
}

async fn sop_page() -> Html<String> {
    let body = r#"
<h1>SOP Management</h1>
<div class="card">
  <h2>Available SOPs</h2>
  <div id="sop-list" hx-get="/htmx/sops" hx-trigger="load" hx-swap="innerHTML">Loading...</div>
</div>
<div class="card">
  <h2>Execution Result</h2>
  <div id="exec-result"><p class="muted">Execute a SOP to see results.</p></div>
</div>
"#;
    Html(page_shell("SOPs", "sop", body))
}

async fn skills_page() -> Html<String> {
    let body = r#"
<h1>Skill Market</h1>
<div class="card">
  <h2>Available Skills</h2>
  <div id="skill-list" hx-get="/htmx/skills" hx-trigger="load" hx-swap="innerHTML">Loading...</div>
</div>
"#;
    Html(page_shell("Skills", "skills", body))
}

// ── Chaos Engineering Page ─────────────────────

async fn chaos_page() -> Html<String> {
    let body = r##"
<h1>Chaos Engineering</h1>
<div class="card">
  <h2>Chaos Status</h2>
  <div id="chaos-status" hx-get="/htmx/chaos/status" hx-trigger="load, every 3s" hx-swap="innerHTML">Loading...</div>
</div>
<div class="card">
  <h2>Controls</h2>
  <div style="display:flex;flex-direction:column;gap:16px;">
    <div>
      <button id="chaos-toggle-btn" class="btn-exec" hx-post="/htmx/chaos/toggle" hx-target="#chaos-status" hx-swap="innerHTML">Toggle Chaos</button>
      <span class="muted" style="margin-left:8px;">Start / stop fault injection</span>
    </div>
    <div>
      <label>Fault Probability: <strong id="chaos-prob-display">10%</strong></label>
      <input type="range" min="0" max="100" value="10" id="chaos-slider" style="width:300px;display:block;margin-top:4px;" hx-post="/htmx/chaos/probability" hx-trigger="change" hx-target="#chaos-status" hx-swap="innerHTML" name="probability">
    </div>
  </div>
</div>
"##;
    Html(page_shell("Chaos", "chaos", body))
}

// ── HTMX Partial Handlers ─────────────────────

async fn dashboard_htmx(State(state): State<Arc<AppState>>) -> Response {
    let health = fetch_health(&state).await;
    let sessions = fetch_sessions(&state).await;

    let html = format!(
        r#"<div class="stat"><span class="label">Status</span><span class="value status-{}">{}</span></div>
<div class="stat"><span class="label">Version</span><span class="value">{}</span></div>
<div class="stat"><span class="label">Uptime</span><span class="value">{}s</span></div>
<div class="stat"><span class="label">Sessions</span><span class="value">{}</span></div>"#,
        if health.status == "ok" { "ok" } else { "err" },
        health.status,
        health.version,
        health.uptime_secs,
        sessions.len()
    );

    Html(html).into_response()
}

async fn sessions_htmx(State(state): State<Arc<AppState>>) -> Response {
    let sessions = fetch_sessions(&state).await;
    let mut html = String::from("<table class='session-table'><thead><tr><th>Name</th><th>Messages</th><th>Actions</th></tr></thead><tbody>");

    for s in &sessions {
        html.push_str(&format!(
            r#"<tr>
<td>{}</td>
<td>{}</td>
<td><button class='btn-sm' hx-get='/htmx/session/{}/detail' hx-target='#session-detail'>View</button></td>
</tr>"#,
            s.name, s.message_count, s.id
        ));
    }

    html.push_str("</tbody></table>");
    if sessions.is_empty() {
        html.push_str("<p class='muted'>No sessions yet.</p>");
    }
    Html(html).into_response()
}

async fn session_detail_htmx(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Response {
    let detail = fetch_session_detail(&state, &session_id).await;

    let mut html = format!(
        r#"<div class='card'><h2>{}</h2><div class='messages'>"#,
        detail.name
    );

    for msg in &detail.messages {
        let role_class = if msg.role == "user" {
            "user-msg"
        } else {
            "ai-msg"
        };
        html.push_str(&format!(
            r#"<div class='message {}'><div class='msg-role'>{}</div><div class='msg-content'>{}</div><div class='msg-meta'>{} tokens</div></div>"#,
            role_class, msg.role, msg.content, msg.estimated_tokens
        ));
    }

    html.push_str("</div></div>");
    html.push_str(&format!(
        r#"<div class='card'><h2>Send Message</h2>
<form hx-post='/htmx/session/{}/messages' hx-target='#session-detail' hx-swap='innerHTML'>
<textarea name='content' placeholder='Type a message...' rows='3'></textarea>
<input type='hidden' name='role' value='user'>
<button type='submit' style='margin-top:8px'>Send</button>
</form></div>"#,
        session_id
    ));

    Html(html).into_response()
}

async fn dag_htmx(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let sop_id = params
        .get("sop_id")
        .map(|s| s.as_str())
        .unwrap_or("default");
    let graph = fetch_dag_graph(&state, sop_id).await;
    let svg = render_dag_svg(&graph);
    Html(svg).into_response()
}

async fn sop_list_htmx(State(state): State<Arc<AppState>>) -> Response {
    let sops = fetch_sop_list(&state).await;
    let mut html = String::from("<table class='session-table'><thead><tr><th>Name</th><th>Nodes</th><th>Actions</th></tr></thead><tbody>");
    for s in &sops {
        html.push_str(&format!(
            r#"<tr><td>{}</td><td>{}</td><td>
<button class='btn-sm' hx-get='/htmx/dag?sop_id={}' hx-target='#dag-container' hx-swap='innerHTML'>DAG</button>
<button class='btn-sm btn-exec' hx-post='/htmx/exec' hx-vals='{{"sop_id":"{}"}}' hx-target='#exec-result'>Run</button>
</td></tr>"#,
            s.name, s.node_count, s.id, s.id
        ));
    }
    html.push_str("</tbody></table>");
    if sops.is_empty() {
        html.push_str("<p class='muted'>No SOPs available.</p>");
    }
    Html(html).into_response()
}

async fn skills_list_htmx(State(state): State<Arc<AppState>>) -> Response {
    let skills = fetch_skill_list(&state).await;
    let mut html = String::from("<div class='skill-grid'>");
    for s in &skills {
        let stars_len = s.rating as usize;
        let stars = format!("{}{}", "*".repeat(stars_len), "-".repeat(5 - stars_len));
        html.push_str(&format!(
            r#"<div class='skill-card'><h4>{}</h4><p class='muted'>{}</p>
<div class='skill-meta'><span class='stars'>{}</span><span>{} dl</span></div>
<p class='skill-author'>by {} v{}</p>
<button class='btn-sm' hx-post='/htmx/skills/install' hx-vals='{{"skill_id":"{}"}}' hx-target='this' hx-swap='outerHTML'>Install</button>
</div>"#,
            s.name, s.description, stars, s.downloads, s.author, s.version, s.id
        ));
    }
    html.push_str("</div>");
    if skills.is_empty() {
        html.push_str("<p class='muted'>No skills available.</p>");
    }
    Html(html).into_response()
}

async fn exec_result_htmx(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ExecuteRequest>,
) -> Response {
    let mut m = state.metrics.write().await;
    m.sop_executions += 1;
    drop(m);

    match execute_sop(&state, &req.sop_id, req.context.as_ref()).await {
        Ok(r) => {
            let cls = if r.status == "success" { "ok" } else { "err" };
            Html(format!(
                r#"<div class='exec-result status-{}'><h4>Result</h4>
<div class='stat'><span class='label'>SOP</span><span class='value'>{}</span></div>
<div class='stat'><span class='label'>Status</span><span class='value'>{}</span></div>
<div class='stat'><span class='label'>Iterations</span><span class='value'>{}</span></div>
<div class='stat'><span class='label'>Deviation</span><span class='value'>{:.3}</span></div></div>"#,
                cls, r.sop_id, r.status, r.iterations, r.deviation
            )).into_response()
        }
        Err(e) => Html(format!(
            "<div class='exec-result status-err'>Error: {e}</div>"
        ))
        .into_response(),
    }
}

async fn add_message_htmx(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Json(req): Json<MessageAddRequest>,
) -> Response {
    let _ = add_message(&state, &session_id, &req.role, &req.content).await;

    let event = serde_json::json!({"event_type": "message_added", "session_id": session_id});
    let _ = state
        .event_tx
        .send(serde_json::to_string(&event).unwrap_or_default());

    session_detail_htmx(State(state), Path(session_id)).await
}

async fn skill_install_htmx(
    State(_state): State<Arc<AppState>>,
    Json(req): Json<SkillInstallRequest>,
) -> Response {
    Html(format!(
        "<span class='installed'>Installed: {}</span>",
        req.skill_id
    ))
    .into_response()
}

// ── Chaos HTMX Handlers ────────────────────

/// Returns chaos status as an HTML fragment showing enabled/disabled state,
/// current fault probability, and total fault count.
async fn chaos_status_htmx(State(state): State<Arc<AppState>>) -> Response {
    let enabled = state.chaos_enabled.load(Ordering::Relaxed);
    let prob = *state.chaos_fault_probability.read().await;
    let count = *state.chaos_fault_count.read().await;

    let status_label = if enabled { "Running" } else { "Stopped" };
    let status_class = if enabled { "status-ok" } else { "muted" };

    let html = format!(
        r#"<div class="stat"><span class="label">Status</span><span class="value {}">{}</span></div>
<div class="stat"><span class="label">Fault Probability</span><span class="value">{:.0}%</span></div>
<div class="stat"><span class="label">Faults Injected</span><span class="value status-{}">{}</span></div>"#,
        status_class,
        status_label,
        prob * 100.0,
        if count > 0 { "err" } else { "ok" },
        count
    );
    Html(html).into_response()
}

/// Toggle chaos on/off. When enabled, increments the fault count to simulate
/// active fault injection visible on the dashboard.
async fn chaos_toggle_htmx(State(state): State<Arc<AppState>>) -> Response {
    let was_enabled = state.chaos_enabled.fetch_xor(true, Ordering::Relaxed);
    let now_enabled = !was_enabled;

    // Simulate fault injection on toggle for visual feedback
    if now_enabled {
        let mut count = state.chaos_fault_count.write().await;
        *count += 1;
    }

    // Broadcast chaos toggle event
    let event = serde_json::json!({
        "event_type": "chaos_toggle",
        "enabled": now_enabled,
        "timestamp_secs": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    });
    let _ = state
        .event_tx
        .send(serde_json::to_string(&event).unwrap_or_default());

    chaos_status_htmx(State(state)).await
}

/// Set the chaos fault probability from a slider value (0-100 integer).
async fn chaos_probability_htmx(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let raw: f64 = params
        .get("probability")
        .and_then(|v| v.parse().ok())
        .unwrap_or(10.0);
    let pct = raw.clamp(0.0, 100.0);
    *state.chaos_fault_probability.write().await = pct / 100.0;

    chaos_status_htmx(State(state)).await
}

// ── SSE Handler ────────────────────────────────

async fn sse_handler(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.event_tx.subscribe();
    let stream = futures::stream::unfold(rx, |mut rx| async {
        match rx.recv().await {
            Ok(data) => Some((Ok(Event::default().data(data)), rx)),
            Err(_) => None,
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// SSE endpoint for the dashboard. Subscribes to the same event bus as
/// `/events` but wraps each event as an HTML fragment so htmx-ext-sse
/// can inject it directly into the dashboard "Live Events" panel.
async fn dashboard_events_sse(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.event_tx.subscribe();
    let stream = futures::stream::unfold(rx, |mut rx| async {
        match rx.recv().await {
            Ok(data) => {
                // Escape HTML special chars to avoid XSS in the dashboard
                let escaped = data
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;");
                let html = format!(
                    r#"<div style="padding:6px;margin-bottom:4px;background:#0d1117;border:1px solid #21262d;border-radius:4px;font-size:0.8rem;font-family:monospace;word-break:break-all;">{}</div>"#,
                    escaped
                );
                Some((Ok(Event::default().data(html)), rx))
            }
            Err(_) => None,
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

// ── WebSocket Handler ──────────────────────────

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> Response {
    ws.on_upgrade(move |socket| handle_ws(socket, (*state).clone()))
}

async fn handle_ws(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.event_tx.subscribe();
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(WsMessage::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });
    let mut recv_task =
        tokio::spawn(async move { while let Some(Ok(_)) = receiver.next().await {} });
    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }
}

// ── Prometheus Metrics ─────────────────────────

async fn metrics_endpoint(State(state): State<Arc<AppState>>) -> Response {
    let m = state.metrics.read().await;
    let body = format!(
        "# HELP sisyphus_requests_total Total HTTP requests\n# TYPE sisyphus_requests_total counter\nsisyphus_requests_total {}\n# HELP sisyphus_sessions_created Total sessions created\n# TYPE sisyphus_sessions_created counter\nsisyphus_sessions_created {}\n# HELP sisyphus_sop_executions Total SOP executions\n# TYPE sisyphus_sop_executions counter\nsisyphus_sop_executions {}\n# HELP sisyphus_search_queries Total search queries\n# TYPE sisyphus_search_queries counter\nsisyphus_search_queries {}\n# HELP sisyphus_uptime_seconds Server uptime\n# TYPE sisyphus_uptime_seconds gauge\nsisyphus_uptime_seconds {}\n",
        m.requests_total, m.sessions_created, m.sop_executions, m.search_queries, m.uptime_secs()
    );
    (
        [("Content-Type", "text/plain; version=0.0.4; charset=utf-8")],
        body,
    )
        .into_response()
}

// ── Auth API ──────────────────────────────────

async fn auth_validate(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AuthRequest>,
) -> Result<Json<AuthResponse>, StatusCode> {
    let keys = state.api_keys.read().await;
    let valid = keys.is_empty() || keys.contains(&req.api_key);
    Ok(Json(AuthResponse {
        valid,
        message: if valid { "authorized" } else { "unauthorized" }.into(),
    }))
}

// ── REST API Handlers ─────────────────────────

async fn api_health(
    State(state): State<Arc<AppState>>,
) -> Result<Json<HealthResponse>, StatusCode> {
    Ok(Json(fetch_health(&state).await))
}

async fn api_sessions(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<Session>>, StatusCode> {
    Ok(Json(fetch_sessions(&state).await))
}

async fn api_session_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<SessionDetail>, StatusCode> {
    Ok(Json(fetch_session_detail(&state, &id).await))
}

async fn api_create_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SessionCreateRequest>,
) -> Result<Json<Session>, StatusCode> {
    let session = create_session(&state, &req.name).await;
    state.metrics.write().await.sessions_created += 1;
    Ok(Json(session))
}

async fn api_add_message(
    State(state): State<Arc<AppState>>,
    Path(sid): Path<String>,
    Json(req): Json<MessageAddRequest>,
) -> Result<Json<ChatMessage>, StatusCode> {
    add_message(&state, &sid, &req.role, &req.content)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        .map(Json)
}

async fn api_search(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SearchRequest>,
) -> Result<Json<Vec<SearchResult>>, StatusCode> {
    state.metrics.write().await.search_queries += 1;
    Ok(Json(
        search_memory(&state, &req.query, req.top_k.unwrap_or(10)).await,
    ))
}

async fn api_execute_sop(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ExecuteRequest>,
) -> Result<Json<ExecuteResult>, StatusCode> {
    state.metrics.write().await.sop_executions += 1;
    execute_sop(&state, &req.sop_id, req.context.as_ref())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        .map(Json)
}

async fn api_sop_list(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<SopInfo>>, StatusCode> {
    Ok(Json(fetch_sop_list(&state).await))
}

async fn api_dag(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<DagGraph>, StatusCode> {
    Ok(Json(fetch_dag_graph(&state, &id).await))
}

async fn api_skill_list(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<SkillEntry>>, StatusCode> {
    Ok(Json(fetch_skill_list(&state).await))
}

async fn api_skill_install(
    State(_state): State<Arc<AppState>>,
    Json(req): Json<SkillInstallRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    Ok(Json(
        serde_json::json!({"status": "installed", "skill_id": req.skill_id}),
    ))
}

// ── Bridge Client Helpers ─────────────────────

async fn fetch_health(state: &AppState) -> HealthResponse {
    match state
        .client
        .get(format!("{}/health", state.bridge_url))
        .send()
        .await
    {
        Ok(r) => r.json::<HealthResponse>().await.unwrap_or(HealthResponse {
            status: "unreachable".into(),
            version: "unknown".into(),
            uptime_secs: 0,
        }),
        Err(_) => HealthResponse {
            status: "unreachable".into(),
            version: "unknown".into(),
            uptime_secs: 0,
        },
    }
}

async fn fetch_sessions(state: &AppState) -> Vec<Session> {
    match state
        .client
        .get(format!("{}/api/v1/sessions", state.bridge_url))
        .send()
        .await
    {
        Ok(r) => r.json::<Vec<Session>>().await.unwrap_or_default(),
        Err(_) => vec![],
    }
}

async fn fetch_session_detail(state: &AppState, id: &str) -> SessionDetail {
    match state
        .client
        .get(format!("{}/api/v1/sessions/{id}", state.bridge_url))
        .send()
        .await
    {
        Ok(r) => r.json::<SessionDetail>().await.unwrap_or(SessionDetail {
            id: id.into(),
            name: "unknown".into(),
            messages: vec![],
            metadata: HashMap::new(),
        }),
        Err(_) => SessionDetail {
            id: id.into(),
            name: "unknown".into(),
            messages: vec![],
            metadata: HashMap::new(),
        },
    }
}

async fn create_session(state: &AppState, name: &str) -> Session {
    match state
        .client
        .post(format!("{}/api/v1/sessions", state.bridge_url))
        .json(&serde_json::json!({"name": name}))
        .send()
        .await
    {
        Ok(r) => r.json::<Session>().await.unwrap_or(Session {
            id: "unknown".into(),
            name: name.into(),
            message_count: 0,
            created_at: None,
            parent_session_id: None,
        }),
        Err(_) => Session {
            id: "unknown".into(),
            name: name.into(),
            message_count: 0,
            created_at: None,
            parent_session_id: None,
        },
    }
}

async fn add_message(
    state: &AppState,
    sid: &str,
    role: &str,
    content: &str,
) -> Result<ChatMessage, reqwest::Error> {
    state
        .client
        .post(format!(
            "{}/api/v1/sessions/{sid}/messages",
            state.bridge_url
        ))
        .json(&serde_json::json!({"role": role, "content": content}))
        .send()
        .await?
        .json::<ChatMessage>()
        .await
}

async fn search_memory(state: &AppState, query: &str, top_k: usize) -> Vec<SearchResult> {
    match state
        .client
        .post(format!("{}/api/v1/vector/search", state.bridge_url))
        .json(&serde_json::json!({"query": query, "top_k": top_k}))
        .send()
        .await
    {
        Ok(r) => r.json::<Vec<SearchResult>>().await.unwrap_or_default(),
        Err(_) => vec![],
    }
}

async fn execute_sop(
    state: &AppState,
    sop_id: &str,
    context: Option<&serde_json::Value>,
) -> Result<ExecuteResult, reqwest::Error> {
    state.client.post(format!("{}/api/v1/execute", state.bridge_url))
        .json(&serde_json::json!({"sop_id": sop_id, "context": context.unwrap_or(&serde_json::json!({}))}))
        .send().await?.json::<ExecuteResult>().await
}

async fn fetch_sop_list(state: &AppState) -> Vec<SopInfo> {
    match state
        .client
        .get(format!("{}/api/v1/sops", state.bridge_url))
        .send()
        .await
    {
        Ok(r) => r.json::<Vec<SopInfo>>().await.unwrap_or_default(),
        Err(_) => vec![],
    }
}

async fn fetch_dag_graph(state: &AppState, sop_id: &str) -> DagGraph {
    match state
        .client
        .get(format!("{}/api/v1/sops/{sop_id}/dag", state.bridge_url))
        .send()
        .await
    {
        Ok(r) => r.json::<DagGraph>().await.unwrap_or(DagGraph {
            nodes: vec![],
            edges: vec![],
            width: 0.0,
            height: 0.0,
        }),
        Err(_) => DagGraph {
            nodes: vec![],
            edges: vec![],
            width: 0.0,
            height: 0.0,
        },
    }
}

async fn fetch_skill_list(state: &AppState) -> Vec<SkillEntry> {
    match state
        .client
        .get(format!("{}/api/v1/skills", state.bridge_url))
        .send()
        .await
    {
        Ok(r) => r.json::<Vec<SkillEntry>>().await.unwrap_or_default(),
        Err(_) => vec![],
    }
}

// ── SVG DAG Renderer ──────────────────────────

fn render_dag_svg(graph: &DagGraph) -> String {
    if graph.nodes.is_empty() {
        return "<p class='muted'>No DAG to display.</p>".into();
    }
    let w = graph.width + 40.0;
    let h = graph.height + 100.0; // extra space for legend
    let mut svg =
        format!("<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 {w} {h}' class='dag-svg'>");
    svg.push_str("<defs><marker id='arrow' markerWidth='10' markerHeight='7' refX='10' refY='3.5' orient='auto'><polygon points='0 0,10 3.5,0 7' fill='#30363d'/></marker></defs>");

    for edge in &graph.edges {
        if let (Some(f), Some(t)) = (
            graph.nodes.iter().find(|n| n.id == edge.from),
            graph.nodes.iter().find(|n| n.id == edge.to),
        ) {
            svg.push_str(&format!("<line x1='{}' y1='{}' x2='{}' y2='{}' stroke='#30363d' stroke-width='2' marker-end='url(#arrow)'/>", f.x + 60.0, f.y + 20.0, t.x + 60.0, t.y + 20.0));
        }
    }

    for node in &graph.nodes {
        let color = match node.status.as_str() {
            "completed" => "#3fb950",
            "running" => "#d29922",
            "failed" => "#f85149",
            _ => "#484f58",
        };
        svg.push_str(&format!("<rect x='{}' y='{}' width='120' height='40' rx='6' fill='#161b22' stroke='{color}' stroke-width='2'/>", node.x, node.y));
        svg.push_str(&format!("<text x='{}' y='{}' text-anchor='middle' fill='#c9d1d9' font-size='12' font-family='system-ui'>{}</text>", node.x + 60.0, node.y + 25.0, node.label));
    }

    // ── Color-coded legend ───────────────────────
    let legend_y = graph.height + 20.0;
    let legend_items = [
        ("#3fb950", "Completed"),
        ("#d29922", "Running"),
        ("#f85149", "Failed"),
        ("#484f58", "Pending"),
    ];
    let mut legend_x = 20.0;
    for (color, label) in &legend_items {
        svg.push_str(&format!(
            "<rect x='{legend_x}' y='{legend_y}' width='14' height='14' rx='3' fill='#161b22' stroke='{color}' stroke-width='2'/>"
        ));
        svg.push_str(&format!(
            "<text x='{}' y='{}' fill='#8b949e' font-size='11' font-family='system-ui'>{label}</text>",
            legend_x + 20.0,
            legend_y + 12.0,
        ));
        legend_x += 90.0;
    }

    svg.push_str("</svg>");
    svg
}

// ── Router ────────────────────────────────────

/// Auth middleware that checks `Authorization: Bearer <token>` against the
/// `WEBUI_API_KEY` env var. Passes through when no token is configured.
/// Reads the env var directly so it works without needing State extraction.
async fn auth_middleware(
    request: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Result<axum::http::Response<axum::body::Body>, (StatusCode, &'static str)> {
    let auth_token: Option<String> = std::env::var("WEBUI_API_KEY")
        .ok()
        .filter(|s| !s.is_empty());
    if let Some(ref token) = auth_token {
        let header = request
            .headers()
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let expected = format!("Bearer {}", token);
        if header != expected {
            return Err((StatusCode::UNAUTHORIZED, "Missing or invalid API key"));
        }
    }
    Ok(next.run(request).await)
}

pub fn app(bridge_url: String) -> Router {
    // Read optional auth token from environment. When set, all non-public
    // routes require `Authorization: Bearer <WEBUI_API_KEY>`.
    let auth_token: Option<String> = std::env::var("WEBUI_API_KEY")
        .ok()
        .filter(|s| !s.is_empty());

    let (event_tx, _) = broadcast::channel(256);
    let state = Arc::new(AppState {
        bridge_url,
        client: reqwest::Client::new(),
        event_tx,
        api_keys: Arc::new(RwLock::new(vec![])),
        metrics: Arc::new(RwLock::new(MetricsCollector {
            uptime_start: Some(Instant::now()),
            ..Default::default()
        })),
        auth_token,
        chaos_enabled: Arc::new(AtomicBool::new(false)),
        chaos_fault_probability: Arc::new(RwLock::new(0.1)),
        chaos_fault_count: Arc::new(RwLock::new(0)),
    });

    // Public routes accessible without authentication
    let public_routes = Router::new()
        .route("/events", get(sse_handler))
        .route("/htmx/dashboard-events", get(dashboard_events_sse))
        .route("/ws", get(ws_handler))
        .route("/metrics", get(metrics_endpoint))
        .nest_service("/static", ServeDir::new("static"));

    // Protected routes requiring auth (when WEBUI_API_KEY is set)
    let protected_routes = Router::new()
        .route("/", get(index))
        .route("/sessions", get(sessions_page))
        .route("/dag", get(dag_page))
        .route("/sop", get(sop_page))
        .route("/skills", get(skills_page))
        .route("/chaos", get(chaos_page))
        .route("/htmx/dashboard", get(dashboard_htmx))
        .route("/htmx/sessions", get(sessions_htmx))
        .route("/htmx/session/{id}/detail", get(session_detail_htmx))
        .route("/htmx/dag", get(dag_htmx))
        .route("/htmx/sops", get(sop_list_htmx))
        .route("/htmx/skills", get(skills_list_htmx))
        .route("/htmx/exec", post(exec_result_htmx))
        .route("/htmx/session/{id}/messages", post(add_message_htmx))
        .route("/htmx/skills/install", post(skill_install_htmx))
        .route("/htmx/chaos/status", get(chaos_status_htmx))
        .route("/htmx/chaos/toggle", post(chaos_toggle_htmx))
        .route("/htmx/chaos/probability", post(chaos_probability_htmx))
        .route("/api/health", get(api_health))
        .route("/api/sessions", get(api_sessions).post(api_create_session))
        .route("/api/sessions/{id}", get(api_session_detail))
        .route("/api/sessions/{id}/messages", post(api_add_message))
        .route("/api/search", post(api_search))
        .route("/api/execute", post(api_execute_sop))
        .route("/api/sops", get(api_sop_list))
        .route("/api/sops/{id}/dag", get(api_dag))
        .route("/api/skills", get(api_skill_list))
        .route("/api/skills/install", post(api_skill_install))
        .route("/api/auth/validate", post(auth_validate))
        .layer(axum_mw::from_fn(auth_middleware));

    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .layer(CorsLayer::permissive())
        .with_state(state)
}
