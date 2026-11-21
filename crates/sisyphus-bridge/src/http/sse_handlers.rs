//! 服务端发送事件 (SSE) 处理器，用于实时事件流式传输。
//!
//! 提供 `/api/events` 端点，基于 Server-Sent Events 实现实时事件流式传输。

use axum::{
    extract::{Extension, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response, Sse},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::borrow::Cow;
use std::convert::Infallible;
use std::time::Duration;

use crate::http::RestApiState;

/// SSE 事件类型
///
/// `timestamp` 字段统一为 `jiff::Timestamp`（chrono 迁移后类型），serde 序列化为
/// RFC 3339 UTC 字符串。注意：jiff 的 Display/Serialize 会 trim 尾零，与迁移前
/// chrono 的 `AutoSi` 不同，但输出仍合法 RFC 3339 — 下游 consumer 无需适配。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SseEvent {
    /// 认知迭代开始
    CognitiveIterationStarted {
        iteration: usize,
        goal_id: String,
        timestamp: jiff::Timestamp,
    },
    /// 认知迭代完成
    CognitiveIterationCompleted {
        iteration: usize,
        goal_id: String,
        deviation: f64,
        converged: bool,
        timestamp: jiff::Timestamp,
    },
    /// 节点执行开始
    NodeExecutionStarted {
        sop_id: String,
        node_id: String,
        node_name: String,
        timestamp: jiff::Timestamp,
    },
    /// 节点执行完成
    NodeExecutionCompleted {
        sop_id: String,
        node_id: String,
        node_name: String,
        success: bool,
        output: Option<serde_json::Value>,
        timestamp: jiff::Timestamp,
    },
    /// SOP 执行开始
    SopExecutionStarted {
        sop_id: String,
        goal_id: String,
        timestamp: jiff::Timestamp,
    },
    /// SOP 执行完成
    SopExecutionCompleted {
        sop_id: String,
        goal_id: String,
        success: bool,
        timestamp: jiff::Timestamp,
    },
    /// 混沌故障注入
    ChaosFaultInjected {
        fault_type: String,
        target: String,
        timestamp: jiff::Timestamp,
    },
    /// 混沌自动修复触发
    ChaosAutoFixTriggered {
        fault_count: u64,
        timestamp: jiff::Timestamp,
    },
    /// 技能安装
    SkillInstalled {
        skill_id: String,
        version: String,
        timestamp: jiff::Timestamp,
    },
    /// 技能卸载
    SkillUninstalled {
        skill_id: String,
        timestamp: jiff::Timestamp,
    },
    /// 插件加载
    PluginLoaded {
        plugin_id: String,
        version: String,
        timestamp: jiff::Timestamp,
    },
    /// 插件卸载
    PluginUnloaded {
        plugin_id: String,
        timestamp: jiff::Timestamp,
    },
    /// 工作区切换
    WorkspaceSwitched {
        workspace_id: String,
        workspace_name: String,
        timestamp: jiff::Timestamp,
    },
    /// 会话消息新增
    SessionMessageAdded {
        session_id: String,
        role: String,
        content: String,
        timestamp: jiff::Timestamp,
    },
    /// 自定义事件
    Custom {
        event_type: String,
        payload: serde_json::Value,
        timestamp: jiff::Timestamp,
    },
}

