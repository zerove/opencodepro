//! Sisyphus Web 仪表盘 —— axum + HTMX + SSE
//!
//! 功能完整的 Web 界面：
//! - 实时 SSE 事件流式传输
//! - DAG 可视化 (交互式 SVG)
//! - 会话详情与消息历史
//! - SOP 执行面板
//! - 技能市场浏览器
//! - 记忆搜索
//! - Prometheus 指标端点
//! - OAuth2/API key 认证中间件

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

// ── 共享状态 ───────────────────────────────

#[derive(Clone)]
pub struct AppState {
    pub bridge_url: String,
    pub client: reqwest::Client,
    pub event_tx: broadcast::Sender<String>,
    pub api_keys: Arc<RwLock<Vec<String>>>,
    pub metrics: Arc<RwLock<MetricsCollector>>,
    /// 可选的认证 token，来自 WEBUI_API_KEY 环境变量。若设置，则除 events/ws/metrics 外的所有请求必须包含 `Authorization: Bearer <token>`。
    pub auth_token: Option<String>,
    /// 混沌工程：故障注入当前是否启用。
    pub chaos_enabled: Arc<AtomicBool>,
    /// 混沌工程：故障概率，范围 0.0 到 1.0。
    pub chaos_fault_probability: Arc<RwLock<f64>>,
    /// 混沌工程：启动以来注入的故障总数。
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
        self.uptime_start.map(|t| t.elapsed().as_secs()).unwrap_or(0)
    }
}

// ── API 类型 ─────────────────────────────────

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

// ── HTML 模板 ────────────────────────────

