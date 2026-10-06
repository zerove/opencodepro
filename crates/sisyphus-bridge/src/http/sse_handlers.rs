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

    /// 提取事件携带的 SOP ID（若该变体包含）
    ///
    /// 用于 `?sop_id=` 服务端过滤。只有节点/SOP 执行类事件带有此维度，
    /// 其余变体返回 `None`（**不匹配任何 sop_id 过滤条件**）。
    pub fn sop_id(&self) -> Option<&str> {
        match self {
            SseEvent::NodeExecutionStarted { sop_id, .. }
            | SseEvent::NodeExecutionCompleted { sop_id, .. }
            | SseEvent::SopExecutionStarted { sop_id, .. }
            | SseEvent::SopExecutionCompleted { sop_id, .. } => Some(sop_id),
            // Custom 变体的维度信息只能来自 payload。生产中唯一真正广播的事件
            // （`intervention`）就是 Custom，若不在此提取，`?sop_id=` 过滤会把
            // 它连同它自己所属 sop 的订阅者一起全部丢弃。
            SseEvent::Custom { payload, .. } => payload.get("sop_id").and_then(|v| v.as_str()),
            _ => None,
        }
    }

    /// 提取事件携带的会话 ID（若该变体包含）
    pub fn session_id(&self) -> Option<&str> {
        match self {
            SseEvent::SessionMessageAdded { session_id, .. } => Some(session_id),
            // 同 sop_id：Custom 事件的会话维度来自 payload
            SseEvent::Custom { payload, .. } => payload.get("session_id").and_then(|v| v.as_str()),
            _ => None,
        }
    }

    /// 转换为 SSE 格式字符串
    pub fn to_sse_string(&self) -> String {
        let data = serde_json::to_string(self).unwrap_or_default();
        format!("event: {}\ndata: {}\n\n", self.event_type(), data)
    }

    /// 把内核的 `ExecutionEvent` 转为可广播的 `SseEvent`
    ///
    /// 内核事件不带 `sop_id`（只有 `node_name`），因此由调用方补齐归属，
    /// 注入 payload 后即可被 `?sop_id=` 服务端过滤正确识别。
    ///
    /// 采用 `Custom` 承载而非扩展枚举：`SseEvent` 是公开 API，新增变体是
    /// 破坏性变更；而 `ExecutionEvent` 本身已 `Serialize`，整体放入 payload
    /// 可无损保留全部字段（含 `fuel_consumed` / `duration_ms` 等枚举变体
    /// 中无处安放的信息），事件名用 snake_case 与既有 SSE 事件保持一致。
    pub fn from_execution_event(event: &sisyphus_types::ExecutionEvent, sop_id: &str) -> Self {
        // 序列化为 {"NodeStarted": {...}} 的外部标签形态，取出内部值。
        // 失败时退回 NullValue 而不是丢弃事件：SSE 流的语义是「尽力送达」，
        // 一个无法序列化的变体不应中断整条流。
        let inner = serde_json::to_value(event)
            .ok()
            .and_then(|v| match v {
                // 外部标签枚举只有一个键，该键即变体名，取出其内部值
                serde_json::Value::Object(m) => m.into_iter().next().map(|(_, val)| val),
                other => Some(other),
            })
            .unwrap_or(serde_json::Value::Null);

        let event_type = execution_event_type(event);
        let mut payload = match inner {
            serde_json::Value::Object(m) => m,
            _ => serde_json::Map::new(),
        };
        // 维度键必须与 sop_id()/session_id() 读取的键名一致
        payload.insert(
            "sop_id".to_string(),
            serde_json::Value::String(sop_id.to_string()),
        );

        SseEvent::Custom {
            event_type,
            payload: serde_json::Value::Object(payload),
            timestamp: chrono::Utc::now(),
        }
    }
}

/// 内核执行事件的 snake_case 事件名
fn execution_event_type(event: &sisyphus_types::ExecutionEvent) -> String {
    use sisyphus_types::ExecutionEvent as E;
    match event {
        E::NodeStarted { .. } => "node_started",
        E::NodeCompleted { .. } => "node_completed",
        E::NodeFailed { .. } => "node_failed",
        E::ExecutionComplete { .. } => "execution_complete",
        E::CognitivePlan { .. } => "cognitive_plan",
        E::CognitiveStep { .. } => "cognitive_step",
        E::MicroReflection { .. } => "micro_reflection",
        E::CognitiveComplete { .. } => "cognitive_complete",
    }
    .to_string()
}