impl SseEvent {
    /// 创建自定义事件
    pub fn custom(event_type: String, payload: serde_json::Value) -> Self {
        Self::Custom {
            event_type,
            payload,
            timestamp: jiff::Timestamp::now(),
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

    /// 提取事件携带的项目 ID（若该变体包含）
    ///
    /// `Custom` 变体从 payload 读取。用于 `?project_id=` 服务端过滤：
    /// 广播端点会把请求顶层的 `project_id` 注入 payload，订阅方据此判定归属。
    /// 不携带项目维度的事件不会投递给带项目过滤的订阅者（避免跨项目泄露）。
    pub fn project_id(&self) -> Option<&str> {
        match self {
            SseEvent::Custom { payload, .. } => payload.get("project_id").and_then(|v| v.as_str()),
            _ => None,
        }
    }

    /// 提取事件携带的目标 ID（若该变体包含）
    ///
    /// 认知迭代 / SOP 执行事件带结构化 `goal_id`；`Custom` 变体从 payload 读取
    /// （team 事件转发器会把 `goal_id` 写入 payload）。用于 `?goal_id=` 过滤，
    /// 让客户端只订阅某一次团队/目标执行的事件。
    pub fn goal_id(&self) -> Option<&str> {
        match self {
            SseEvent::CognitiveIterationStarted { goal_id, .. }
            | SseEvent::CognitiveIterationCompleted { goal_id, .. }
            | SseEvent::SopExecutionStarted { goal_id, .. }
            | SseEvent::SopExecutionCompleted { goal_id, .. } => Some(goal_id),
            SseEvent::Custom { payload, .. } => payload.get("goal_id").and_then(|v| v.as_str()),
            _ => None,
        }
    }

    /// Team 事件：任务开始（M1.3 对外一致暴露）。
    pub fn team_task_started(
        goal_id: impl Into<String>,
        task_id: impl Into<String>,
        agent_id: impl Into<String>,
        tool_id: impl Into<String>,
    ) -> Self {
        Self::custom(
            "team_task_started".into(),
            json!({
                "goal_id": goal_id.into(),
                "task_id": task_id.into(),
                "agent_id": agent_id.into(),
                "tool_id": tool_id.into(),
            }),
        )
    }

    /// Team 事件：任务完成（M1.3 对外一致暴露）。
    pub fn team_task_completed(
        goal_id: impl Into<String>,
        task_id: impl Into<String>,
        agent_id: impl Into<String>,
        success: bool,
        duration_ms: u128,
        error: Option<String>,
    ) -> Self {
        Self::custom(
            "team_task_completed".into(),
            json!({
                "goal_id": goal_id.into(),
                "task_id": task_id.into(),
                "agent_id": agent_id.into(),
                "success": success,
                "duration_ms": duration_ms,
                "error": error,
            }),
        )
    }

    /// Team 事件：并行冲突检测（M1.3 对外一致暴露）。
    pub fn team_conflict_detected(
        goal_id: impl Into<String>,
        kind: impl Into<String>,
        key: impl Into<String>,
        task_ids: Vec<String>,
    ) -> Self {
        Self::custom(
            "team_conflict_detected".into(),
            json!({
                "goal_id": goal_id.into(),
                "kind": kind.into(),
                "key": key.into(),
                "task_ids": task_ids,
            }),
        )
    }

    /// Team 事件：工作区变更影响评估（M1.3 对外一致暴露）。
    pub fn team_workspace_impact(
        goal_id: impl Into<String>,
        batch_index: usize,
        changed_paths: Vec<String>,
        affected_task_ids: Vec<String>,
    ) -> Self {
        Self::custom(
            "team_workspace_impact".into(),
            json!({
                "goal_id": goal_id.into(),
                "batch_index": batch_index,
                "changed_paths": changed_paths,
                "affected_task_ids": affected_task_ids,
            }),
        )
    }

    /// Team 事件：综合/汇总阶段（M1.3 对外一致暴露）。
    pub fn team_synthesizing(goal_id: impl Into<String>, completed: usize, failed: usize) -> Self {
        Self::custom(
            "team_synthesizing".into(),
            json!({
                "goal_id": goal_id.into(),
                "completed": completed,
                "failed": failed,
            }),
        )
    }

    /// Team 事件：执行完成（M1.3 对外一致暴露，含冲突统计与影响记录摘要）。
    pub fn team_complete(goal_id: impl Into<String>, report: serde_json::Value) -> Self {
        Self::custom(
            "team_complete".into(),
            json!({
                "goal_id": goal_id.into(),
                "report": report,
            }),
        )
    }

    /// 返回 `Custom` 事件的整个 payload（测试用）
    ///
    /// 非 `Custom` 变体返回 `Null`：治理规则禁止 panic，
    /// 测试辅助也不应例外。
    #[cfg(test)]
    pub(crate) fn payload_field_raw(&self) -> serde_json::Value {
        match self {
            SseEvent::Custom { payload, .. } => payload.clone(),
            _ => serde_json::Value::Null,
        }
    }

    /// 读取 `Custom` 事件的单个 payload 字段（测试用）
    ///
    /// `sop_id()` / `session_id()` 只暴露维度，不便于断言其他业务字段
    /// 在维度注入后仍然完好。
    #[cfg(test)]
    pub(crate) fn payload_field(&self, key: &str) -> Option<serde_json::Value> {
        match self {
            SseEvent::Custom { payload, .. } => payload.get(key).cloned(),
            _ => None,
        }
    }

    /// 转换为 SSE 格式字符串
    pub fn to_sse_string(&self) -> String {
        let data = serde_json::to_string(self).unwrap_or_default();
        format!("event: {}\ndata: {}\n\n", self.event_type(), data)
    }

    /// 将团队编排事件转换为可广播的 SSE 事件（M1.3 对外一致暴露/桥接辅助）。
    ///
    /// team 事件在工具层定义（`sisyphus_tools_agent::team::TeamEvent`），bridge 侧通过
    /// `Custom` 事件承载以保持 `SseEvent` 枚举稳定。`goal_id` 由调用方提供（部分 team 事件
    /// 本身不携带 goal_id）。
    pub fn from_team_event(
        event: &sisyphus_tools_agent::team::TeamEvent,
        goal_id: Option<&str>,
    ) -> Self {
        let goal = goal_id.unwrap_or_default().to_string();
        match event {
            sisyphus_tools_agent::team::TeamEvent::Planning { goal_id, objective } => Self::custom(
                "team_planning".into(),
                json!({ "goal_id": goal_id, "objective": objective }),
            ),
            sisyphus_tools_agent::team::TeamEvent::PlanReady { task_count } => Self::custom(
                "team_plan_ready".into(),
                json!({ "goal_id": goal, "task_count": task_count }),
            ),
            sisyphus_tools_agent::team::TeamEvent::TaskStarted {
                task_id,
                agent_id,
                tool_id,
            } => Self::team_task_started(goal, task_id.clone(), agent_id.clone(), tool_id.clone()),
            sisyphus_tools_agent::team::TeamEvent::TaskCompleted(result) => {
                Self::team_task_completed(
                    if goal.is_empty() {
                        // 无 goal_id 时仍保留任务信息
                        String::new()
                    } else {
                        goal
                    },
                    result.task_id.clone(),
                    result.agent_id.clone(),
                    result.success,
                    result.duration_ms,
                    result.error.clone(),
                )
            },
            sisyphus_tools_agent::team::TeamEvent::ConflictDetected {
                kind,
                key,
                task_ids,
            } => Self::team_conflict_detected(goal, kind.clone(), key.clone(), task_ids.clone()),
            sisyphus_tools_agent::team::TeamEvent::WorkspaceImpact {
                batch_index,
                changed_paths,
                affected_task_ids,
            } => Self::team_workspace_impact(
                goal,
                *batch_index,
                changed_paths.clone(),
                affected_task_ids.clone(),
            ),
            sisyphus_tools_agent::team::TeamEvent::ImpactRetry { round, task_ids } => Self::custom(
                "team_impact_retry".into(),
                json!({ "goal_id": goal, "round": round, "task_ids": task_ids }),
            ),
            sisyphus_tools_agent::team::TeamEvent::Synthesizing { completed, failed } => {
                Self::team_synthesizing(goal, *completed, *failed)
            },
            sisyphus_tools_agent::team::TeamEvent::Reflecting { iteration } => Self::custom(
                "team_reflecting".into(),
                json!({ "goal_id": goal, "iteration": iteration }),
            ),
            sisyphus_tools_agent::team::TeamEvent::Replanning { iteration, reason } => {
                Self::custom(
                    "team_replanning".into(),
                    json!({ "goal_id": goal, "iteration": iteration, "reason": reason }),
                )
            },
            sisyphus_tools_agent::team::TeamEvent::Complete(report) => {
                let goal_id = if report.goal_id.is_empty() {
                    goal
                } else {
                    report.goal_id.clone()
                };
                Self::team_complete(
                    goal_id,
                    serde_json::to_value(report).unwrap_or_else(|_| json!({})),
                )
            },
            sisyphus_tools_agent::team::TeamEvent::Error { message } => Self::custom(
                "team_error".into(),
                json!({ "goal_id": goal, "message": message }),
            ),
        }
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
    /// 同 [`SseEvent::from_execution_event`]，额外把 trace id 作为维度键注入 payload。
    ///
    /// T4.4：让订阅方能把事件与发起它的 HTTP/gRPC 请求串联（`trace_id` 键）。
    pub fn from_execution_event_with_trace(
        event: &sisyphus_types::ExecutionEvent,
        sop_id: &str,
        trace_id: Option<&str>,
    ) -> Self {
        let mut sse = Self::from_execution_event(event, sop_id);
        if let (SseEvent::Custom { payload, .. }, Some(trace_id)) = (&mut sse, trace_id) {
            if let Some(map) = payload.as_object_mut() {
                map.insert(
                    "trace_id".to_string(),
                    serde_json::Value::String(trace_id.to_string()),
                );
            }
        }
        sse
    }

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
            timestamp: jiff::Timestamp::now(),
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
        E::ToolCall { .. } => "tool_call",
        E::TokenUsage { .. } => "token_usage",
        E::ExecutionComplete { .. } => "execution_complete",
        E::CognitivePlan { .. } => "cognitive_plan",
        E::CognitiveStep { .. } => "cognitive_step",
        E::MicroReflection { .. } => "micro_reflection",
        E::CognitiveComplete { .. } => "cognitive_complete",
    }
    .to_string()
}

/// SSE 连接查询参数
///
/// 所有字段均已生效：`events` / `sop_id` / `session_id` / `project_id` /
/// `goal_id` 参与事件过滤；`last_event_id` 与 `Last-Event-ID` header 一起
/// 用于断线重连的事件重放（header 优先）；`heartbeat` 控制心跳间隔。
#[derive(Debug, Deserialize)]
pub struct SseQuery {
    /// 按事件类型过滤（逗号分隔）。**已生效**
    pub events: Option<String>,
    /// 按项目 ID 过滤。**已生效**（匹配 payload 携带 `project_id` 的 Custom 事件
    /// —— `POST /api/events/broadcast` 会把请求顶层的 `project_id` 注入事件）
    pub project_id: Option<String>,
    /// 按目标 ID 过滤。**已生效**（匹配认知迭代/SOP 执行事件，以及 payload 携带
    /// `goal_id` 的 Custom 事件 —— 含 team 执行转发器注入的事件）
    pub goal_id: Option<String>,
    /// 按 SOP ID 过滤。**已生效**（匹配节点/SOP 执行类事件，以及 payload 携带
    /// `sop_id` 的 Custom 事件 —— 含广播端点与执行转发器注入的事件）
    pub sop_id: Option<String>,
    /// 按会话 ID 过滤。**已生效**（`session_message_added` 事件，或 payload 携带
    /// `session_id` 的 Custom 事件）
    pub session_id: Option<String>,
    /// 断点续传的最后事件 ID。**已生效**（与 `Last-Event-ID` header 等价，
    /// header 优先）。值必须是发布侧分配的数字序号；历史客户端传来的 UUID
    /// 解析失败按「未声明断点」处理（只接收实时事件）。
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

/// 解析断线重连游标：`Last-Event-ID` header 优先于 `?last_event_id=`。
///
/// 只接受发布侧分配的数字序号：
/// - 旧版本客户端可能回传随机 UUID —— 解析失败按「未声明断点」处理，
///   连接从实时流开始（与旧行为一致，不会 400）；
/// - `0` 是合法游标（表示「补发缓冲内全部事件」），有效序号从 1 开始。
fn resolve_last_event_id(headers: &HeaderMap, query: Option<&String>) -> Option<u64> {
    let raw = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| query.map(|s| s.trim().to_string()))?;
    raw.parse::<u64>().ok()
}

/// 把 `{ id, event }` 渲染为 SSE 帧：`id:` 字段供客户端断线重连时回传。
fn render_sse_event(id: u64, event: &SseEvent) -> axum::response::sse::Event {
    axum::response::sse::Event::default()
        .event(event.event_type())
        .data(serde_json::to_string(event).unwrap_or_default())
        .id(id.to_string())
}

/// 订阅方的过滤条件
///
/// 持有拥有所有权的 `String` 而非借用：过滤条件要被 `async_stream::stream!`
/// 生成的流捕获，而该流的生命周期长于本函数的局部变量。借用会导致
/// “captured variable cannot escape” 类错误。
#[derive(Debug, Clone, Default)]
pub(crate) struct SseFilters {
    /// 按事件类型过滤（逗号分隔）
    pub types: Option<Vec<String>>,
    /// 按 SOP ID 过滤
    pub sop_id: Option<String>,
    /// 按会话 ID 过滤
    pub session_id: Option<String>,
    /// 按项目 ID 过滤
    pub project_id: Option<String>,
    /// 按目标 ID 过滤
    pub goal_id: Option<String>,
}

/// 判断事件是否应推送给当前订阅者
///
/// 各维度为「与」（AND）关系，全部满足才推送：
/// - `types` 非空时，事件类型必须命中其中之一；
/// - 其余维度要求事件**携带**该维度且值相等（维度缺失即不匹配）。
///
/// 维度缺失即不匹配是刻意的：宁可少推，也不要把无归属的事件泄露给
/// 按项目/会话隔离的订阅者。
///
/// 抽为独立纯函数以便在不启动 HTTP 服务的情况下直接验证过滤语义。
///
/// `pub(crate)` 而非私有：`handlers.rs` 的转发器测试需要用同一套过滤语义
/// 验证「转发出来的事件确实能被 `?sop_id=` 识别」，避免测试里复制一份
/// 过滤逻辑导致验证失真。
pub(crate) fn matches_filters(event: &SseEvent, f: &SseFilters) -> bool {
    if let Some(filters) = &f.types {
        if !filters.iter().any(|t| t.as_str() == event.event_type()) {
            return false;
        }
    }
    if let Some(want) = &f.sop_id {
        if event.sop_id() != Some(want.as_str()) {
            return false;
        }
    }
    if let Some(want) = &f.session_id {
        if event.session_id() != Some(want.as_str()) {
            return false;
        }
    }
    if let Some(want) = &f.project_id {
        if event.project_id() != Some(want.as_str()) {
            return false;
        }
    }
    if let Some(want) = &f.goal_id {
        if event.goal_id() != Some(want.as_str()) {
            return false;
        }
    }
    true
}

/// SSE 流处理器
///
/// 断线重连：客户端把最后收到的 `id:` 通过 `Last-Event-ID` header（或
/// `?last_event_id=`）带回，服务端先补发重放缓冲中该 ID 之后的事件，再
/// 接续实时流。补发与实时流在边界处可能重叠（订阅先于快照），通过
/// `replayed_through` 游标去重，保证每个 ID 只发送一次。
pub async fn sse_handler(
    State(state): State<RestApiState>,
    headers: HeaderMap,
    Query(query): Query<SseQuery>,
) -> Response {
    // 并发连接数限制：超过上限直接返回 429，避免无限连接耗尽资源
    let permit = match state.sse_connection_limit.clone().try_acquire_owned() {
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

    // 断线重连游标：先订阅、后取重放快照。两者之间的窗口会同时出现在
    // 重放列表与实时流里，由流内的 replayed_through 游标去重。
    let requested_last_id = resolve_last_event_id(&headers, query.last_event_id.as_ref());
    // 服务重启后序号归零：客户端携带的 ID 若大于当前最新 ID，说明它来自
    // 上一个进程，历史不可用 —— 退回实时流而不是永久跳过新事件。
    let resume_from =
        requested_last_id.filter(|last_id| *last_id <= state.sse_broadcaster.latest_id());
    let replayed = state.sse_broadcaster.replay_since(resume_from);

    // 解析事件过滤器
    let event_filters: Option<Vec<String>> =
        query.events.as_ref().map(|s| s.split(',').map(|s| s.trim().to_string()).collect());

    // 按维度过滤：事件必须携带该维度且值匹配才推送。
    // 维度缺失的事件（如 skill_installed 没有 sop_id）在此维度下被排除。
    let sop_filter: Option<String> = normalize_dimension(query.sop_id.as_ref());
    let session_filter: Option<String> = normalize_dimension(query.session_id.as_ref());
    let project_filter: Option<String> = normalize_dimension(query.project_id.as_ref());
    let goal_filter: Option<String> = normalize_dimension(query.goal_id.as_ref());
    let filters = SseFilters {
        types: event_filters,
        sop_id: sop_filter,
        session_id: session_filter,
        project_id: project_filter,
        goal_id: goal_filter,
    };

    let heartbeat_interval = query.heartbeat.unwrap_or(30);

    // 从广播接收器创建事件流
    let stream = async_stream::stream! {
        // P0 ⑥：permit 随流生命周期持有——作为 handler 局部变量时会在返回
        // Response 后即释放，连接上限形同虚设；移入流内直至连接关闭才归还。
        let _permit = permit;
        let mut receiver = receiver;
        let mut last_heartbeat = std::time::Instant::now();

        // ── 断线重连补发 ─────────────────────────────────────────────
        // 先补发快照，再接续实时流；replayed_through 记录已发送的最大 ID，
        // 实时侧跳过 <= 它的重复事件（订阅与快照之间发布的事件会同时出现在
        // 两个来源）。
        let mut replayed_through = resume_from.unwrap_or(0);
        for published in replayed {
            replayed_through = replayed_through.max(published.id);
            if !matches_filters(&published.event, &filters) {
                continue;
            }
            yield Ok::<axum::response::sse::Event, Infallible>(
                render_sse_event(published.id, &published.event),
            );
        }

        loop {
            // 检查是否该发送心跳
            let now = std::time::Instant::now();
            if heartbeat_interval > 0 && now.duration_since(last_heartbeat) >= Duration::from_secs(heartbeat_interval) {
                last_heartbeat = now;
                yield Ok::<axum::response::sse::Event, Infallible>(
                    axum::response::sse::Event::default()
                        .event("heartbeat")
                        // Wire format 说明：字段类型为 `jiff::Timestamp`，serde 经 Display
                        // 序列化为 RFC 3339 UTC（`...Z`）。与迁移前 chrono `AutoSi` 的
                        // Display 不同，**不会**保留尾零（如 `.780` → `.78`），但仍是合法
                        // RFC 3339 —— 下游 SSE consumer 用标准解析器即可兼容。
                        .data(json!({"timestamp": jiff::Timestamp::now().to_string()}).to_string()),
                );
            }

            // 尝试接收事件（带超时）
            match tokio::time::timeout(
                Duration::from_millis(100),
                receiver.recv()
            ).await {
                Ok(Ok(published)) => {
                    // 边界去重：重放快照已发送过的 ID 不再重复推送。
                    if published.id <= replayed_through {
                        continue;
                    }
                    replayed_through = published.id;

                    // 应用过滤器（类型 / sop_id / session_id / project_id / goal_id 取交集）
                    if !matches_filters(&published.event, &filters) {
                        continue;
                    }

                    yield Ok::<axum::response::sse::Event, Infallible>(
                        render_sse_event(published.id, &published.event),
                    );
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
    maybe_trace_id: Option<Extension<sisyphus_types::trace::TraceId>>,
    Json(payload): Json<BroadcastEventRequest>,
) -> impl IntoResponse {
    // T4.4：广播事件的 trace id 来自请求头（入口中间件注入），
    // 与执行事件同构 —— 订阅方据此把事件与发起请求串联。
    let trace_id = maybe_trace_id.map(|ext| ext.0.as_str().to_string());
    let event = SseEvent::Custom {
        event_type: payload.event_type.clone(),
        payload: inject_broadcast_dimensions(
            payload.payload.clone(),
            &payload,
            trace_id.as_deref(),
        ),
        timestamp: jiff::Timestamp::now(),
    };

    // 事件始终写入重放缓冲（即使当前无订阅者），断线重连的客户端仍能补收。
    // 但「广播时无人订阅」沿用既有契约：返回 500 提示调用方。
    let outcome = state.sse_broadcaster.publish(event);
    if outcome.receivers > 0 {
        (
            StatusCode::OK,
            Json(json!({"status": "broadcasted", "event_id": outcome.id})),
        )
            .into_response()
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "No subscribers", "event_id": outcome.id})),
        )
            .into_response()
    }
}

/// 团队事件转发到 SSE 时补齐的归属维度（M1.3）。
///
/// `goal_id` 同时用于 `from_team_event` 补齐事件缺失的目标维度；
/// `project_id` / `trace_id` 会注入 Custom payload，使 `/api/events`
/// 的 `?project_id=` / `?goal_id=` 过滤与请求追踪可用。
#[derive(Debug, Clone, Default)]
pub struct TeamSseScope {
    /// 团队执行目标 ID（事件归属/过滤维度）。
    pub goal_id: Option<String>,
    /// 项目归属（`?project_id=` 过滤维度）。
    pub project_id: Option<String>,
    /// 请求级 trace id（串联入口请求与事件流）。
    pub trace_id: Option<String>,
}

/// 将团队事件流转发到 SSE 广播器（M1.3 广播接线）。
///
/// 将 `TeamEvent` 转换为统一 `SseEvent::Custom(team_*)` 并广播给
/// `/api/events` 订阅者，按 `scope` 补齐 `goal_id`/`project_id`/`trace_id`
/// 维度。接收端关闭（执行结束）后自行退出。
pub async fn forward_team_events_to_sse(
    mut rx: tokio::sync::mpsc::Receiver<sisyphus_tools_agent::team::TeamEvent>,
    publisher: crate::http::sse_replay::SsePublisher,
    scope: TeamSseScope,
) {
    while let Some(event) = rx.recv().await {
        let sse = SseEvent::from_team_event(&event, scope.goal_id.as_deref());
        // publish 会分配全局序号并写入重放缓冲：team 事件因此可在断线后
        // 按 `Last-Event-ID` 补发（窗口受缓冲容量限制）。
        let _ = publisher.publish(inject_team_scope(sse, &scope));
    }
}

/// 把团队执行的归属维度写入 Custom payload（不改动事件类型/业务字段）。
fn inject_team_scope(mut sse: SseEvent, scope: &TeamSseScope) -> SseEvent {
    if let SseEvent::Custom { payload, .. } = &mut sse {
        if let Some(map) = payload.as_object_mut() {
            if let Some(project_id) = scope.project_id.as_deref() {
                map.insert(
                    "project_id".to_string(),
                    serde_json::Value::String(project_id.to_string()),
                );
            }
            if let Some(trace_id) = scope.trace_id.as_deref() {
                map.insert(
                    "trace_id".to_string(),
                    serde_json::Value::String(trace_id.to_string()),
                );
            }
        }
    }
    sse
}

/// 把请求顶层的归属维度写入事件 payload
///
/// 顶层 `sop_id` / `session_id` / `project_id` 是 API 契约里专门的归属声明，
/// 而 `SseEvent::sop_id()` 等对 `Custom` 变体正是从 payload 读取维度。旧实现
/// 只透传 `payload`、丢弃全部顶层字段，于是调用方按契约在顶层传 `sop_id`，
/// 事件却因「无维度」被 `?sop_id=` 服务端过滤永久丢弃 —— 广播返回 200，
/// 看起来成功，实际没有订阅者能收到。
///
/// 优先级：顶层 > payload。顶层字段是专门的归属声明，payload 只是业务载荷；
/// 顶层缺省（或为空白字符串，与订阅侧 `normalize_dimension` 归一化一致）时
/// 不写入，payload 里已有的同名维度原样保留。
///
/// payload 不是对象（如 `null`、数组、标量）时无法注入维度，直接原样返回：
/// 维度缺失只会让过滤条件不匹配，不会破坏事件本身。
fn inject_broadcast_dimensions(
    payload: serde_json::Value,
    req: &BroadcastEventRequest,
    trace_id: Option<&str>,
) -> serde_json::Value {
    let mut map = match payload {
        serde_json::Value::Object(m) => m,
        other => return other,
    };

    // trace id 优先取请求头（请求级归属），payload 内同名字段让位。
    if let Some(trace_id) = trace_id.map(str::trim).filter(|s| !s.is_empty()) {
        map.insert(
            "trace_id".to_string(),
            serde_json::Value::String(trace_id.to_string()),
        );
    }

    for (key, value) in [
        ("sop_id", &req.sop_id),
        ("session_id", &req.session_id),
        ("project_id", &req.project_id),
    ] {
        // 与订阅侧 `?sop_id=` 的空值归一化保持一致：空白不是有效归属声明，
        // 写入它会抹掉 payload 里本已正确的维度。
        if let Some(v) = value.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            map.insert(key.to_string(), serde_json::Value::String(v.to_string()));
        }
    }
    serde_json::Value::Object(map)
}

/// 广播事件请求
#[derive(Debug, Deserialize)]
// 字段豁免说明：`event_type` / `payload` 已被 broadcast_event_handler 使用；
//   `project_id` / `sop_id` / `session_id` 经 inject_broadcast_dimensions
//   写入事件 payload，参与 `?project_id=` / `?sop_id=` / `?session_id=`
//   三维服务端过滤。
#[allow(dead_code)]
pub struct BroadcastEventRequest {
    /// 事件类型。**已生效**
    pub event_type: String,
    /// 事件载荷。**已生效**
    pub payload: serde_json::Value,
    /// 项目 ID。**已生效**：写入事件 payload，参与 `?project_id=` 过滤
    pub project_id: Option<String>,
    /// SOP ID。**已生效**：写入事件 payload，参与 `?sop_id=` 过滤
    pub sop_id: Option<String>,
    /// 会话 ID。**已生效**：写入事件 payload，参与 `?session_id=` 过滤
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
    let stats = state.http_stats.as_ref().map(|f| f()).unwrap_or_default();

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

    // ── 认知循环 / 执行器子系统 ────────────────────────────────────
    // 这些量此前只能通过 /health/deep 的 JSON 或 /api/anomaly 查询，
    // 无法被 Prometheus 抓取。
    let kernel = state.bridge.kernel();
    let anomaly_entries = kernel.strategic_manager.anomaly_log.len();
    let budget = {
        // 立刻释放读锁，避免守卫跨越后续代码。
        let guard = kernel.strategic_manager.token_budget.read();
        guard.snapshot()
    };
    let (exec_limit, exec_available) = kernel
        .execution_manager
        .get_executor()
        .map(|ex| {
            (
                ex.configured_limit(),
                ex.concurrency_limit.available_permits(),
            )
        })
        .unwrap_or((0, 0));

    let _ = writeln!(
        out,
        "# HELP sisyphus_anomaly_log_entries Anomalies currently retained in the bounded anomaly log"
    );
    let _ = writeln!(out, "# TYPE sisyphus_anomaly_log_entries gauge");
    let _ = writeln!(out, "sisyphus_anomaly_log_entries {}", anomaly_entries);
    let _ = writeln!(
        out,
        "# HELP sisyphus_token_budget_usage Estimated tokens consumed by LLM stages"
    );
    let _ = writeln!(out, "# TYPE sisyphus_token_budget_usage gauge");
    let _ = writeln!(out, "sisyphus_token_budget_usage {}", budget.usage);
    let _ = writeln!(
        out,
        "# HELP sisyphus_token_budget_capped Whether a token budget cap is configured (1) or unbounded (0)"
    );
    let _ = writeln!(out, "# TYPE sisyphus_token_budget_capped gauge");
    let _ = writeln!(
        out,
        "sisyphus_token_budget_capped {}",
        if budget.cap.is_some() { 1 } else { 0 }
    );
    let _ = writeln!(
        out,
        "# HELP sisyphus_token_budget_exhausted Whether remaining budget is below the reserve threshold (1) or not (0)"
    );
    let _ = writeln!(out, "# TYPE sisyphus_token_budget_exhausted gauge");
    let _ = writeln!(
        out,
        "sisyphus_token_budget_exhausted {}",
        if budget.exhausted { 1 } else { 0 }
    );
    if let Some(cap) = budget.cap {
        let _ = writeln!(
            out,
            "# HELP sisyphus_token_budget_remaining Remaining tokens before the configured cap"
        );
        let _ = writeln!(out, "# TYPE sisyphus_token_budget_remaining gauge");
        let _ = writeln!(
            out,
            "sisyphus_token_budget_remaining {}",
            cap.saturating_sub(budget.usage)
        );
    }
    let _ = writeln!(
        out,
        "# HELP sisyphus_executor_concurrency_limit Configured node concurrency limit"
    );
    let _ = writeln!(out, "# TYPE sisyphus_executor_concurrency_limit gauge");
    let _ = writeln!(out, "sisyphus_executor_concurrency_limit {}", exec_limit);
    let _ = writeln!(
        out,
        "# HELP sisyphus_executor_available_permits Idle node slots in the executor semaphore"
    );
    let _ = writeln!(out, "# TYPE sisyphus_executor_available_permits gauge");
    let _ = writeln!(
        out,
        "sisyphus_executor_available_permits {}",
        exec_available
    );

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
///
/// 附带重放缓冲指标：`buffered`/`capacity` 反映断线窗口占用，
/// `latest_id` 是当前最新全局序号（客户端可据此判断断点是否已被淘汰）。
pub async fn sse_health_handler(State(state): State<RestApiState>) -> impl IntoResponse {
    let replay = state.sse_broadcaster.replay();
    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "service": "sse",
            "replay": {
                "buffered": replay.len(),
                "capacity": replay.capacity(),
                "latest_id": replay.latest_id(),
            }
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    // ── broadcast_event_handler 维度透传 ────────────────────────────
    //
    // 缺陷：handler 只透传 payload，把 BroadcastEventRequest 顶层的
    // sop_id / session_id / project_id 全丢了。而 SseEvent::sop_id()
    // 对 Custom 变体正是从 payload 读维度，于是调用方按 API 契约在
    // 顶层传 sop_id，事件却因「无维度」被 ?sop_id= 过滤永久丢弃。

    /// 顶层维度必须落到事件上，且能被服务端过滤识别。
    /// 走真实 handler，不复制逻辑。
    #[tokio::test]
    async fn broadcast_top_level_dimensions_reach_subscribers() {
        let state = make_test_state(None).await;
        let mut rx = state.sse_broadcaster.subscribe();

        let req = BroadcastEventRequest {
            event_type: "custom_event".to_string(),
            payload: serde_json::json!({"message": "hi"}),
            project_id: Some("proj-a".to_string()),
            sop_id: Some("sop-top".to_string()),
            session_id: Some("sess-top".to_string()),
        };
        let resp = broadcast_event_handler(State(state), None, Json(req)).await.into_response();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 4096).await.expect("读取响应体");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("JSON 响应");
        assert_eq!(
            json["event_id"], 1,
            "响应必须回传发布序号，客户端可用作 Last-Event-ID: {json}"
        );

        let ev = rx.recv().await.expect("订阅者应收到广播事件");
        assert_eq!(
            ev.id,
            json["event_id"].as_u64().expect("数字 ID"),
            "广播事件与响应 ID 必须一致"
        );
        assert_eq!(ev.event_type(), "custom_event");
        assert_eq!(
            ev.sop_id(),
            Some("sop-top"),
            "顶层 sop_id 必须成为事件维度，否则 ?sop_id=sop-top 订阅者收不到"
        );
        assert_eq!(ev.session_id(), Some("sess-top"));
        // 业务字段不得被维度注入覆盖
        assert_eq!(ev.payload_field("message"), Some(serde_json::json!("hi")));
    }

    /// payload 里的维度与顶层冲突时，顶层是权威的
    /// （顶层字段是 API 契约里专门的归属声明，payload 只是业务载荷）。
    #[tokio::test]
    async fn top_level_dimensions_override_payload() {
        let state = make_test_state(None).await;
        let mut rx = state.sse_broadcaster.subscribe();

        let req = BroadcastEventRequest {
            event_type: "custom_event".to_string(),
            payload: serde_json::json!({"sop_id": "from-payload", "session_id": "sess-payload"}),
            project_id: None,
            sop_id: Some("sop-top".to_string()),
            session_id: Some("sess-top".to_string()),
        };
        let resp = broadcast_event_handler(State(state), None, Json(req)).await.into_response();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);

        let ev = rx.recv().await.expect("订阅者应收到广播事件");
        assert_eq!(ev.sop_id(), Some("sop-top"));
        assert_eq!(ev.session_id(), Some("sess-top"));
    }

    /// 顶层未提供维度时，payload 内的维度必须原样保留。
    #[tokio::test]
    async fn payload_dimensions_survive_when_top_level_absent() {
        let state = make_test_state(None).await;
        let mut rx = state.sse_broadcaster.subscribe();

        let req = BroadcastEventRequest {
            event_type: "custom_event".to_string(),
            payload: serde_json::json!({"sop_id": "sop-payload"}),
            project_id: None,
            sop_id: None,
            session_id: None,
        };
        let resp = broadcast_event_handler(State(state), None, Json(req)).await.into_response();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);

        let ev = rx.recv().await.expect("订阅者应收到广播事件");
        assert_eq!(
            ev.sop_id(),
            Some("sop-payload"),
            "顶层缺省时应回退到 payload 的维度，不能被 None 抹掉"
        );
    }