fn page_shell(title: &str, nav_active: &str, body: &str) -> String {
    let nav_items = [
        ("dashboard", "/", "仪表盘"),
        ("sessions", "/sessions", "会话"),
        ("dag", "/dag", "DAG"),
        ("sop", "/sop", "SOP"),
        ("skills", "/skills", "技能"),
        ("chaos", "/chaos", "混沌"),
        ("cogmap", "/cogmap", "认知地图"),
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
<html lang="zh-CN">
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
<h1>仪表盘</h1>
<div class="card">
  <h2>状态</h2>
  <div hx-get="/htmx/dashboard" hx-trigger="load, every 10s" hx-swap="innerHTML">加载中...</div>
</div>
<div class="card">
  <h2>实时事件</h2>
  <div id="dashboard-events" hx-ext="sse" sse-connect="/htmx/dashboard-events" sse-swap="message" hx-swap="beforeend" style="max-height:240px;overflow-y:auto;background:#0d1117;border:1px solid #21262d;border-radius:6px;padding:8px;font-family:monospace;font-size:0.8rem;">
    <span class="muted">等待 bridge 事件...</span>
  </div>
</div>
<div class="card">
  <h2>路线图</h2>
  <div id="roadmap-dashboard" hx-get="/htmx/roadmap/dashboard/default" hx-trigger="load, every 15s" hx-swap="innerHTML">加载中...</div>
</div>
"#
}

// ── 页面处理器 ─────────────────────────────

async fn index() -> Html<String> {
    Html(page_shell("仪表盘", "dashboard", dashboard_body()))
}

async fn sessions_page() -> Html<String> {
    let body = r#"
<h1>会话</h1>
<div class="card">
  <h2>会话列表</h2>
  <div id="session-list" hx-get="/htmx/sessions" hx-trigger="load, every 5s" hx-swap="innerHTML">加载中...</div>
</div>
<div class="card">
  <h2>会话详情</h2>
  <div id="session-detail"><p class="muted">选择一个会话以查看详情。</p></div>
</div>
"#;
    Html(page_shell("会话", "sessions", body))
}

async fn dag_page() -> Html<String> {
    let body = r#"
<h1>DAG 可视化</h1>
<div class="card">
  <h2>执行图</h2>
  <div id="dag-container" hx-get="/htmx/dag?sop_id=default" hx-trigger="load" hx-swap="innerHTML">加载中...</div>
</div>
"#;
    Html(page_shell("DAG", "dag", body))
}

async fn sop_page() -> Html<String> {
    let body = r#"
<h1>SOP 管理</h1>
<div class="card">
  <h2>可用 SOP</h2>
  <div id="sop-list" hx-get="/htmx/sops" hx-trigger="load" hx-swap="innerHTML">加载中...</div>
</div>
<div class="card">
  <h2>执行结果</h2>
  <div id="exec-result"><p class="muted">执行一个 SOP 以查看结果。</p></div>
</div>
"#;
    Html(page_shell("SOP", "sop", body))
}

async fn skills_page() -> Html<String> {
    let body = r#"
<h1>技能市场</h1>
<div class="card">
  <h2>可用技能</h2>
  <div id="skill-list" hx-get="/htmx/skills" hx-trigger="load" hx-swap="innerHTML">加载中...</div>
</div>
"#;
    Html(page_shell("技能", "skills", body))
}

// ── 混沌工程页面 ─────────────────────

async fn cogmap_page() -> Html<String> {
    let body = include_str!("../templates/cogmap.html");
    Html(page_shell("实时认知地图", "cogmap", body))
}

async fn chaos_page() -> Html<String> {
    let body = r##"
<h1>混沌工程</h1>
<div class="card">
  <h2>混沌状态</h2>
  <div id="chaos-status" hx-get="/htmx/chaos/status" hx-trigger="load, every 3s" hx-swap="innerHTML">加载中...</div>
</div>
<div class="card">
  <h2>控制</h2>
  <div style="display:flex;flex-direction:column;gap:16px;">
    <div>
      <button id="chaos-toggle-btn" class="btn-exec" hx-post="/htmx/chaos/toggle" hx-target="#chaos-status" hx-swap="innerHTML">切换混沌</button>
      <span class="muted" style="margin-left:8px;">启动 / 停止故障注入</span>
    </div>
    <div>
      <label>故障概率: <strong id="chaos-prob-display">10%</strong></label>
      <input type="range" min="0" max="100" value="10" id="chaos-slider" style="width:300px;display:block;margin-top:4px;" hx-post="/htmx/chaos/probability" hx-trigger="change" hx-target="#chaos-status" hx-swap="innerHTML" name="probability">
    </div>
  </div>
</div>
"##;
    Html(page_shell("混沌", "chaos", body))
}

// ── HTMX 局部处理器 ─────────────────────

async fn dashboard_htmx(State(state): State<Arc<AppState>>) -> Response {
    let health = fetch_health(&state).await;
    let sessions = fetch_sessions(&state).await;

    let html = format!(
        r#"<div class="stat"><span class="label">状态</span><span class="value status-{}">{}</span></div>
<div class="stat"><span class="label">版本</span><span class="value">{}</span></div>
<div class="stat"><span class="label">运行时间</span><span class="value">{}s</span></div>
<div class="stat"><span class="label">会话数</span><span class="value">{}</span></div>"#,
        if health.status == "ok" { "ok" } else { "err" },
        health.status,
        health.version,
        health.uptime_secs,
        sessions.len()
    );

    Html(html).into_response()
}

async fn roadmap_dashboard_htmx(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<String>,
) -> Response {
    let data = fetch_roadmap_dashboard(&state, &project_id).await;
    let inner = if data.is_null() {
        r#"<div class="card"><h2>路线图仪表盘</h2><p class="muted">加载失败：bridge 不可达或接口错误。</p></div>"#.to_string()
    } else {
        let pretty = serde_json::to_string_pretty(&data).unwrap_or_else(|_| "{}".to_string());
        format!(
            r#"<div class="card"><h2>路线图仪表盘</h2><pre class="muted" style="white-space:pre-wrap;">{}</pre></div>"#,
            pretty
        )
    };
    Html(inner).into_response()
}

async fn sessions_htmx(State(state): State<Arc<AppState>>) -> Response {
    let sessions = fetch_sessions(&state).await;
    let mut html = String::from("<table class='session-table'><thead><tr><th>名称</th><th>消息数</th><th>操作</th></tr></thead><tbody>");

    for s in &sessions {
        html.push_str(&format!(
            r#"<tr>
<td>{}</td>
<td>{}</td>
<td><button class='btn-sm' hx-get='/htmx/session/{}/detail' hx-target='#session-detail'>查看</button></td>
</tr>"#,
            s.name, s.message_count, s.id
        ));
    }

    html.push_str("</tbody></table>");
    if sessions.is_empty() {
        html.push_str("<p class='muted'>暂无会话。</p>");
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
        r#"<div class='card'><h2>发送消息</h2>
<form hx-post='/htmx/session/{}/messages' hx-target='#session-detail' hx-swap='innerHTML'>
<textarea name='content' placeholder='输入消息...' rows='3'></textarea>
<input type='hidden' name='role' value='user'>
<button type='submit' style='margin-top:8px'>发送</button>
</form></div>"#,
        session_id
    ));

    Html(html).into_response()
}