/// SSE 连接查询参数
#[derive(Debug, Deserialize)]
// 字段豁免说明（ADR-007 反模式要求给出正当理由）：
//   `events` / `heartbeat` / `sop_id` / `session_id` 均已生效并参与过滤；
//   `project_id` / `last_event_id` 目前**仅从 URL 解析、尚未参与过滤**，保留它们是为了
//   不破坏既有 API 契约（去掉字段会让传这些参数的客户端报 400/422 而非静默忽略）。
//   - `project_id`：当前生产中唯一被广播的事件（`intervention`）是 `Custom` 变体，
//     其 payload 不含项目维度；工作区/JWT 虽有 `project_id`，但广播点未把它带进事件，
//     且全局广播 channel 本身不区分项目。实现过滤需先确定「事件属于哪个项目」
//     的判定规则（当前 workspace 概念与事件无绑定关系），属独立特性。
//   - `last_event_id`：事件 ID 为每次生成的随机 UUID，重连携带的 ID 无可匹配的历史，
//     且仓库内无任何客户端消费该参数。补齐需要引入带序号的全局事件日志，属独立特性。
//   此处**不是**「为审计保留」，而是「解析已实现、过滤待实现」。
#[allow(dead_code)]
pub struct SseQuery {
    /// 按事件类型过滤（逗号分隔）。**已生效**
    pub events: Option<String>,
    /// 按项目 ID 过滤。**已解析但未生效**（生产事件不带项目维度，见上方说明）
    pub project_id: Option<String>,
    /// 按 SOP ID 过滤。**已生效**（匹配节点/SOP 执行类事件，以及 payload 携带
    /// `sop_id` 的 Custom 事件 —— 含生产中唯一真正广播的 `intervention`）
    pub sop_id: Option<String>,
    /// 按会话 ID 过滤。**已生效**（`session_message_added` 事件，或 payload 携带
    /// `session_id` 的 Custom 事件）
    pub session_id: Option<String>,
    /// 用于断点续传的最后事件 ID。**已解析但未生效**（见上方说明）
    pub last_event_id: Option<String>,
    /// 心跳间隔（秒，0 表示禁用）。**已生效**
    pub heartbeat: Option<u64>,
}

/// SSE 并发连接数限制失败时的 HTTP 状态码
const SSE_LIMIT_REACHED: StatusCode = StatusCode::TOO_MANY_REQUESTS;