    /// 端到端：顶层 sop_id 广播的事件必须能通过 ?sop_id= 过滤。
    /// 只测维度提取不够 —— 还要证明 matches_filters 对它返回 true。
    #[tokio::test]
    async fn top_level_sop_id_is_filterable_end_to_end() {
        let state = make_test_state(None).await;
        let mut rx = state.sse_broadcaster.subscribe();

        let req = BroadcastEventRequest {
            event_type: "custom_event".to_string(),
            payload: serde_json::json!({}),
            project_id: None,
            sop_id: Some("sop-e2e".to_string()),
            session_id: None,
        };
        let _ = broadcast_event_handler(State(state), None, Json(req)).await;
        let ev = rx.recv().await.expect("订阅者应收到广播事件");

        assert!(
            matches_filters(
                &ev,
                &SseFilters {
                    sop_id: Some("sop-e2e".to_string()),
                    ..Default::default()
                }
            ),
            "?sop_id=sop-e2e 的订阅者必须能收到顶层声明该 sop_id 的事件"
        );
        assert!(
            !matches_filters(
                &ev,
                &SseFilters {
                    sop_id: Some("other-sop".to_string()),
                    ..Default::default()
                }
            ),
            "不得匹配无关 sop_id"
        );
    }

    /// payload 不是对象时不能注入维度，但也不能因此损坏事件。
    #[tokio::test]
    async fn non_object_payload_is_broadcast_unchanged() {
        let state = make_test_state(None).await;
        let mut rx = state.sse_broadcaster.subscribe();

        for payload in [
            serde_json::Value::Null,
            serde_json::json!([1, 2, 3]),
            serde_json::json!("plain"),
            serde_json::json!(7),
        ] {
            let req = BroadcastEventRequest {
                event_type: "custom_event".to_string(),
                payload: payload.clone(),
                project_id: None,
                sop_id: Some("sop-x".to_string()),
                session_id: None,
            };
            let resp = broadcast_event_handler(State(state.clone()), None, Json(req))
                .await
                .into_response();
            assert_eq!(resp.status(), axum::http::StatusCode::OK, "{payload}");

            let ev = rx.recv().await.expect("订阅者应收到广播事件");
            assert_eq!(
                ev.payload_field_raw(),
                payload,
                "非对象 payload 必须原样送达，不能被维度注入破坏"
            );
            assert_eq!(ev.sop_id(), None);
        }
    }

