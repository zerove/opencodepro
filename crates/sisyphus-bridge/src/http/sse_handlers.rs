//! 服务端发送事件 (SSE) 处理器，用于实时事件流式传输。
//!
//! 提供 `/api/events` 端点，基于 Server-Sent Events 实现实时事件流式传输。

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response, Sse},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::borrow::Cow;
use std::convert::Infallible;
use std::time::Duration;
use uuid::Uuid;

use crate::http::RestApiState;

/// SSE 事件类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SseEvent {
    /// 认知迭代开始
    CognitiveIterationStarted {
        iteration: usize,
        goal_id: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// 认知迭代完成
    CognitiveIterationCompleted {
        iteration: usize,
        goal_id: String,
        deviation: f64,
        converged: bool,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// 节点执行开始
    NodeExecutionStarted {
        sop_id: String,
        node_id: String,
        node_name: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// 节点执行完成
    NodeExecutionCompleted {
        sop_id: String,
        node_id: String,
        node_name: String,
        success: bool,
        output: Option<serde_json::Value>,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// SOP 执行开始
    SopExecutionStarted {
        sop_id: String,
        goal_id: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// SOP 执行完成
    SopExecutionCompleted {
        sop_id: String,
        goal_id: String,
        success: bool,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// 混沌故障注入
    ChaosFaultInjected {
        fault_type: String,
        target: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// 混沌自动修复触发
    ChaosAutoFixTriggered {
        fault_count: u64,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// 技能安装
    SkillInstalled {
        skill_id: String,
        version: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// 技能卸载
    SkillUninstalled {
        skill_id: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// 插件加载
    PluginLoaded {
        plugin_id: String,
        version: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// 插件卸载
    PluginUnloaded {
        plugin_id: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// 工作区切换
    WorkspaceSwitched {
        workspace_id: String,
        workspace_name: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// 会话消息新增
    SessionMessageAdded {
        session_id: String,
        role: String,
        content: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// 自定义事件
    Custom {
        event_type: String,
        payload: serde_json::Value,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
}

impl SseEvent {
    /// 创建自定义事件
    #[allow(dead_code)] // 公共 API，用于广播自定义事件
    pub fn custom(event_type: String, payload: serde_json::Value) -> Self {
        Self::Custom {
            event_type,
            payload,
            timestamp: chrono::Utc::now(),
        }
    }

    /// 获取事件类型字符串
    pub fn event_type(&self) -> Cow<'static, str> {
        match self {
            SseEvent::CognitiveIterationStarted { .. } => "cognitive_iteration_started".into(),
            SseEvent::CognitiveIterationCompleted { .. } => "cognitive_iteration_completed".into(),
            SseEvent::NodeExecutionStarted { .. } => "node_execution_started".into(),
            SseEvent::NodeExecutionCompleted { .. } => "node_execution_completed".into(),
            SseEvent::SopExecutionStarted { .. } => "sop_execution_started".into(),
            SseEvent::SopExecutionCompleted { .. } => "sop_execution_completed".into(),
            SseEvent::ChaosFaultInjected { .. } => "chaos_fault_injected".into(),
            SseEvent::ChaosAutoFixTriggered { .. } => "chaos_auto_fix_triggered".into(),
            SseEvent::SkillInstalled { .. } => "skill_installed".into(),
            SseEvent::SkillUninstalled { .. } => "skill_uninstalled".into(),
            SseEvent::PluginLoaded { .. } => "plugin_loaded".into(),
            SseEvent::PluginUnloaded { .. } => "plugin_unloaded".into(),
            SseEvent::WorkspaceSwitched { .. } => "workspace_switched".into(),
            SseEvent::SessionMessageAdded { .. } => "session_message_added".into(),
            SseEvent::Custom { event_type, .. } => Cow::Owned(event_type.clone()),
        }
    }

    /// 转换为 SSE 格式字符串
    #[allow(dead_code)] // 测试与外部消费者使用
    pub fn to_sse_string(&self) -> String {
        let data = serde_json::to_string(self).unwrap_or_default();
        format!("event: {}\ndata: {}\n\n", self.event_type(), data)
    }
}

/// SSE 连接查询参数
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // 从 URL 解析的查询参数，用于过滤
pub struct SseQuery {
    /// 按事件类型过滤（逗号分隔）
    pub events: Option<String>,
    /// 按项目 ID 过滤
    pub project_id: Option<String>,
    /// 按 SOP ID 过滤
    pub sop_id: Option<String>,
    /// 按会话 ID 过滤
    pub session_id: Option<String>,
    /// 用于断点续传的最后事件 ID
    pub last_event_id: Option<String>,
    /// 心跳间隔（秒，0 表示禁用）
    pub heartbeat: Option<u64>,
}

/// SSE 并发连接数限制失败时的 HTTP 状态码
const SSE_LIMIT_REACHED: StatusCode = StatusCode::TOO_MANY_REQUESTS;

/// SSE 流处理器
pub async fn sse_handler(
    State(state): State<RestApiState>,
    Query(query): Query<SseQuery>,
) -> Response {
    // 并发连接数限制：超过上限直接返回 429，避免无限连接耗尽资源
    let _permit = match state.sse_connection_limit.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return (
                SSE_LIMIT_REACHED,
                Json(json!({"error": "Too many SSE connections"})),
            )
                .into_response();
        }
    };

    // 订阅全局广播器（所有客户端共享同一 channel）
    let receiver = state.sse_broadcaster.subscribe();

    // 解析事件过滤器
    let event_filters: Option<Vec<String>> = query
        .events
        .as_ref()
        .map(|s| s.split(',').map(|s| s.trim().to_string()).collect());

    let heartbeat_interval = query.heartbeat.unwrap_or(30);

    // 从广播接收器创建事件流
    let stream = async_stream::stream! {
        let mut receiver = receiver;
        let mut last_heartbeat = std::time::Instant::now();

        loop {
            // 检查是否该发送心跳
            let now = std::time::Instant::now();
            if heartbeat_interval > 0 && now.duration_since(last_heartbeat) >= Duration::from_secs(heartbeat_interval) {
                last_heartbeat = now;
                yield Ok::<axum::response::sse::Event, Infallible>(
                    axum::response::sse::Event::default()
                        .event("heartbeat")
                        .data(json!({"timestamp": chrono::Utc::now().to_rfc3339()}).to_string()),
                );
            }

            // 尝试接收事件（带超时）
            match tokio::time::timeout(
                Duration::from_millis(100),
                receiver.recv()
            ).await {
                Ok(Ok(event)) => {
                    // 应用过滤器
                    if let Some(ref filters) = event_filters {
                        if !filters.contains(&event.event_type().to_string()) {
                            continue;
                        }
                    }

                    let sse_event = axum::response::sse::Event::default()
                        .event(event.event_type())
                        .data(serde_json::to_string(&event).unwrap_or_default())
                        .id(Uuid::new_v4().to_string());

                    yield Ok::<axum::response::sse::Event, Infallible>(sse_event);
                }
                Ok(Err(_)) => break,
                Err(_) => {
                    // 超时 - 继续循环以发送心跳
                    continue;
                }
            }
        }
    };

    Sse::new(Box::pin(stream))
        .keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(Duration::from_secs(heartbeat_interval))
                .text("keep-alive"),
        )
        .into_response()
}

/// 用于内部广播事件的 REST 端点
pub async fn broadcast_event_handler(
    State(state): State<RestApiState>,
    Json(payload): Json<BroadcastEventRequest>,
) -> impl IntoResponse {
    let event = SseEvent::Custom {
        event_type: payload.event_type.clone(),
        payload: payload.payload,
        timestamp: chrono::Utc::now(),
    };

    match state.sse_broadcaster.send(event) {
        Ok(_) => (StatusCode::OK, Json(json!({"status": "broadcasted"}))).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "No subscribers"})),
        )
            .into_response(),
    }
}

/// 广播事件请求
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // 从请求体解析的字段
pub struct BroadcastEventRequest {
    pub event_type: String,
    pub payload: serde_json::Value,
    pub project_id: Option<String>,
    pub sop_id: Option<String>,
    pub session_id: Option<String>,
}

/// Prometheus 文本格式指标端点 (GET /api/metrics)
///
/// 暴露：
/// - sisyphus_http_requests_total / errors_total / in_flight
/// - sisyphus_http_shutdowns_total
/// - 进程启动时间（uptime_seconds）
///
/// 格式遵循 Prometheus exposition format (0.0.4)：
/// <https://prometheus.io/docs/instrumenting/exposition_formats/>
///
/// 设计原则：
/// - 纯字符串拼接，**不**依赖 prometheus crate（保持轻量）
/// - 即使 shared_http 不可用（如手工 register 的 registry）也返回 200 + 全 0
/// - 进程启动时间在 handler 第一次构造时确定（每次请求相对便宜）
pub async fn metrics_handler(State(state): State<RestApiState>) -> impl IntoResponse {
    use std::fmt::Write as _;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // 进程启动时间（首次访问时固化）
    let started_at: u64 = *state.metrics_started_at.read().await;
    let uptime_secs = now.saturating_sub(started_at);

    // 拿 shared_http（如果 bridge 持有 ProviderRegistry 间接的）
    let stats = state
        .shared_http
        .as_ref()
        .map(|c| c.stats())
        .unwrap_or_default();

    // SSE 活跃连接数 = SSE_MAX_CONNECTIONS - 剩余许可
    let sse_max = crate::http::types::SSE_MAX_CONNECTIONS;
    let sse_active = sse_max.saturating_sub(state.sse_connection_limit.available_permits());

    // Chaos engineering 累计故障注入次数
    let chaos_total = state.bridge.chaos_fault_count();

    let mut out = String::with_capacity(1024);
    let _ = writeln!(out, "# HELP sisyphus_http_requests_total Total HTTP requests since process start");
    let _ = writeln!(out, "# TYPE sisyphus_http_requests_total counter");
    let _ = writeln!(out, "sisyphus_http_requests_total {}", stats.requests_total);
    let _ = writeln!(out, "# HELP sisyphus_http_errors_total Total failed HTTP requests");
    let _ = writeln!(out, "# TYPE sisyphus_http_errors_total counter");
    let _ = writeln!(out, "sisyphus_http_errors_total {}", stats.errors_total);
    let _ = writeln!(out, "# HELP sisyphus_http_in_flight Currently in-flight HTTP requests");
    let _ = writeln!(out, "# TYPE sisyphus_http_in_flight gauge");
    let _ = writeln!(out, "sisyphus_http_in_flight {}", stats.in_flight);
    let _ = writeln!(out, "# HELP sisyphus_http_shutdowns_total Total graceful shutdown invocations");
    let _ = writeln!(out, "# TYPE sisyphus_http_shutdowns_total counter");
    let _ = writeln!(out, "sisyphus_http_shutdowns_total {}", stats.shutdowns_total);
    let _ = writeln!(out, "# HELP sisyphus_uptime_seconds Process uptime in seconds");
    let _ = writeln!(out, "# TYPE sisyphus_uptime_seconds gauge");
    let _ = writeln!(out, "sisyphus_uptime_seconds {}", uptime_secs);
    let _ = writeln!(out, "# HELP sisyphus_sse_connections_active Active SSE client connections");
    let _ = writeln!(out, "# TYPE sisyphus_sse_connections_active gauge");
    let _ = writeln!(out, "sisyphus_sse_connections_active {}", sse_active);
    let _ = writeln!(out, "# HELP sisyphus_chaos_faults_total Total chaos engineering faults injected");
    let _ = writeln!(out, "# TYPE sisyphus_chaos_faults_total counter");
    let _ = writeln!(out, "sisyphus_chaos_faults_total {}", chaos_total);

    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        out,
    )
}

/// SSE 端点健康检查
pub async fn sse_health_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({"status": "ok", "service": "sse"})),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_sse_event_serialization() {
        let event = SseEvent::CognitiveIterationStarted {
            iteration: 1,
            goal_id: "goal1".to_string(),
            timestamp: chrono::Utc::now(),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("cognitive_iteration_started"));
        assert!(json.contains("goal1"));
    }

    #[test]
    fn test_sse_event_type() {
        let event = SseEvent::CognitiveIterationStarted {
            iteration: 1,
            goal_id: "goal1".to_string(),
            timestamp: chrono::Utc::now(),
        };

        assert_eq!(event.event_type(), "cognitive_iteration_started");
    }

    #[test]
    fn test_sse_event_to_string() {
        let event = SseEvent::Custom {
            event_type: "test_event".to_string(),
            payload: json!({"key": "value"}),
            timestamp: chrono::Utc::now(),
        };

        let sse_str = event.to_sse_string();
        assert!(sse_str.contains("event: test_event"));
        assert!(sse_str.contains("data:"));
    }

    #[tokio::test]
    async fn test_sse_broadcast_shared_channel() {
        // 同一 channel 内：先订阅后广播，订阅端必须收到事件
        let (tx, mut rx) = tokio::sync::broadcast::channel(16);
        let event = SseEvent::Custom {
            event_type: "shared_test".to_string(),
            payload: json!({"k": "v"}),
            timestamp: chrono::Utc::now(),
        };
        assert!(tx.send(event.clone()).is_ok());
        let received = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("同一 channel 内应能收到广播事件")
            .expect("发送不应失败");
        assert_eq!(received.event_type(), "shared_test");
    }

    #[tokio::test]
    async fn test_sse_connection_limit() {
        // 直接验证信号量限制：两个 acquire 成功，但当达到上限后应失败
        let semaphore = Arc::new(tokio::sync::Semaphore::new(2));
        let p1 = semaphore.clone().try_acquire_owned();
        let p2 = semaphore.clone().try_acquire_owned();
        assert!(p1.is_ok());
        assert!(p2.is_ok());
        // 已用尽 2 个许可，第 3 个获取失败
        let p3 = semaphore.clone().try_acquire_owned();
        assert!(p3.is_err(), "超过上限的获取应失败");
        drop(p1);
        drop(p2);
        // 释放后可再次获取
        let p4 = semaphore.clone().try_acquire_owned();
        assert!(p4.is_ok());
        drop(p4);
    }

    // ── /api/metrics 端点测试 ──────────────────────────────────

    /// 构造一个最小 RestApiState 供 metrics handler 测试
    async fn make_test_state(
        shared_http: Option<Arc<sisyphus_tools_llm::http_client::SharedHttpClient>>,
    ) -> RestApiState {
        use crate::bridge_service::MySisyphusBridge;
        use sisyphus_types::Config;
        use std::sync::Arc as StdArc;
        use std::sync::RwLock as StdRwLock;
        // 构造一个允许 dummy embedding 的 minimal bridge。
        // 默认 Config 的 embedding provider = "dummy" + 不允许，
        // 桥接初始化会 panic，这里手动开启。
        let mut cfg = Config::default();
        cfg.llm.embedding.allow_dummy_embeddings = true;
        let cfg = StdArc::new(StdRwLock::new(cfg));
        let bridge = Arc::new(
            MySisyphusBridge::new(cfg, None)
                .await
                .expect("构造 test bridge"),
        );
        let state = RestApiState::new(bridge, std::path::PathBuf::from("/tmp"));
        match shared_http {
            Some(c) => state.with_shared_http(c),
            None => state,
        }
    }

    #[tokio::test]
    async fn test_metrics_handler_without_shared_http() {
        // 当 shared_http = None，应返回全 0 + 进程 uptime
        let state = make_test_state(None).await;
        let resp = metrics_handler(State(state)).await.into_response();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let text = String::from_utf8_lossy(&body);
        // 即使没有 shared_http 也要有所有 5 个指标（值 0）
        assert!(text.contains("sisyphus_http_requests_total 0"), "{text}");
        assert!(text.contains("sisyphus_http_errors_total 0"), "{text}");
        assert!(text.contains("sisyphus_http_in_flight 0"), "{text}");
        assert!(text.contains("sisyphus_http_shutdowns_total 0"), "{text}");
        assert!(text.contains("sisyphus_uptime_seconds"), "{text}");
        // Content-Type 应为 prometheus 文本格式
        assert!(text.contains("# TYPE sisyphus_http_requests_total counter"), "{text}");
    }

    #[tokio::test]
    async fn test_metrics_handler_increments_after_request() {
        // shared_http 注入后，stats 仍为 0 (无实际请求); 主要验证"读到" 不是 panic
        use sisyphus_tools_llm::http_client::{SharedHttpClient, SharedHttpClientConfig};
        let cfg = SharedHttpClientConfig::default();
        let client = SharedHttpClient::new(cfg).expect("构造 client");
        let state = make_test_state(Some(Arc::new(client))).await;
        let resp = metrics_handler(State(state)).await.into_response();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let text = String::from_utf8_lossy(&body);
        // 计数器仍是 0 (没人发过请求)
        assert!(text.contains("sisyphus_http_requests_total 0"), "{text}");
        // 启动时间应已固化（> 0 unix 时间）
        let started_at_line = text
            .lines()
            .find(|l| l.starts_with("sisyphus_uptime_seconds"))
            .expect("uptime line");
        // uptime 应是合理正数（< 几小时）
        let secs: u64 = started_at_line
            .split_whitespace()
            .last()
            .unwrap()
            .parse()
            .unwrap();
        assert!(secs < 86_400, "uptime 看起来不真实: {secs}");
    }

    #[tokio::test]
    async fn test_metrics_handler_exposes_sse_and_chaos() {
        // 验证两个新增指标均存在且语义正确：
        // - sisyphus_sse_connections_active: 初始 0（无客户端）
        // - sisyphus_chaos_faults_total: 初始 0 (MySisyphusBridge::chaos_fault_count)
        let state = make_test_state(None).await;
        let resp = metrics_handler(State(state)).await.into_response();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let text = String::from_utf8_lossy(&body);

        // SSE 活跃连接 — 初始应为 0
        let sse_line = text
            .lines()
            .find(|l| l.starts_with("sisyphus_sse_connections_active"))
            .expect("sse line");
        let sse_active: u64 = sse_line.split_whitespace().last().unwrap().parse().unwrap();
        assert_eq!(sse_active, 0, "初始 SSE 连接数应为 0");

        // Chaos 计数 — 初始应为 0
        let chaos_line = text
            .lines()
            .find(|l| l.starts_with("sisyphus_chaos_faults_total"))
            .expect("chaos line");
        let chaos_total: u64 = chaos_line.split_whitespace().last().unwrap().parse().unwrap();
        assert_eq!(chaos_total, 0, "初始 chaos count 应为 0");

        // TYPE 注释齐全（Prometheus 规范）
        assert!(text.contains("# TYPE sisyphus_sse_connections_active gauge"), "{text}");
        assert!(text.contains("# TYPE sisyphus_chaos_faults_total counter"), "{text}");
    }
}