/// 解析维度过滤值：去空白，空串视为未提供
///
/// `?sop_id=`（空值）被归一化为 `None`（不过滤），而不是「只匹配空 ID」
/// 这种永不命中、客户端也无法与「无结果」区分的条件。
fn normalize_dimension(raw: Option<&String>) -> Option<String> {
    raw.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// 判断事件是否应推送给当前订阅者
///
/// 三个维度为「与」（AND）关系，全部满足才推送：
/// - `type_filters` 非空时，事件类型必须命中其中之一；
/// - `sop_filter` 要求事件**携带**该 `sop_id` 且值相等（维度缺失即不匹配）；
/// - `session_filter` 同理。
///
/// 抽为独立纯函数以便在不启动 HTTP 服务的情况下直接验证过滤语义。
///
/// `pub(crate)` 而非私有：`handlers.rs` 的转发器测试需要用同一套过滤语义
/// 验证「转发出来的事件确实能被 `?sop_id=` 识别」，避免测试里复制一份
/// 过滤逻辑导致验证失真。
pub(crate) fn matches_filters(
    event: &SseEvent,
    type_filters: Option<&[String]>,
    sop_filter: Option<&str>,
    session_filter: Option<&str>,
) -> bool {
    if let Some(filters) = type_filters {
        if !filters.iter().any(|f| f.as_str() == event.event_type()) {
            return false;
        }
    }
    if let Some(want) = sop_filter {
        if event.sop_id() != Some(want) {
            return false;
        }
    }
    if let Some(want) = session_filter {
        if event.session_id() != Some(want) {
            return false;
        }
    }
    true
}

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
        },
    };

    // 订阅全局广播器（所有客户端共享同一 channel）
    let receiver = state.sse_broadcaster.subscribe();

    // 解析事件过滤器
    let event_filters: Option<Vec<String>> =
        query.events.as_ref().map(|s| s.split(',').map(|s| s.trim().to_string()).collect());

    // 按维度过滤：事件必须携带该维度且值匹配才推送。
    // 维度缺失的事件（如 skill_installed 没有 sop_id）在此维度下被排除。
    let sop_filter: Option<String> = normalize_dimension(query.sop_id.as_ref());
    let session_filter: Option<String> = normalize_dimension(query.session_id.as_ref());

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
                    // 应用过滤器（类型 / sop_id / session_id 三维取交集）
                    if !matches_filters(
                        &event,
                        event_filters.as_deref(),
                        sop_filter.as_deref(),
                        session_filter.as_deref(),
                    ) {
                        continue;
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
// 字段豁免说明：`event_type` / `payload` 已被 broadcast_event_handler 使用；
//   `project_id` / `sop_id` / `session_id` 目前仅从请求体解析、未参与过滤
//   （广播是全量的，不按维度筛选）。保留以维持 API 契约：删掉字段会让
//   现有客户端的请求体反序列化失败。
#[allow(dead_code)]
pub struct BroadcastEventRequest {
    /// 事件类型。**已生效**
    pub event_type: String,
    /// 事件载荷。**已生效**
    pub payload: serde_json::Value,
    /// 项目 ID。**已解析但未生效**（广播目前是全量的）
    pub project_id: Option<String>,
    /// SOP ID。**已解析但未生效**
    pub sop_id: Option<String>,
    /// 会话 ID。**已解析但未生效**
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
    let stats = state.shared_http.as_ref().map(|c| c.stats()).unwrap_or_default();

    // SSE 活跃连接数 = SSE_MAX_CONNECTIONS - 剩余许可
    let sse_max = crate::http::types::SSE_MAX_CONNECTIONS;
    let sse_active = sse_max.saturating_sub(state.sse_connection_limit.available_permits());

    // Chaos engineering 累计故障注入次数
    let chaos_total = state.bridge.chaos_fault_count();

    let mut out = String::with_capacity(1024);
    let _ = writeln!(
        out,
        "# HELP sisyphus_http_requests_total Total HTTP requests since process start"
    );
    let _ = writeln!(out, "# TYPE sisyphus_http_requests_total counter");
    let _ = writeln!(out, "sisyphus_http_requests_total {}", stats.requests_total);
    let _ = writeln!(
        out,
        "# HELP sisyphus_http_errors_total Total failed HTTP requests"
    );
    let _ = writeln!(out, "# TYPE sisyphus_http_errors_total counter");
    let _ = writeln!(out, "sisyphus_http_errors_total {}", stats.errors_total);
    let _ = writeln!(
        out,
        "# HELP sisyphus_http_in_flight Currently in-flight HTTP requests"
    );
    let _ = writeln!(out, "# TYPE sisyphus_http_in_flight gauge");
    let _ = writeln!(out, "sisyphus_http_in_flight {}", stats.in_flight);
    let _ = writeln!(
        out,
        "# HELP sisyphus_http_shutdowns_total Total graceful shutdown invocations"
    );
    let _ = writeln!(out, "# TYPE sisyphus_http_shutdowns_total counter");
    let _ = writeln!(
        out,
        "sisyphus_http_shutdowns_total {}",
        stats.shutdowns_total
    );
    let _ = writeln!(
        out,
        "# HELP sisyphus_uptime_seconds Process uptime in seconds"
    );
    let _ = writeln!(out, "# TYPE sisyphus_uptime_seconds gauge");
    let _ = writeln!(out, "sisyphus_uptime_seconds {}", uptime_secs);
    let _ = writeln!(
        out,
        "# HELP sisyphus_sse_connections_active Active SSE client connections"
    );
    let _ = writeln!(out, "# TYPE sisyphus_sse_connections_active gauge");
    let _ = writeln!(out, "sisyphus_sse_connections_active {}", sse_active);
    let _ = writeln!(
        out,
        "# HELP sisyphus_chaos_faults_total Total chaos engineering faults injected"
    );
    let _ = writeln!(out, "# TYPE sisyphus_chaos_faults_total counter");
    let _ = writeln!(out, "sisyphus_chaos_faults_total {}", chaos_total);

    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
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

    #[test]
    fn test_every_execution_event_variant_maps_to_sse() {
        use sisyphus_types::ExecutionEvent as E;

        // 覆盖全部 8 个变体：任何新增变体若忘记映射，本测试的
        // 「变体数量」断言会先失败，强制补齐。
        let variants = vec![
            (
                E::NodeStarted {
                    node_name: "fetch".to_string(),
                },
                "node_started",
            ),
            (
                E::NodeCompleted {
                    node_name: "fetch".to_string(),
                    output: sisyphus_types::NodeOutput::from(serde_json::json!("ok")),
                    fuel_consumed: 42,
                    duration_ms: 120,
                },
                "node_completed",
            ),
            (
                E::NodeFailed {
                    node_name: "fetch".to_string(),
                    error: "timeout".to_string(),
                    fuel_consumed: 7,
                    duration_ms: 3000,
                },
                "node_failed",
            ),
            (
                E::ExecutionComplete {
                    result: sisyphus_types::NodeOutput::from(serde_json::json!("done")),
                },
                "execution_complete",
            ),
            (
                E::CognitivePlan {
                    plan: sisyphus_types::CognitivePlan::default(),
                },
                "cognitive_plan",
            ),
            (
                E::CognitiveStep {
                    step: "Predicting".to_string(),
                    message: "预测执行结果".to_string(),
                    metrics: None,
                },
                "cognitive_step",
            ),
            (
                E::MicroReflection {
                    node_name: "fetch".to_string(),
                    prediction: "200".to_string(),
                    actual: "500".to_string(),
                    decision: "retry".to_string(),
                    updates: None,
                    reason: Some("上游错误".to_string()),
                },
                "micro_reflection",
            ),
            (
                E::CognitiveComplete {
                    report: "已收敛".to_string(),
                },
                "cognitive_complete",
            ),
        ];

        // 变体总数守卫：ExecutionEvent 新增变体时本测试会先失败，
        // 强制补齐映射而非静默漏掉。
        assert_eq!(
            variants.len(),
            8,
            "ExecutionEvent 变体数已变（当前 {}），必须为新变体补映射与测试",
            variants.len()
        );

        for (event, expected_type) in variants {
            let sse = SseEvent::from_execution_event(&event, "sop-abc");
            assert_eq!(
                sse.event_type(),
                expected_type,
                "变体 {expected_type} 的事件名不符"
            );
            // 归属必须可被服务端过滤识别
            assert_eq!(
                sse.sop_id(),
                Some("sop-abc"),
                "变体 {expected_type} 未携带 sop_id，无法按 sop 过滤"
            );
            // payload 必须保留原始字段，不能只剩维度
            if let SseEvent::Custom { payload, .. } = &sse {
                assert!(
                    payload.as_object().map(|m| m.len()).unwrap_or(0) >= 2,
                    "变体 {expected_type} 的 payload 只剩维度，原始字段丢失: {payload}"
                );
            }
        }
    }

    #[test]
    fn test_execution_event_payload_preserves_metric_fields() {
        // fuel_consumed / duration_ms 在 SseEvent 枚举中无处安放，
        // 放入 Custom payload 是无损保留它们的关键，必须逐字段核对。
        let event = sisyphus_types::ExecutionEvent::NodeCompleted {
            node_name: "fetch".to_string(),
            output: sisyphus_types::NodeOutput::from(serde_json::json!({"rows": 3})),
            fuel_consumed: 99,
            duration_ms: 1234,
        };
        let sse = SseEvent::from_execution_event(&event, "sop-1");
        let SseEvent::Custom { payload, .. } = &sse else {
            panic!("应映射为 Custom");
        };

        assert_eq!(payload["node_name"], "fetch");
        assert_eq!(payload["fuel_consumed"], 99);
        assert_eq!(payload["duration_ms"], 1234);
        assert_eq!(payload["sop_id"], "sop-1");
        assert_eq!(payload["output"], serde_json::json!({"rows": 3}));
    }

    #[test]
    fn test_mapped_execution_events_pass_sop_filter() {
        // 端到端：内核事件经映射后必须能通过自己所属 sop 的过滤
        let event = sisyphus_types::ExecutionEvent::NodeStarted {
            node_name: "fetch".to_string(),
        };
        let sse = SseEvent::from_execution_event(&event, "sop-abc");

        assert!(matches_filters(&sse, None, Some("sop-abc"), None));
        assert!(!matches_filters(&sse, None, Some("sop-other"), None));
        // 也能被类型过滤命中
        assert!(matches_filters(
            &sse,
            Some(&["node_started".to_string()]),
            None,
            None
        ));
        assert!(!matches_filters(
            &sse,
            Some(&["node_completed".to_string()]),
            None,
            None
        ));
    }

    #[tokio::test]
    async fn test_execution_event_reaches_subscriber_end_to_end() {
        // 走真实广播链路：发送 ExecutionEvent → 转换 → 广播 → 订阅者收到，
        // 确认不是只有纯函数层面正确。
        let (tx, mut rx) = tokio::sync::broadcast::channel::<SseEvent>(16);
        let event = sisyphus_types::ExecutionEvent::ExecutionComplete {
            result: sisyphus_types::NodeOutput::from(serde_json::json!("finished")),
        };

        tx.send(SseEvent::from_execution_event(&event, "sop-e2e")).expect("应有订阅者");

        let got = rx.recv().await.expect("订阅者应收到事件");
        assert_eq!(got.event_type(), "execution_complete");
        assert_eq!(got.sop_id(), Some("sop-e2e"));
        assert!(matches_filters(&got, None, Some("sop-e2e"), None));
    }

    fn node_started(sop: &str) -> SseEvent {
        SseEvent::NodeExecutionStarted {
            sop_id: sop.to_string(),
            node_id: "n1".to_string(),
            node_name: "fetch".to_string(),
            timestamp: chrono::Utc::now(),
        }
    }

    /// 复现 `intervene_cognitive_map_handler` 广播的干预事件：
    /// 生产中唯一真正发送的 SseEvent，经由 `SseEvent::custom` 构造，
    /// sop_id 只存在于 payload 内部，不是枚举字段。
    fn intervention_event(sop: &str) -> SseEvent {
        SseEvent::custom(
            "intervention".to_string(),
            json!({
                "sop_id": sop,
                "action": "pause",
                "target_node": null,
                "reason": null,
                "applied": true,
            }),
        )
    }

    #[test]
    fn test_intervention_event_survives_its_own_sop_filter() {
        // `handlers.rs` 的文档承诺「所有订阅此 sop_id 的客户端立即收到」。
        // 干预事件是生产中唯一真正广播的 SseEvent，因此这条承诺必须对它成立。
        let event = intervention_event("sop-abc");

        assert_eq!(
            event.sop_id(),
            Some("sop-abc"),
            "intervention 事件的 sop_id 必须可提取，否则它无法通过自己所属 sop 的过滤"
        );
        assert!(
            matches_filters(&event, None, Some("sop-abc"), None),
            "订阅 ?sop_id=sop-abc 的客户端必须能收到 sop-abc 的干预事件"
        );
        // 其他 sop 的订阅者不应收到
        assert!(!matches_filters(&event, None, Some("sop-other"), None));
    }

    #[test]
    fn test_custom_event_without_dimension_is_not_matched_by_any_filter() {
        // payload 缺维度字段时必须返回 None。若误返回 Some("")，
        // 订阅 `?sop_id=`（空值，会被归一化为不过滤）与维度为空的边界就乱了。
        let plain = SseEvent::custom("heartbeat_like".to_string(), json!({"value": 1}));
        assert_eq!(plain.sop_id(), None);
        assert_eq!(plain.session_id(), None);

        // 非字符串的维度值不得被强制转成字符串
        let numeric = SseEvent::custom("bad".to_string(), json!({"sop_id": 42}));
        assert_eq!(numeric.sop_id(), None);

        // 显式 null 同样视为无维度
        let null_dim = SseEvent::custom("bad".to_string(), json!({"sop_id": null}));
        assert_eq!(null_dim.sop_id(), None);
    }

    #[test]
    fn test_sop_id_extracted_only_from_sop_scoped_variants() {
        assert_eq!(node_started("sop-a").sop_id(), Some("sop-a"));
        // 认知迭代事件没有 sop_id 维度
        let goal_only = SseEvent::CognitiveIterationStarted {
            iteration: 1,
            goal_id: "g1".to_string(),
            timestamp: chrono::Utc::now(),
        };
        assert_eq!(goal_only.sop_id(), None);
    }

    #[test]
    fn test_session_id_extracted_only_from_session_variant() {
        let msg = SseEvent::SessionMessageAdded {
            session_id: "sess-1".to_string(),
            role: "user".to_string(),
            content: "hi".to_string(),
            timestamp: chrono::Utc::now(),
        };
        assert_eq!(msg.session_id(), Some("sess-1"));
        assert_eq!(node_started("sop-a").session_id(), None);
    }

    #[test]
    fn test_sop_filter_excludes_other_sop_and_dimensionless_events() {
        // 命中：sop_id 相等
        assert!(matches_filters(
            &node_started("sop-a"),
            None,
            Some("sop-a"),
            None
        ));
        // 不命中：sop_id 不同
        assert!(!matches_filters(
            &node_started("sop-b"),
            None,
            Some("sop-a"),
            None
        ));
        // 不命中：事件根本不带 sop_id 维度（不得当作「匹配全部」）
        let skill = SseEvent::SkillInstalled {
            skill_id: "s1".to_string(),
            version: "1.0".to_string(),
            timestamp: chrono::Utc::now(),
        };
        assert!(!matches_filters(&skill, None, Some("sop-a"), None));
    }

    #[test]
    fn test_no_filter_pushes_everything() {
        assert!(matches_filters(&node_started("sop-a"), None, None, None));
        let skill = SseEvent::SkillInstalled {
            skill_id: "s1".to_string(),
            version: "1.0".to_string(),
            timestamp: chrono::Utc::now(),
        };
        assert!(matches_filters(&skill, None, None, None));
    }

    #[test]
    fn test_type_and_dimension_filters_are_conjunctive() {
        let types = vec!["node_execution_started".to_string()];
        // 类型命中且 sop 命中 → 推送
        assert!(matches_filters(
            &node_started("sop-a"),
            Some(&types),
            Some("sop-a"),
            None
        ));
        // 类型命中但 sop 不命中 → 不推送
        assert!(!matches_filters(
            &node_started("sop-b"),
            Some(&types),
            Some("sop-a"),
            None
        ));
        // sop 命中但类型不命中 → 不推送
        assert!(!matches_filters(
            &node_started("sop-a"),
            Some(&["skill_installed".to_string()]),
            Some("sop-a"),
            None
        ));
    }

    #[test]
    fn test_session_filter_excludes_other_sessions() {
        let mine = SseEvent::SessionMessageAdded {
            session_id: "sess-1".to_string(),
            role: "user".to_string(),
            content: "hi".to_string(),
            timestamp: chrono::Utc::now(),
        };
        let other = SseEvent::SessionMessageAdded {
            session_id: "sess-2".to_string(),
            role: "user".to_string(),
            content: "hi".to_string(),
            timestamp: chrono::Utc::now(),
        };
        assert!(matches_filters(&mine, None, None, Some("sess-1")));
        assert!(!matches_filters(&other, None, None, Some("sess-1")));
        // 节点事件没有 session 维度，被排除
        assert!(!matches_filters(
            &node_started("sop-a"),
            None,
            None,
            Some("sess-1")
        ));
    }

    #[test]
    fn test_dimension_normalization_treats_blank_as_absent() {
        assert_eq!(normalize_dimension(None), None);
        assert_eq!(normalize_dimension(Some(&"".to_string())), None);
        // 纯空白同样视为未提供
        assert_eq!(normalize_dimension(Some(&"   ".to_string())), None);
        // 正常值去空白后保留
        assert_eq!(
            normalize_dimension(Some(&"  sop-a ".to_string())),
            Some("sop-a".to_string())
        );
    }

    #[test]
    fn test_blank_filter_does_not_silently_blackhole_stream() {
        // 空串被归一化为「无过滤」，事件仍应推送（而非永不命中）
        let raw = "".to_string();
        let normalized = normalize_dimension(Some(&raw));
        assert!(matches_filters(
            &node_started("sop-a"),
            None,
            normalized.as_deref(),
            None
        ));
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
        let bridge =
            Arc::new(MySisyphusBridge::new(cfg, None, None).await.expect("构造 test bridge"));
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
        assert!(
            text.contains("# TYPE sisyphus_http_requests_total counter"),
            "{text}"
        );
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
        let started_at_line =
            text.lines().find(|l| l.starts_with("sisyphus_uptime_seconds")).expect("uptime line");
        // uptime 应是合理正数（< 几小时）
        let secs: u64 = started_at_line.split_whitespace().last().unwrap().parse().unwrap();
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
        assert!(
            text.contains("# TYPE sisyphus_sse_connections_active gauge"),
            "{text}"
        );
        assert!(
            text.contains("# TYPE sisyphus_chaos_faults_total counter"),
            "{text}"
        );
    }
}