    /// 顶层空串视为「未声明」，不得抹掉 payload 里已有的维度。
    /// 与 SSE 订阅侧 `normalize_dimension` 的空值归一化保持一致。
    #[tokio::test]
    async fn blank_top_level_dimension_does_not_erase_payload_dimension() {
        let state = make_test_state(None).await;
        let mut rx = state.sse_broadcaster.subscribe();

        let req = BroadcastEventRequest {
            event_type: "custom_event".to_string(),
            payload: serde_json::json!({"sop_id": "sop-payload"}),
            project_id: Some("   ".to_string()),
            sop_id: Some("  ".to_string()),
            session_id: None,
        };
        let resp = broadcast_event_handler(State(state), None, Json(req)).await.into_response();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);

        let ev = rx.recv().await.expect("订阅者应收到广播事件");
        assert_eq!(
            ev.sop_id(),
            Some("sop-payload"),
            "顶层空串不是有效归属声明，不应覆盖 payload 的维度"
        );
        assert_eq!(ev.project_id(), None);
    }

    #[test]
    fn test_sse_event_serialization() {
        let event = SseEvent::CognitiveIterationStarted {
            iteration: 1,
            goal_id: "goal1".to_string(),
            timestamp: jiff::Timestamp::now(),
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
            timestamp: jiff::Timestamp::now(),
        };

        assert_eq!(event.event_type(), "cognitive_iteration_started");
    }

    #[test]
    fn test_sse_event_to_string() {
        let event = SseEvent::Custom {
            event_type: "test_event".to_string(),
            payload: json!({"key": "value"}),
            timestamp: jiff::Timestamp::now(),
        };

        let sse_str = event.to_sse_string();
        assert!(sse_str.contains("event: test_event"));
        assert!(sse_str.contains("data:"));
    }

    #[test]
    fn test_every_execution_event_variant_maps_to_sse() {
        use sisyphus_types::ExecutionEvent as E;

        // 覆盖全部 10 个变体：任何新增变体若忘记映射，本测试的
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
                E::ToolCall {
                    tool_id: "builtin:file_read".to_string(),
                    phase: "completed".to_string(),
                    duration_ms: Some(12),
                    success: Some(true),
                    error: None,
                },
                "tool_call",
            ),
            (
                E::TokenUsage {
                    model: Some("gpt-probe".to_string()),
                    prompt_tokens: 10,
                    completion_tokens: 20,
                    total_tokens: 30,
                },
                "token_usage",
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
            10,
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
    fn test_broadcast_payload_carries_trace_id() {
        // T4.4：广播事件的 trace id 来自请求头（入口中间件注入），
        // 与执行事件同构；payload 内同名字段让位于请求级归属。
        let req = BroadcastEventRequest {
            event_type: "custom".into(),
            payload: serde_json::json!({"message": "hi"}),
            project_id: None,
            sop_id: None,
            session_id: None,
        };
        let payload = inject_broadcast_dimensions(req.payload.clone(), &req, Some("trace-abc"));
        assert_eq!(payload["trace_id"], "trace-abc");

        // 无 trace id 时不注入该键（保持旧行为）
        let payload = inject_broadcast_dimensions(req.payload.clone(), &req, None);
        assert!(payload.get("trace_id").is_none());
    }

    #[test]
    fn test_mapped_execution_events_carry_trace_id() {
        // T4.4：事件负载携带 trace id，便于与发起请求串联
        let event = sisyphus_types::ExecutionEvent::NodeStarted {
            node_name: "fetch".to_string(),
        };
        let sse = SseEvent::from_execution_event_with_trace(&event, "sop-abc", Some("trace-abc"));
        assert_eq!(
            sse.payload_field("trace_id"),
            Some(serde_json::json!("trace-abc"))
        );
        assert_eq!(
            sse.payload_field("sop_id"),
            Some(serde_json::json!("sop-abc"))
        );

        // 未提供 trace id 时不注入该键（保持旧行为兼容）
        let plain = SseEvent::from_execution_event_with_trace(&event, "sop-abc", None);
        assert_eq!(plain.payload_field("trace_id"), None);
    }

    #[test]
    fn test_mapped_execution_events_pass_sop_filter() {
        // 端到端：内核事件经映射后必须能通过自己所属 sop 的过滤
        let event = sisyphus_types::ExecutionEvent::NodeStarted {
            node_name: "fetch".to_string(),
        };
        let sse = SseEvent::from_execution_event(&event, "sop-abc");

        assert!(matches_filters(
            &sse,
            &SseFilters {
                sop_id: Some("sop-abc".to_string()),
                ..Default::default()
            }
        ));
        assert!(!matches_filters(
            &sse,
            &SseFilters {
                sop_id: Some("sop-other".to_string()),
                ..Default::default()
            }
        ));
        // 也能被类型过滤命中
        assert!(matches_filters(
            &sse,
            &SseFilters {
                types: Some(vec!["node_started".to_string()]),
                ..Default::default()
            }
        ));
        assert!(!matches_filters(
            &sse,
            &SseFilters {
                types: Some(vec!["node_completed".to_string()]),
                ..Default::default()
            }
        ));
    }

    #[test]
    fn test_team_events_pass_goal_filter() {
        // M1.3：team 事件必须携带 goal_id 维度，并能被 ?goal_id= 精确过滤。
        let event = sisyphus_tools_agent::team::TeamEvent::TaskStarted {
            task_id: "t1".to_string(),
            agent_id: "metis".to_string(),
            tool_id: "metis/analyse".to_string(),
        };
        let sse = SseEvent::from_team_event(&event, Some("goal-abc"));
        assert_eq!(sse.event_type(), "team_task_started");
        assert_eq!(sse.goal_id(), Some("goal-abc"));
        assert!(matches_filters(
            &sse,
            &SseFilters {
                goal_id: Some("goal-abc".to_string()),
                ..Default::default()
            }
        ));
        assert!(!matches_filters(
            &sse,
            &SseFilters {
                goal_id: Some("goal-other".to_string()),
                ..Default::default()
            }
        ));
    }

    #[test]
    fn test_cognitive_iteration_events_pass_goal_filter() {
        let sse = SseEvent::CognitiveIterationStarted {
            iteration: 1,
            goal_id: "goal-cog".to_string(),
            timestamp: jiff::Timestamp::now(),
        };
        assert!(matches_filters(
            &sse,
            &SseFilters {
                goal_id: Some("goal-cog".to_string()),
                ..Default::default()
            }
        ));
    }

    #[tokio::test]
    async fn test_forward_team_events_injects_scope_dimensions() {
        // 转发器必须把 goal/project/trace 维度写进 payload，供订阅过滤与追踪使用。
        use sisyphus_tools_agent::team::TeamEvent;

        let publisher = crate::http::sse_replay::SsePublisher::new(16, 16);
        let mut broadcast_rx = publisher.subscribe();
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(16);
        let handle = tokio::spawn(forward_team_events_to_sse(
            event_rx,
            publisher,
            TeamSseScope {
                goal_id: Some("goal-f".to_string()),
                project_id: Some("proj-f".to_string()),
                trace_id: Some("trace-f".to_string()),
            },
        ));

        event_tx.send(TeamEvent::PlanReady { task_count: 2 }).await.expect("事件应可发送");
        drop(event_tx);

        let ev = broadcast_rx.recv().await.expect("订阅者应收到事件");
        assert_eq!(ev.event_type(), "team_plan_ready");
        assert_eq!(ev.goal_id(), Some("goal-f"));
        assert_eq!(ev.project_id(), Some("proj-f"));
        assert_eq!(
            ev.payload_field("trace_id"),
            Some(serde_json::json!("trace-f"))
        );
        assert!(matches_filters(
            &ev,
            &SseFilters {
                goal_id: Some("goal-f".to_string()),
                project_id: Some("proj-f".to_string()),
                ..Default::default()
            }
        ));

        tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .expect("sender 释放后转发器应退出")
            .expect("转发器不得 panic");
    }

    /// team 事件转发必须走发布器：序号连续递增，断线后可按
    /// `Last-Event-ID` 从重放缓冲补发（M1.3）。
    #[tokio::test]
    async fn test_team_events_get_replayable_ids() {
        use sisyphus_tools_agent::team::TeamEvent;

        let publisher = crate::http::sse_replay::SsePublisher::new(16, 16);
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(16);
        let handle = tokio::spawn(forward_team_events_to_sse(
            event_rx,
            publisher.clone(),
            TeamSseScope {
                goal_id: Some("goal-ids".to_string()),
                project_id: Some("proj-ids".to_string()),
                trace_id: None,
            },
        ));

        event_tx.send(TeamEvent::PlanReady { task_count: 2 }).await.expect("发送计划事件");
        event_tx
            .send(TeamEvent::Synthesizing {
                completed: 1,
                failed: 0,
            })
            .await
            .expect("发送聚合事件");
        drop(event_tx);
        tokio::time::timeout(Duration::from_secs(5), handle)
            .await
            .expect("转发器应退出")
            .expect("不得 panic");

        let buffered = publisher.replay_since(Some(0));
        let ids: Vec<u64> = buffered.iter().map(|entry| entry.id).collect();
        assert_eq!(ids, vec![1, 2], "team 事件必须获得连续发布序号");
        assert_eq!(buffered[0].event_type(), "team_plan_ready");
        assert_eq!(buffered[1].event_type(), "team_synthesizing");
        assert_eq!(buffered[0].goal_id(), Some("goal-ids"));

        // 重连者从 1 号之后补发：只应拿到 2 号。
        let resumed = publisher.replay_since(Some(1));
        assert_eq!(resumed.len(), 1);
        assert_eq!(resumed[0].id, 2);
    }

    /// 端到端：`POST /api/team/execute` 的响应与 SSE 事件必须带同一
    /// goal/project 维度，客户端可据此过滤本次执行的事件流。
    #[tokio::test]
    async fn team_execute_handler_broadcasts_scoped_events() {
        use crate::http::handlers::team_execute_handler;
        use crate::http::types::{TeamExecuteRequest, WorkspaceContext};
        use sisyphus_tools_agent::team::{TaskNode, TeamPlan};

        let state = make_test_state(None).await;
        let mut rx = state.sse_broadcaster.subscribe();
        let req = TeamExecuteRequest {
            plan: TeamPlan {
                goal_id: "goal-http".to_string(),
                tasks: vec![TaskNode {
                    id: "bad".to_string(),
                    agent_id: "unknown-agent".to_string(),
                    tool_id: "unknown/tool".to_string(),
                    args: serde_json::json!({}),
                    depends_on: vec![],
                    read_keys: vec![],
                    write_keys: vec![],
                }],
            },
            project_id: Some("proj-http".to_string()),
            event_tx: None,
            impact_retry_rounds: None,
        };

        let resp = team_execute_handler(
            State(state.clone()),
            WorkspaceContext { project_id: None },
            None,
            Json(req),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 64 * 1024).await.expect("读取响应体");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("响应应为 JSON");
        assert_eq!(json["goal_id"], "goal-http");
        assert_eq!(json["success"], false);
        assert_eq!(json["sse_filter"]["goal_id"], "goal-http");
        assert_eq!(json["sse_filter"]["project_id"], "proj-http");

        let mut saw_complete = false;
        while let Ok(ev) = rx.try_recv() {
            if ev.event_type() == "team_complete" {
                saw_complete = true;
                assert_eq!(ev.goal_id(), Some("goal-http"));
                assert_eq!(ev.project_id(), Some("proj-http"));
                assert!(matches_filters(
                    &ev,
                    &SseFilters {
                        goal_id: Some("goal-http".to_string()),
                        project_id: Some("proj-http".to_string()),
                        ..Default::default()
                    }
                ));
            }
        }
        assert!(saw_complete, "应广播 team_complete 事件");
    }

    #[tokio::test]
    async fn test_execution_event_reaches_subscriber_end_to_end() {
        // 走真实广播链路：发送 ExecutionEvent → 转换 → 广播 → 订阅者收到，
        // 确认不是只有纯函数层面正确。
        let publisher = crate::http::sse_replay::SsePublisher::new(16, 16);
        let mut rx = publisher.subscribe();
        let event = sisyphus_types::ExecutionEvent::ExecutionComplete {
            result: sisyphus_types::NodeOutput::from(serde_json::json!("finished")),
        };

        let outcome = publisher.publish(SseEvent::from_execution_event(&event, "sop-e2e"));
        assert_eq!(outcome.receivers, 1, "应有订阅者");

        let got = rx.recv().await.expect("订阅者应收到事件");
        assert_eq!(got.event_type(), "execution_complete");
        assert_eq!(got.sop_id(), Some("sop-e2e"));
        assert!(matches_filters(
            &got,
            &SseFilters {
                sop_id: Some("sop-e2e".to_string()),
                ..Default::default()
            }
        ));
    }

    fn node_started(sop: &str) -> SseEvent {
        SseEvent::NodeExecutionStarted {
            sop_id: sop.to_string(),
            node_id: "n1".to_string(),
            node_name: "fetch".to_string(),
            timestamp: jiff::Timestamp::now(),
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
            matches_filters(
                &event,
                &SseFilters {
                    sop_id: Some("sop-abc".to_string()),
                    ..Default::default()
                }
            ),
            "订阅 ?sop_id=sop-abc 的客户端必须能收到 sop-abc 的干预事件"
        );
        // 其他 sop 的订阅者不应收到
        assert!(!matches_filters(
            &event,
            &SseFilters {
                sop_id: Some("sop-other".to_string()),
                ..Default::default()
            }
        ));
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
            timestamp: jiff::Timestamp::now(),
        };
        assert_eq!(goal_only.sop_id(), None);
    }

    #[test]
    fn test_session_id_extracted_only_from_session_variant() {
        let msg = SseEvent::SessionMessageAdded {
            session_id: "sess-1".to_string(),
            role: "user".to_string(),
            content: "hi".to_string(),
            timestamp: jiff::Timestamp::now(),
        };
        assert_eq!(msg.session_id(), Some("sess-1"));
        assert_eq!(node_started("sop-a").session_id(), None);
    }

    #[test]
    fn test_sop_filter_excludes_other_sop_and_dimensionless_events() {
        // 命中：sop_id 相等
        assert!(matches_filters(
            &node_started("sop-a"),
            &SseFilters {
                sop_id: Some("sop-a".to_string()),
                ..Default::default()
            }
        ));
        // 不命中：sop_id 不同
        assert!(!matches_filters(
            &node_started("sop-b"),
            &SseFilters {
                sop_id: Some("sop-a".to_string()),
                ..Default::default()
            }
        ));
        // 不命中：事件根本不带 sop_id 维度（不得当作「匹配全部」）
        let skill = SseEvent::SkillInstalled {
            skill_id: "s1".to_string(),
            version: "1.0".to_string(),
            timestamp: jiff::Timestamp::now(),
        };
        assert!(!matches_filters(
            &skill,
            &SseFilters {
                sop_id: Some("sop-a".to_string()),
                ..Default::default()
            }
        ));
    }

    #[test]
    fn test_no_filter_pushes_everything() {
        assert!(matches_filters(
            &node_started("sop-a"),
            &SseFilters::default()
        ));
        let skill = SseEvent::SkillInstalled {
            skill_id: "s1".to_string(),
            version: "1.0".to_string(),
            timestamp: jiff::Timestamp::now(),
        };
        assert!(matches_filters(&skill, &SseFilters::default()));
    }

    #[test]
    fn test_project_filter_matches_only_that_project() {
        let ev = |pid: &str| {
            SseEvent::custom(
                "custom_event".to_string(),
                serde_json::json!({"project_id": pid}),
            )
        };
        let mine = ev("proj-a");
        let other = ev("proj-b");
        let no_dim = SseEvent::custom("custom_event".to_string(), serde_json::json!({}));

        assert!(matches_filters(
            &mine,
            &SseFilters {
                project_id: Some("proj-a".to_string()),
                ..Default::default()
            }
        ));
        assert!(!matches_filters(
            &other,
            &SseFilters {
                project_id: Some("proj-a".to_string()),
                ..Default::default()
            }
        ));
        // 无项目维度的事件不得泄露给按项目隔离的订阅者
        assert!(!matches_filters(
            &no_dim,
            &SseFilters {
                project_id: Some("proj-a".to_string()),
                ..Default::default()
            }
        ));
    }

    /// 多个维度同时给出时为「与」关系。
    #[test]
    fn test_project_and_sop_filters_are_conjunctive() {
        let both = SseEvent::custom(
            "custom_event".to_string(),
            serde_json::json!({"project_id": "proj-a", "sop_id": "sop-1"}),
        );
        assert!(matches_filters(
            &both,
            &SseFilters {
                project_id: Some("proj-a".to_string()),
                sop_id: Some("sop-1".to_string()),
                ..Default::default()
            }
        ));
        // 项目对但 sop 不对 → 不推送
        assert!(!matches_filters(
            &both,
            &SseFilters {
                project_id: Some("proj-a".to_string()),
                sop_id: Some("sop-2".to_string()),
                ..Default::default()
            }
        ));
        // sop 对但项目不对 → 不推送
        assert!(!matches_filters(
            &both,
            &SseFilters {
                project_id: Some("proj-z".to_string()),
                sop_id: Some("sop-1".to_string()),
                ..Default::default()
            }
        ));
    }

    /// project_id 过滤必须真的走生产广播链路，而不只是纯函数。
    #[tokio::test]
    async fn test_broadcast_project_id_is_filterable_end_to_end() {
        let state = make_test_state(None).await;
        let mut rx = state.sse_broadcaster.subscribe();

        let req = BroadcastEventRequest {
            event_type: "custom_event".to_string(),
            payload: serde_json::json!({"data": 1}),
            project_id: Some("proj-e2e".to_string()),
            sop_id: None,
            session_id: None,
        };
        let resp = broadcast_event_handler(State(state), None, Json(req)).await.into_response();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);

        let ev = rx.recv().await.expect("订阅者应收到广播事件");
        assert!(matches_filters(
            &ev,
            &SseFilters {
                project_id: Some("proj-e2e".to_string()),
                ..Default::default()
            }
        ));
        assert!(!matches_filters(
            &ev,
            &SseFilters {
                project_id: Some("other-proj".to_string()),
                ..Default::default()
            }
        ));
    }

    #[test]
    fn test_type_and_dimension_filters_are_conjunctive() {
        let types = vec!["node_execution_started".to_string()];
        // 类型命中且 sop 命中 → 推送
        assert!(matches_filters(
            &node_started("sop-a"),
            &SseFilters {
                types: Some(types.clone()),
                sop_id: Some("sop-a".to_string()),
                ..Default::default()
            }
        ));
        // 类型命中但 sop 不命中 → 不推送
        assert!(!matches_filters(
            &node_started("sop-b"),
            &SseFilters {
                types: Some(types.clone()),
                sop_id: Some("sop-a".to_string()),
                ..Default::default()
            }
        ));
        // sop 命中但类型不命中 → 不推送
        assert!(!matches_filters(
            &node_started("sop-a"),
            &SseFilters {
                types: Some(vec!["skill_installed".to_string()]),
                sop_id: Some("sop-a".to_string()),
                ..Default::default()
            }
        ));
    }

    #[test]
    fn test_session_filter_excludes_other_sessions() {
        let mine = SseEvent::SessionMessageAdded {
            session_id: "sess-1".to_string(),
            role: "user".to_string(),
            content: "hi".to_string(),
            timestamp: jiff::Timestamp::now(),
        };
        let other = SseEvent::SessionMessageAdded {
            session_id: "sess-2".to_string(),
            role: "user".to_string(),
            content: "hi".to_string(),
            timestamp: jiff::Timestamp::now(),
        };
        assert!(matches_filters(
            &mine,
            &SseFilters {
                session_id: Some("sess-1".to_string()),
                ..Default::default()
            }
        ));
        assert!(!matches_filters(
            &other,
            &SseFilters {
                session_id: Some("sess-1".to_string()),
                ..Default::default()
            }
        ));
        // 节点事件没有 session 维度，被排除
        assert!(!matches_filters(
            &node_started("sop-a"),
            &SseFilters {
                session_id: Some("sess-1".to_string()),
                ..Default::default()
            }
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
            &SseFilters {
                sop_id: normalized.clone(),
                ..Default::default()
            }
        ));
    }

    #[tokio::test]
    async fn test_sse_broadcast_shared_channel() {
        // 同一 channel 内：先订阅后广播，订阅端必须收到事件
        let publisher = crate::http::sse_replay::SsePublisher::new(16, 16);
        let mut rx = publisher.subscribe();
        let event = SseEvent::Custom {
            event_type: "shared_test".to_string(),
            payload: json!({"k": "v"}),
            timestamp: jiff::Timestamp::now(),
        };
        assert_eq!(publisher.publish(event).receivers, 1);
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

    // ── 断线重连事件重放（M1.3）────────────────────────────────────

    /// 构造只关心重放游标/目标过滤的查询（`heartbeat: 0` 让读流只看到业务帧）。
    fn replay_query(last_event_id: Option<&str>, goal_id: Option<&str>) -> Query<SseQuery> {
        Query(SseQuery {
            events: None,
            project_id: None,
            goal_id: goal_id.map(str::to_string),
            sop_id: None,
            session_id: None,
            last_event_id: last_event_id.map(str::to_string),
            heartbeat: Some(0),
        })
    }

    fn custom_with_goal(event_type: &str, goal_id: &str) -> SseEvent {
        SseEvent::Custom {
            event_type: event_type.to_string(),
            payload: json!({ "goal_id": goal_id }),
            timestamp: jiff::Timestamp::now(),
        }
    }

    /// 从响应体读取至少 `want_data` 个 `data:` 帧（超过 2 秒视为失败）。
    async fn read_sse_body(body: axum::body::Body, want_data: usize) -> String {
        use futures_util::StreamExt;

        let mut stream = body.into_data_stream();
        let mut out = String::new();
        while out.matches("data:").count() < want_data {
            let chunk = tokio::time::timeout(Duration::from_secs(2), stream.next())
                .await
                .expect("等待 SSE 帧超时")
                .expect("事件流不应提前结束")
                .expect("SSE 数据块应可读");
            out.push_str(&String::from_utf8_lossy(&chunk));
        }
        out
    }

    /// 连接携带 `last_event_id`：必须补发该 ID 之后的事件（严格大于）。
    #[tokio::test]
    async fn test_replay_sends_events_after_last_event_id() {
        let state = make_test_state(None).await;
        // 无订阅者时发布：事件仍写入重放缓冲，供后续重连补发。
        let ids: Vec<u64> = ["e0", "e1", "e2"]
            .into_iter()
            .map(|name| state.sse_broadcaster.publish(custom_with_goal(name, "goal-replay")).id)
            .collect();
        assert_eq!(ids, vec![1, 2, 3], "发布侧序号从 1 单调递增");

        let resp = sse_handler(
            State(state.clone()),
            HeaderMap::new(),
            replay_query(Some("1"), None),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = read_sse_body(resp.into_body(), 2).await;
        assert!(
            body.contains("id: 2"),
            "游标之后的事件必须补发，实际帧：{body}"
        );
        assert!(
            body.contains("id: 3"),
            "游标之后的事件必须补发，实际帧：{body}"
        );
        assert!(
            body.contains("\"e1\""),
            "补发的事件内容必须保留，实际帧：{body}"
        );
        assert!(
            !body.contains("id: 1"),
            "游标本身（及更早事件）不得重发，实际帧：{body}"
        );
    }

    /// `Last-Event-ID` header 优先于 `?last_event_id=`。
    #[tokio::test]
    async fn test_replay_prefers_header_over_query() {
        let state = make_test_state(None).await;
        for name in ["e0", "e1", "e2"] {
            state.sse_broadcaster.publish(custom_with_goal(name, "goal-h"));
        }

        let mut headers = HeaderMap::new();
        headers.insert("last-event-id", "2".parse().expect("header value"));

        let resp = sse_handler(State(state), headers, replay_query(Some("0"), None)).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = read_sse_body(resp.into_body(), 1).await;
        assert!(
            body.contains("id: 3"),
            "header 游标之后的 3 号事件必须补发，实际帧：{body}"
        );
        assert!(
            !body.contains("id: 2"),
            "query 不应覆盖 header，实际帧：{body}"
        );
        assert!(
            !body.contains("id: 1"),
            "query 不应覆盖 header，实际帧：{body}"
        );
    }

    /// 新连接（无断点）不得重播历史；订阅后的实时事件仍按发布 ID 送达。
    #[tokio::test]
    async fn test_no_cursor_replays_nothing_but_live_events_arrive() {
        let state = make_test_state(None).await;
        state.sse_broadcaster.publish(custom_with_goal("old", "goal-live"));

        let resp = sse_handler(
            State(state.clone()),
            HeaderMap::new(),
            replay_query(None, None),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        // handler 返回时订阅已建立；实时发布的事件应携带发布序号。
        state.sse_broadcaster.publish(custom_with_goal("new", "goal-live"));

        let body = read_sse_body(resp.into_body(), 1).await;
        assert!(
            body.contains("id: 2"),
            "实时事件应带发布序号，实际帧：{body}"
        );
        assert!(
            body.contains("\"new\""),
            "新连接只收实时事件，实际帧：{body}"
        );
        assert!(
            !body.contains("\"old\""),
            "新连接不得收到历史事件，实际帧：{body}"
        );
    }

    /// 重放同样受维度过滤约束：补发的历史不能绕过 `?goal_id=`。
    #[tokio::test]
    async fn test_replay_respects_dimension_filters() {
        let state = make_test_state(None).await;
        state.sse_broadcaster.publish(custom_with_goal("keep", "goal-a"));
        state.sse_broadcaster.publish(custom_with_goal("drop", "goal-b"));

        let resp = sse_handler(
            State(state),
            HeaderMap::new(),
            replay_query(Some("0"), Some("goal-a")),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = read_sse_body(resp.into_body(), 1).await;
        assert!(
            body.contains("\"keep\""),
            "匹配维度的事件应补发，实际帧：{body}"
        );
        assert!(
            !body.contains("\"drop\""),
            "不匹配维度的事件不得补发，实际帧：{body}"
        );
    }

    /// 服务重启后客户端游标大于当前最新 ID：历史不可用，退回实时流，
    /// 不能因为「id <= 游标」把新进程的事件全部跳过。
    #[tokio::test]
    async fn test_stale_cursor_falls_back_to_live_stream() {
        let state = make_test_state(None).await;
        state.sse_broadcaster.publish(custom_with_goal("before", "goal-restart"));

        let resp = sse_handler(
            State(state.clone()),
            HeaderMap::new(),
            replay_query(Some("99"), None),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        // 重启语义下新事件的序号仍会从低位继续；必须送达而不是被游标吞掉。
        state.sse_broadcaster.publish(custom_with_goal("after", "goal-restart"));

        let body = read_sse_body(resp.into_body(), 1).await;
        assert!(
            body.contains("\"after\""),
            "重启后必须退回实时流，实际帧：{body}"
        );
    }

    /// 健康检查暴露重放窗口指标（buffered/capacity/latest_id）。
    #[tokio::test]
    async fn test_sse_health_reports_replay_stats() {
        let state = make_test_state(None).await;
        state.sse_broadcaster.publish(custom_with_goal("x", "goal-health"));

        let resp = sse_health_handler(State(state)).await.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.expect("读取响应体");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("JSON 响应");
        assert_eq!(json["replay"]["buffered"], 1);
        assert_eq!(json["replay"]["latest_id"], 1);
        assert!(
            json["replay"]["capacity"].as_u64().unwrap_or(0) > 0,
            "容量应为正数：{json}"
        );
    }

    /// 路由级：`Last-Event-ID` header 必须穿过 Router/中间件到达 handler。
    /// handler 直调测试无法覆盖路由层的 header 传递与 extractor 顺序。
    #[tokio::test]
    async fn test_router_replays_from_last_event_id_header() {
        use tower::ServiceExt as _;

        let state = make_test_state(None).await;
        state.sse_broadcaster.publish(custom_with_goal("first", "goal-route"));
        state.sse_broadcaster.publish(custom_with_goal("second", "goal-route"));

        let response = crate::http::build_router(state)
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/events?heartbeat=0")
                    .header("last-event-id", "1")
                    .body(axum::body::Body::empty())
                    .expect("构造请求"),
            )
            .await
            .expect("路由应返回响应");

        assert_eq!(response.status(), StatusCode::OK);
        let body = read_sse_body(response.into_body(), 1).await;
        assert!(
            body.contains("id: 2"),
            "路由层必须转发 header 触发重放，实际帧：{body}"
        );
        assert!(
            body.contains("\"second\""),
            "应补发游标之后的事件，实际帧：{body}"
        );
        assert!(
            !body.contains("id: 1"),
            "游标之前的事件不得重发，实际帧：{body}"
        );
    }

    /// P0 ⑥ 回归：走真实 handler 验证 permit 覆盖连接全程。
    /// 旧实现 permit 是 handler 局部变量，返回 Response 时即释放，
    /// 本测试第二个请求会得到 200（上限形同虚设）而非 429。
    #[tokio::test]
    async fn test_sse_permit_held_for_response_lifetime() {
        let mut state = make_test_state(None).await;
        state.sse_connection_limit = Arc::new(tokio::sync::Semaphore::new(1));

        let query = || {
            Query(SseQuery {
                events: None,
                project_id: None,
                goal_id: None,
                sop_id: None,
                session_id: None,
                last_event_id: None,
                heartbeat: None,
            })
        };

        let resp1 = sse_handler(State(state.clone()), axum::http::HeaderMap::new(), query()).await;
        assert_eq!(
            resp1.status(),
            axum::http::StatusCode::OK,
            "首个连接应被接纳"
        );

        let resp2 = sse_handler(State(state.clone()), axum::http::HeaderMap::new(), query()).await;
        assert_eq!(
            resp2.status(),
            SSE_LIMIT_REACHED,
            "持有一个活跃连接时第二个连接必须 429"
        );

        drop(resp1);
        let resp3 = sse_handler(State(state.clone()), axum::http::HeaderMap::new(), query()).await;
        assert_eq!(
            resp3.status(),
            axum::http::StatusCode::OK,
            "响应销毁后 permit 应归还"
        );
    }

    // ── /api/metrics 端点测试 ──────────────────────────────────

    /// 构造一个最小 RestApiState 供 metrics handler 测试
    async fn make_test_state(
        http_stats: Option<Arc<dyn Fn() -> sisyphus_types::HttpClientStats + Send + Sync>>,
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
        match http_stats {
            Some(p) => state.with_http_stats(p),
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
        let client = Arc::new(SharedHttpClient::new(cfg).expect("构造 client"));
        let reader = {
            let client = client.clone();
            Arc::new(move || client.stats())
                as Arc<dyn Fn() -> sisyphus_types::HttpClientStats + Send + Sync>
        };
        let state = make_test_state(Some(reader)).await;
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