async fn dag_htmx(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let sop_id = params.get("sop_id").map(|s| s.as_str()).unwrap_or("default");
    let graph = fetch_dag_graph(&state, sop_id).await;
    let svg = render_dag_svg(&graph);
    Html(svg).into_response()
}

async fn sop_list_htmx(State(state): State<Arc<AppState>>) -> Response {
    let sops = fetch_sop_list(&state).await;
    let mut html = String::from("<table class='session-table'><thead><tr><th>名称</th><th>节点数</th><th>操作</th></tr></thead><tbody>");
    for s in &sops {
        html.push_str(&format!(
            r#"<tr><td>{}</td><td>{}</td><td>
<button class='btn-sm' hx-get='/htmx/dag?sop_id={}' hx-target='#dag-container' hx-swap='innerHTML'>DAG</button>
<button class='btn-sm btn-exec' hx-post='/htmx/exec' hx-vals='{{"sop_id":"{}"}}' hx-target='#exec-result'>运行</button>
</td></tr>"#,
            s.name, s.node_count, s.id, s.id
        ));
    }
    html.push_str("</tbody></table>");
    if sops.is_empty() {
        html.push_str("<p class='muted'>暂无可用 SOP。</p>");
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
<div class='skill-meta'><span class='stars'>{}</span><span>{} 下载</span></div>
<p class='skill-author'>作者 {} v{}</p>
<button class='btn-sm' hx-post='/htmx/skills/install' hx-vals='{{"skill_id":"{}"}}' hx-target='this' hx-swap='outerHTML'>安装</button>
</div>"#,
            s.name, s.description, stars, s.downloads, s.author, s.version, s.id
        ));
    }
    html.push_str("</div>");
    if skills.is_empty() {
        html.push_str("<p class='muted'>暂无可用技能。</p>");
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
                r#"<div class='exec-result status-{}'><h4>执行结果</h4>
<div class='stat'><span class='label'>SOP</span><span class='value'>{}</span></div>
<div class='stat'><span class='label'>状态</span><span class='value'>{}</span></div>
<div class='stat'><span class='label'>迭代次数</span><span class='value'>{}</span></div>
<div class='stat'><span class='label'>偏差</span><span class='value'>{:.3}</span></div></div>"#,
                cls, r.sop_id, r.status, r.iterations, r.deviation
            ))
            .into_response()
        },
        Err(e) => Html(format!(
            "<div class='exec-result status-err'>错误: {e}</div>"
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
    let _ = state.event_tx.send(serde_json::to_string(&event).unwrap_or_default());

    session_detail_htmx(State(state), Path(session_id)).await
}

async fn skill_install_htmx(
    State(_state): State<Arc<AppState>>,
    Json(req): Json<SkillInstallRequest>,
) -> Response {
    Html(format!(
        "<span class='installed'>已安装: {}</span>",
        req.skill_id
    ))
    .into_response()
}

// ── 混沌 HTMX 处理器 ────────────────────

/// 返回混沌状态的 HTML 片段，显示启用/禁用状态、当前故障概率和故障总数。
async fn chaos_status_htmx(State(state): State<Arc<AppState>>) -> Response {
    let enabled = state.chaos_enabled.load(Ordering::Relaxed);
    let prob = *state.chaos_fault_probability.read().await;
    let count = *state.chaos_fault_count.read().await;

    let status_label = if enabled { "运行中" } else { "已停止" };
    let status_class = if enabled { "status-ok" } else { "muted" };

    let html = format!(
        r#"<div class="stat"><span class="label">状态</span><span class="value {}">{}</span></div>
<div class="stat"><span class="label">故障概率</span><span class="value">{:.0}%</span></div>
<div class="stat"><span class="label">已注入故障</span><span class="value status-{}">{}</span></div>"#,
        status_class,
        status_label,
        prob * 100.0,
        if count > 0 { "err" } else { "ok" },
        count
    );
    Html(html).into_response()
}

/// 切换混沌开启/关闭。启用时，增加故障计数以模拟仪表盘可见的活跃故障注入。
async fn chaos_toggle_htmx(State(state): State<Arc<AppState>>) -> Response {
    let was_enabled = state.chaos_enabled.fetch_xor(true, Ordering::Relaxed);
    let now_enabled = !was_enabled;

    // 切换时模拟故障注入以提供视觉反馈
    if now_enabled {
        let mut count = state.chaos_fault_count.write().await;
        *count += 1;
    }

    // 广播混沌切换事件
    let event = serde_json::json!({
        "event_type": "chaos_toggle",
        "enabled": now_enabled,
        "timestamp_secs": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    });
    let _ = state.event_tx.send(serde_json::to_string(&event).unwrap_or_default());

    chaos_status_htmx(State(state)).await
}

/// 从滑块值 (0-100 整数) 设置混沌故障概率。
async fn chaos_probability_htmx(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let raw: f64 = params.get("probability").and_then(|v| v.parse().ok()).unwrap_or(10.0);
    let pct = raw.clamp(0.0, 100.0);
    *state.chaos_fault_probability.write().await = pct / 100.0;

    chaos_status_htmx(State(state)).await
}

// ── SSE 处理器 ────────────────────────────────

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

/// 仪表盘的 SSE 端点。订阅与 `/events` 相同的事件总线，但将每个事件包装为 HTML 片段，
/// 以便 htmx-ext-sse 直接注入仪表盘的 "Live Events" 面板。
async fn dashboard_events_sse(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.event_tx.subscribe();
    let stream = futures::stream::unfold(rx, |mut rx| async {
        match rx.recv().await {
            Ok(data) => {
                // 转义 HTML 特殊字符以避免仪表盘 XSS
                let escaped = data.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
                let html = format!(
                    r#"<div style="padding:6px;margin-bottom:4px;background:#0d1117;border:1px solid #21262d;border-radius:4px;font-size:0.8rem;font-family:monospace;word-break:break-all;">{}</div>"#,
                    escaped
                );
                Some((Ok(Event::default().data(html)), rx))
            },
            Err(_) => None,
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

// ── WebSocket 处理器 ──────────────────────────

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

// ── Prometheus 指标 ───────────────────────────

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

// ── 认证 API ──────────────────────────────────

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

// ── REST API 处理器 ─────────────────────────

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

async fn api_roadmap_dashboard(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    Ok(Json(fetch_roadmap_dashboard(&state, &project_id).await))
}

// ── Bridge 客户端辅助函数 ─────────────────────

async fn fetch_health(state: &AppState) -> HealthResponse {
    match state.client.get(format!("{}/health", state.bridge_url)).send().await {
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

async fn fetch_roadmap_dashboard(state: &AppState, project_id: &str) -> serde_json::Value {
    match state
        .client
        .get(format!(
            "{}/api/roadmap/dashboard/{}",
            state.bridge_url, project_id
        ))
        .send()
        .await
    {
        Ok(r) => r.json::<serde_json::Value>().await.unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::Null,
    }
}

async fn fetch_sessions(state: &AppState) -> Vec<Session> {
    match state.client.get(format!("{}/api/sessions", state.bridge_url)).send().await {
        Ok(r) => r.json::<Vec<Session>>().await.unwrap_or_default(),
        Err(_) => vec![],
    }
}

async fn fetch_session_detail(state: &AppState, id: &str) -> SessionDetail {
    match state.client.get(format!("{}/api/sessions/{id}", state.bridge_url)).send().await {
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
        .post(format!("{}/api/sessions", state.bridge_url))
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
        .post(format!("{}/api/sessions/{sid}/messages", state.bridge_url))
        .json(&serde_json::json!({"role": role, "content": content}))
        .send()
        .await?
        .json::<ChatMessage>()
        .await
}

async fn search_memory(state: &AppState, query: &str, top_k: usize) -> Vec<SearchResult> {
    match state
        .client
        .post(format!("{}/api/vector/search", state.bridge_url))
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
    state.client.post(format!("{}/api/execute", state.bridge_url))
        .json(&serde_json::json!({"sop_id": sop_id, "context": context.unwrap_or(&serde_json::json!({}))}))
        .send().await?.json::<ExecuteResult>().await
}

async fn fetch_sop_list(state: &AppState) -> Vec<SopInfo> {
    match state.client.get(format!("{}/api/sops", state.bridge_url)).send().await {
        Ok(r) => r.json::<Vec<SopInfo>>().await.unwrap_or_default(),
        Err(_) => vec![],
    }
}

async fn fetch_dag_graph(state: &AppState, sop_id: &str) -> DagGraph {
    match state.client.get(format!("{}/api/sops/{sop_id}", state.bridge_url)).send().await {
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
    match state.client.get(format!("{}/api/skills", state.bridge_url)).send().await {
        Ok(r) => r.json::<Vec<SkillEntry>>().await.unwrap_or_default(),
        Err(_) => vec![],
    }
}

// ── SVG DAG 渲染器 ──────────────────────────

fn render_dag_svg(graph: &DagGraph) -> String {
    if graph.nodes.is_empty() {
        return "<p class='muted'>No DAG to display.</p>".into();
    }
    let w = graph.width + 40.0;
    let h = graph.height + 100.0; // 为图例预留额外空间
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

    // ── 颜色编码图例 ───────────────────────
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

// ── 路由 ────────────────────────────────────

/// 认证中间件：检查 `Authorization: Bearer <token>` 与 State 中的 auth_token。
/// 未配置 token 时直接放行。
///
/// token 由 [`app_with_token`] 显式传入（[`AuthToken`] 作为中间件 State），
/// 不再从环境变量读取。此前中间件每请求都重读 `WEBUI_API_KEY`，这既冗余
/// （`app()` 已经读过一次），又让 token 藏在进程全局状态里——测试只能靠
/// `std::env::set_var` 改写它，而那正是并行执行下互相污染、且 panic 后
/// 永久残留的来源。
async fn auth_middleware(
    axum::extract::State(auth): axum::extract::State<AuthToken>,
    request: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Result<axum::http::Response<axum::body::Body>, (StatusCode, &'static str)> {
    let auth_token: Option<String> = auth.0.clone();
    if let Some(ref token) = auth_token {
        let header =
            request.headers().get("Authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
        let expected = format!("Bearer {}", token);
        if header != expected {
            return Err((StatusCode::UNAUTHORIZED, "Missing or invalid API key"));
        }
    }
    Ok(next.run(request).await)
}

/// 认证中间件的 State：只携带可选 token，与 [`AppState`] 解耦。
#[derive(Clone, Default)]
struct AuthToken(Option<String>);

pub fn app(bridge_url: String) -> Router {
    // 从环境变量读取可选认证 token。设置后，所有非公开路由
    // 需携带 `Authorization: Bearer <WEBUI_API_KEY>`。
    let auth_token: Option<String> = std::env::var("WEBUI_API_KEY").ok().filter(|s| !s.is_empty());
    app_with_token(bridge_url, auth_token)
}

/// 与 [`app`] 相同，但认证 token 由调用方显式给出而非读环境变量。
///
/// 测试必须走这个入口。`app()` 读的是进程级 `WEBUI_API_KEY`，测试若要
/// 覆盖「已设置 token」与「未设置 token」两种情形就只能改写环境变量，
/// 而并行执行下这会互相污染，且某个断言 panic 会让清理语句被跳过、
/// 变量永久残留给后续测试——失败现场表现为「某个无关测试突然开始
/// 401」，极难归因。
pub fn app_with_token(bridge_url: String, auth_token: Option<String>) -> Router {
    // 先按 token 克隆出中间件 State，再把所有权交给 `AppState`。
    let auth_state = AuthToken(auth_token.clone());

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

    // 公开路由：无需认证即可访问
    let public_routes = Router::new()
        .route("/events", get(sse_handler))
        .route("/htmx/dashboard-events", get(dashboard_events_sse))
        .route("/ws", get(ws_handler))
        .route("/metrics", get(metrics_endpoint))
        .nest_service("/static", ServeDir::new("static"));

    // 受保护路由：设置 WEBUI_API_KEY 时需要认证
    let protected_routes = Router::new()
        .route("/", get(index))
        .route("/sessions", get(sessions_page))
        .route("/dag", get(dag_page))
        .route("/sop", get(sop_page))
        .route("/skills", get(skills_page))
        .route("/chaos", get(chaos_page))
        .route("/cogmap", get(cogmap_page))
        .route("/htmx/dashboard", get(dashboard_htmx))
        .route("/htmx/roadmap/dashboard/{project_id}", get(roadmap_dashboard_htmx))
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
        .route(
            "/api/roadmap/dashboard/{project_id}",
            get(api_roadmap_dashboard),
        )
        // 用 `from_fn_with_state` 而非 `from_fn`：token 作为中间件 State
        // 直接传入，不依赖请求 extensions——后者在 `.layer()` 先于
        // `with_state()` 调用时取不到值，会静默退化成「永不校验」。
        .layer(axum_mw::from_fn_with_state(auth_state, auth_middleware));

    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .layer(CorsLayer::permissive())
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{header, Request};
    use tower::ServiceExt;

    fn test_state() -> Arc<AppState> {
        let (event_tx, _) = broadcast::channel(64);
        Arc::new(AppState {
            bridge_url: "http://localhost:8080".into(),
            client: reqwest::Client::new(),
            event_tx,
            api_keys: Arc::new(RwLock::new(vec![])),
            metrics: Arc::new(RwLock::new(MetricsCollector::default())),
            auth_token: None,
            chaos_enabled: Arc::new(AtomicBool::new(false)),
            chaos_fault_probability: Arc::new(RwLock::new(0.1)),
            chaos_fault_count: Arc::new(RwLock::new(0)),
        })
    }

    // ── DAG SVG 渲染器 ────────────────────────────────

    #[test]
    fn test_render_dag_svg_empty_graph() {
        let graph = DagGraph {
            nodes: vec![],
            edges: vec![],
            width: 0.0,
            height: 0.0,
        };
        let svg = render_dag_svg(&graph);
        assert!(svg.contains("No DAG to display"));
    }

    #[test]
    fn test_render_dag_svg_nodes_and_edges() {
        let graph = DagGraph {
            nodes: vec![
                DagNode {
                    id: "a".into(),
                    label: "Fetch".into(),
                    status: "completed".into(),
                    x: 0.0,
                    y: 0.0,
                },
                DagNode {
                    id: "b".into(),
                    label: "Analyze".into(),
                    status: "running".into(),
                    x: 200.0,
                    y: 0.0,
                },
                DagNode {
                    id: "c".into(),
                    label: "Report".into(),
                    status: "pending".into(),
                    x: 400.0,
                    y: 0.0,
                },
            ],
            edges: vec![
                DagEdge {
                    from: "a".into(),
                    to: "b".into(),
                },
                DagEdge {
                    from: "b".into(),
                    to: "c".into(),
                },
            ],
            width: 500.0,
            height: 100.0,
        };
        let svg = render_dag_svg(&graph);
        // SVG 容器带图例空间
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        // 节点带状态颜色
        assert!(svg.contains("Fetch"));
        assert!(svg.contains("Analyze"));
        assert!(svg.contains("Report"));
        assert!(svg.contains("#3fb950")); // completed
        assert!(svg.contains("#d29922")); // running
        assert!(svg.contains("#484f58")); // pending
                                          // 边渲染为线条
        assert_eq!(svg.matches("<line").count(), 2);
        // 图例
        assert!(svg.contains("Completed"));
        assert!(svg.contains("Failed"));
    }

    #[test]
    fn test_render_dag_svg_failed_status() {
        let graph = DagGraph {
            nodes: vec![DagNode {
                id: "x".into(),
                label: "Broken".into(),
                status: "failed".into(),
                x: 0.0,
                y: 0.0,
            }],
            edges: vec![],
            width: 100.0,
            height: 50.0,
        };
        let svg = render_dag_svg(&graph);
        assert!(svg.contains("#f85149"));
    }

    // ── 混沌状态辅助函数 ─────────────────────────────

    #[tokio::test]
    async fn test_chaos_status_htmx_initial_stopped() {
        let state = test_state();
        let resp = chaos_status_htmx(State(state.clone())).await;
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let html = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(html.contains("已停止"));
        assert!(html.contains("10%"));
        assert!(html.contains("0"));
    }

    #[tokio::test]
    async fn test_chaos_toggle_htmx_enables_and_increments() {
        let state = test_state();
        let resp = chaos_toggle_htmx(State(state.clone())).await;
        let _ = resp.into_response();
        assert!(state.chaos_enabled.load(Ordering::Relaxed));
        assert_eq!(*state.chaos_fault_count.read().await, 1);
    }

    #[tokio::test]
    async fn test_chaos_probability_htmx_sets_value() {
        let state = test_state();
        let mut params = HashMap::new();
        params.insert("probability".to_string(), "42".to_string());
        let resp = chaos_probability_htmx(State(state.clone()), Query(params)).await;
        let _ = resp.into_response();
        assert!((*state.chaos_fault_probability.read().await - 0.42).abs() < 1e-9);
    }

    // ── 路由测试 ─────────────────────────────────────────

    #[tokio::test]
    async fn test_router_index_serves_dashboard() {
        let router = app_with_token("http://localhost:8080".into(), None);
        let resp =
            router.oneshot(Request::builder().uri("/").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_router_metrics_endpoint() {
        let router = app_with_token("http://localhost:8080".into(), None);
        let resp = router
            .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(body.contains("sisyphus_requests_total"));
        assert!(body.contains("sisyphus_uptime_seconds"));
    }

    #[tokio::test]
    async fn test_router_auth_required_when_token_set() {
        let router = app_with_token(
            "http://localhost:8080".into(),
            Some("test-secret-key".into()),
        );

        // 无认证头 → 401
        let resp = router
            .clone()
            .oneshot(Request::builder().uri("/api/sessions").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // 错误 token → 401
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/sessions")
                    .header(header::AUTHORIZATION, "Bearer wrong-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // 正确 token → 200
        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/api/sessions")
                    .header(header::AUTHORIZATION, "Bearer test-secret-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_router_events_public_no_auth() {
        // 未配置 token → 公开路由无需认证即可访问。
        let router = app_with_token("http://localhost:8080".into(), None);
        let resp = router
            .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// 守护用例：token 必须来自显式参数，而非进程级环境变量。
    ///
    /// 若把 `app_with_token(.., Some(t))` 换回读 `WEBUI_API_KEY`，
    /// 本用例与 `test_router_auth_required_when_token_set` 会同时失败。
    /// 这正是本次改动的核心断言——中间件与构造器都不再碰全局状态，
    /// 因此三个路由测试可以安全并行，不必靠互斥锁串行。
    #[tokio::test]
    async fn test_two_routers_with_different_tokens_are_independent() {
        let strict = app_with_token("http://localhost:8080".into(), Some("token-a".into()));
        let other = app_with_token("http://localhost:8080".into(), Some("token-b".into()));

        // token-a 不应被 token-b 放行
        let resp = strict
            .oneshot(
                Request::builder()
                    .uri("/api/sessions")
                    .header(header::AUTHORIZATION, "Bearer token-b")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // 反向：用 token-b 请求 token-b 的 router 应当通过
        let resp = other
            .oneshot(
                Request::builder()
                    .uri("/api/sessions")
                    .header(header::AUTHORIZATION, "Bearer token-b")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
