use std::path::PathBuf;

use crate::bridge::{BridgeClient, SopInfo};
use crate::tui::app::{RemoteSkillEntry, WorkspaceEntry};

// ── 后台操作类型 ──────────────────────────

#[derive(Debug, Clone, serde::Deserialize)]
/// Sop Trace Event.
pub struct SopTraceEvent {
    pub node_id: String,
    pub status: String,
    pub output: Option<String>,
    pub fuel_consumed: i64,
    pub duration_ms: i64,
}

/// REST 认知追踪条目（与 `sisyphus-core::execution_manager::CognitiveTraceEntry` 对齐）。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CognitiveTraceRow {
    pub timestamp_ms: u128,
    pub sop_id: String,
    pub event: sisyphus_types::ExecutionEvent,
}

/// REST 异常观测条目（与 `sisyphus-bridge` 的 `/api/anomaly` 响应对齐）。
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct AnomalyRow {
    pub node_name: String,
    pub severity: String,
    pub metric: String,
    pub observed: f64,
    pub expected: f64,
    pub description: String,
    pub timestamp: u64,
}

/// REST 认知决策健康摘要（与 `sisyphus_types::CognitiveTraceSummary` 对齐）。
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct CognitiveSummaryRow {
    pub total_decisions: u64,
    pub step_count: u64,
    pub reflection_count: u64,
    pub avg_deviation: f64,
    pub avg_confidence: f64,
    pub avg_stability: f64,
    pub avg_info_gain: f64,
    pub total_token_cost: u64,
    pub decision_counts: std::collections::HashMap<String, u64>,
    pub first_timestamp_ms: u128,
    pub last_timestamp_ms: u128,
}

#[derive(Debug)]
/// Bg Op.
pub enum BgOp {
    SopsListed {
        count: usize,
        sops: Vec<SopInfo>,
    },
    SopsFailed(String),
    SopExecuted {
        sop_id: String,
        result: String,
    },
    SopExecuteFailed {
        sop_id: String,
        error: String,
    },
    SopTraceFetched {
        sop_id: String,
        trace: Vec<SopTraceEvent>,
    },
    SopTraceFailed(String),
    /// 认知决策链（可解释性）拉取结果。
    CognitiveTraceFetched {
        rows: Vec<CognitiveTraceRow>,
    },
    /// 认知决策历史（SQL 持久化）拉取结果。
    CognitiveTraceHistoryFetched {
        rows: Vec<CognitiveTraceRow>,
    },
    /// 认知决策健康摘要（跨会话聚合）拉取结果。
    CognitiveSummaryFetched {
        summary: CognitiveSummaryRow,
    },
    CognitiveSummaryFailed(String),
    CognitiveTraceFailed(String),
    /// 仪表盘异常明细（REST `/api/anomaly`）拉取结果。
    AnomalyReportFetched {
        rows: Vec<AnomalyRow>,
    },
    AnomalyReportFailed(String),
    RoadmapFetched {
        projection: String,
        bottlenecks: String,
    },
    RoadmapFailed(String),
    BridgeCmd {
        entries: Vec<super::LogEntry>,
    },
    SessionUpdated {
        session_id: String,
        errors: Vec<super::LogEntry>,
    },
    TodoUpdated,
    DevResult {
        goal: String,
        report: String,
        success: bool,
    },
    /// 开发 Agent 等待用户审批的工具调用（T2.1 MVP）。
    DevAsk {
        tool_id: String,
    },
    SkillsAvailable {
        skills: Vec<RemoteSkillEntry>,
    },
    SkillsInstalled {
        skills: Vec<RemoteSkillEntry>,
    },
    SkillOpResult {
        success: bool,
        message: String,
    },
    WorkspacesListed {
        workspaces: Vec<WorkspaceEntry>,
    },
    WorkspaceSwitched {
        id: String,
    },
    WorkspaceOpResult {
        success: bool,
        message: String,
    },
    /// 删除工作区的结果。
    WorkspaceDeleted {
        id: String,
    },
    /// 来自桥接的通用操作状态更新（例如 anomaly_detector、chaos_engine）。
    OpUpdate {
        op_id: String,
        status: String,
        result: String,
    },
    /// bridge `/health/deep` 拉取结果。
    HealthDeepFetched {
        /// 整体 ok 标志（与 HTTP 200/207 一致）
        all_ok: bool,
        /// 组件级状态 map
        components: serde_json::Map<String, serde_json::Value>,
        /// 原始 JSON（便于渲染全部字段）
        raw: serde_json::Value,
    },
    HealthDeepFailed(String),
}

// ── 后台生成器 ─────────────────────────────────

/// Spawn list sops.
pub fn spawn_list_sops(
    client: Option<crate::bridge::BridgeClient>,
    bridge_address: String,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let result = async {
            let mut client = match client {
                Some(c) => c,
                None => BridgeClient::connect(&bridge_address)
                    .await
                    .map_err(|e| format!("connect: {e}"))?,
            };
            let sops = client.list_sops().await.map_err(|e| format!("list: {e}"))?;
            let count = sops.len();
            Ok((count, sops))
        }
        .await;
        let _ = tx.send(match result {
            Ok((count, sops)) => BgOp::SopsListed { count, sops },
            Err(e) => BgOp::SopsFailed(e),
        });
    });
    rx
}

/// 启动在桥接上执行 SOP 的后台任务。
pub fn spawn_execute_sop(
    client: Option<crate::bridge::BridgeClient>,
    bridge_address: String,
    sop_id: String,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let result = async {
            let mut client = match client {
                Some(c) => c,
                None => BridgeClient::connect(&bridge_address)
                    .await
                    .map_err(|e| format!("connect: {e}"))?,
            };
            client.execute_sop(&sop_id).await.map_err(|e| format!("execute: {e}"))
        }
        .await;
        let _ = tx.send(match result {
            Ok(result) => BgOp::SopExecuted { sop_id, result },
            Err(e) => BgOp::SopExecuteFailed { sop_id, error: e },
        });
    });
    rx
}

/// 启动获取路线图数据（预测 + 瓶颈）的后台任务。
pub fn spawn_fetch_roadmap(
    client: Option<crate::bridge::BridgeClient>,
    bridge_address: String,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let result = async {
            let mut client = match client {
                Some(c) => c,
                None => BridgeClient::connect(&bridge_address)
                    .await
                    .map_err(|e| format!("connect: {e}"))?,
            };
            let projection = client
                .execute_tool("strategic:project_costs", "")
                .await
                .map_err(|e| format!("projection: {e}"))?;
            let bottlenecks = client
                .execute_tool("strategic:bottlenecks", "")
                .await
                .map_err(|e| format!("bottleneck: {e}"))?;
            Ok((projection.result_json, bottlenecks.result_json))
        }
        .await;
        let _ = tx.send(match result {
            Ok((projection, bottlenecks)) => BgOp::RoadmapFetched {
                projection,
                bottlenecks,
            },
            Err(e) => BgOp::RoadmapFailed(e),
        });
    });
    rx
}

/// Spawn session add message.
pub fn spawn_session_add_message(
    data_dir: PathBuf,
    session_id: String,
    role: String,
    content: String,
    bridge_address: Option<String>,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let session_db = data_dir.join("sessions.json");
        if let Err(e) = crate::tools::session_add_message(&session_db, &session_id, &role, &content)
        {
            let _ = tx.send(BgOp::BridgeCmd {
                entries: vec![super::LogEntry::Error(format!(
                    "Failed to add message: {e}"
                ))],
            });
            return;
        }

        // 若桥接可用，发送消息以触发 AI 响应。
        // 桥接错误仅记录日志，不会阻止 SessionUpdated — 会话
        // 必须始终重新加载，使 UI 不会卡在"思考"状态。
        let mut bridge_errors: Vec<super::LogEntry> = Vec::new();
        if let Some(ref addr) = bridge_address {
            let bridge_op = async {
                let mut client = BridgeClient::connect(addr).await.map_err(|e| format!("{e}"))?;
                client
                    .add_message(&session_id, &role, &content, &session_db.to_string_lossy())
                    .await
                    .map_err(|e| format!("{e}"))
            };
            match tokio::time::timeout(std::time::Duration::from_secs(120), bridge_op).await {
                Ok(Ok(resp)) => {
                    if !resp.success {
                        let err_msg = if resp.error.is_empty() {
                            "LLM returned empty response".into()
                        } else {
                            resp.error
                        };
                        bridge_errors.push(super::LogEntry::Error(format!(
                            "Bridge LLM error: {err_msg}",
                        )));
                    }
                },
                Ok(Err(e)) => {
                    bridge_errors.push(super::LogEntry::Error(format!(
                        "Bridge add_message failed: {e}",
                    )));
                },
                Err(_) => {
                    bridge_errors.push(super::LogEntry::Warn(
                        "LLM response timed out (120s), check bridge logs".into(),
                    ));
                },
            }
        }

        // 始终从磁盘重新加载会话详情，使"思考"指示停止
        // 且用户消息反映在持久化状态中。
        let _ = tx.send(BgOp::SessionUpdated {
            session_id,
            errors: bridge_errors,
        });
    });
    rx
}

/// T2.1 MVP：开发 Agent 运行句柄。
///
/// - `result`：任务最终结果（`DevResult`）接收端。
/// - `asks`：任务等待用户审批时下发的 `DevAsk` 消息队列。
/// - `answer`：审批应答通道（TUI → 任务）。
pub struct DevRunHandle {
    pub result: tokio::sync::oneshot::Receiver<BgOp>,
    pub asks: tokio::sync::mpsc::UnboundedReceiver<BgOp>,
    pub answer: tokio::sync::mpsc::UnboundedSender<bool>,
    /// 中断标志（T2.1 MVP）：TUI 置位后，开发循环在步骤边界返回 `Cancelled`。
    pub interrupt: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// T2.1 MVP：开发 Agent 审批 approver。
///
/// 判定走**统一权限体系**（T2.2）：先按 `CliPermission` 规则（glob / 危险默认 ask）
/// 判定 —— 命中 allow/deny 直接放行/拒绝并写审计；只有需要人工确认（Ask）时才
/// 向 TUI 下发 `DevAsk` 弹窗，等待用户应答。
///
/// 应答通道关闭（用户已离开）视为拒绝（fail-safe）。
struct DevAskApprover {
    ask: tokio::sync::mpsc::UnboundedSender<BgOp>,
    answer: tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<bool>>,
    perms: crate::permission::CliPermission,
}

#[async_trait::async_trait]
impl crate::tools::developer::DevApprover for DevAskApprover {
    async fn approve(&self, tool_id: &str, _action: &serde_json::Value) -> Result<bool, String> {
        use crate::permission::Decision;

        let decision = self.perms.evaluate(tool_id);
        match decision.decision {
            Decision::Allow => {
                self.perms.record_decision(tool_id, &decision, "allowed");
                return Ok(true);
            },
            Decision::Deny => {
                self.perms.record_decision(tool_id, &decision, "denied");
                return Ok(false);
            },
            Decision::Ask => {},
        }

        let _ = self.ask.send(crate::tui::background::BgOp::DevAsk {
            tool_id: tool_id.to_string(),
        });
        let mut answer = self.answer.lock().await;
        let answered =
            answer.recv().await.ok_or_else(|| "dev approval channel closed".to_string())?;
        self.perms.record_decision(
            tool_id,
            &decision,
            if answered {
                "asked-approved"
            } else {
                "asked-denied"
            },
        );
        Ok(answered)
    }
}

/// 派生 dev 运行的 checkpoint 目录（按目标区分）。
///
/// 同一目标的中断与恢复必须落在同一目录（resume 才能读到 checkpoint）；
/// 不同目标必须隔离 —— 否则 A 的 checkpoint 会被 B 覆盖，恢复 A 时按 B 的
/// 已完成步骤跳过，造成「跳过了没做过的步骤」。
fn dev_checkpoint_dir(goal: &str) -> std::path::PathBuf {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    goal.hash(&mut hasher);
    let digest = hasher.finish();
    let slug: String = goal
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(32)
        .collect();
    let slug = if slug.is_empty() {
        "goal".to_string()
    } else {
        slug
    };
    std::env::temp_dir().join("sisyphus_dev_agent").join(format!("{slug}-{digest:016x}"))
}

/// 启动运行开发代理并返回格式化报告的后台任务。
///
/// T2.1 MVP：同时接线 checkpoint（支持 resume）、中断标志与审批闸（`DevApprover`）。
pub fn spawn_dev_agent(
    config: sisyphus_types::Config,
    goal: String,
    context: String,
    parallel: bool,
    resume: bool,
) -> DevRunHandle {
    let (tx, result) = tokio::sync::oneshot::channel();
    let (ask_tx, asks) = tokio::sync::mpsc::unbounded_channel::<BgOp>();
    let (answer_tx, answer_rx) = tokio::sync::mpsc::unbounded_channel::<bool>();
    let interrupt = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let interrupt_task = interrupt.clone();
    tokio::spawn(async move {
        let config = std::sync::Arc::new(std::sync::RwLock::new(config));
        let llm: std::sync::Arc<dyn crate::tools::metis::LlmClient> =
            std::sync::Arc::new(crate::tools::llm::DynamicLlmClient::new(config));
        let developer = crate::tools::developer::Developer::new(llm)
            .with_checkpoint(dev_checkpoint_dir(&goal))
            .with_interrupt(interrupt_task)
            .with_approval_handler(std::sync::Arc::new(DevAskApprover {
                ask: ask_tx,
                answer: tokio::sync::Mutex::new(answer_rx),
                // 统一权限体系：dev 的工具调用同样受 glob 规则 / 危险默认 / 审计约束
                perms: crate::permission::CliPermission::load(None).unwrap_or_default(),
            }));
        let goal_c = goal.clone();
        match developer.develop(&goal, &context, parallel, resume).await {
            Ok(report) => {
                let formatted = crate::tools::developer::format_report(&report);
                let success =
                    report.overall_status == crate::tools::developer::DevOverallStatus::Success;
                let _ = tx.send(BgOp::DevResult {
                    goal: goal_c,
                    report: formatted,
                    success,
                });
            },
            Err(e) => {
                let _ = tx.send(BgOp::DevResult {
                    goal: goal_c,
                    report: format!("Development agent failed:\n{e}"),
                    success: false,
                });
            },
        }
    });
    DevRunHandle {
        result,
        asks,
        answer: answer_tx,
        interrupt,
    }
}

/// Spawn approve proposal.
pub fn spawn_approve_proposal(
    client: Option<crate::bridge::BridgeClient>,
    bridge_address: String,
    proposal_id: String,
    reviewer: String,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let result = async {
            let mut client = match client {
                Some(c) => c,
                None => BridgeClient::connect(&bridge_address)
                    .await
                    .map_err(|e| format!("connect: {e}"))?,
            };
            let args = serde_json::json!({
                "proposal_id": proposal_id,
                "reviewer": reviewer,
            });
            client
                .execute_tool("strategic:approve_proposal", &args.to_string())
                .await
                .map_err(|e| format!("approve: {e}"))
        }
        .await;
        let _ = tx.send(match result {
            Ok(_) => BgOp::BridgeCmd {
                entries: vec![super::LogEntry::Success("Proposal approved".to_string())],
            },
            Err(e) => BgOp::BridgeCmd {
                entries: vec![super::LogEntry::Error(format!("Approve failed: {e}"))],
            },
        });
    });
    rx
}

/// Spawn fetch sop trace.
pub fn spawn_fetch_sop_trace(
    bridge_address: String,
    sop_id: String,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let url = format!("{}/api/execute/goal/{}/trace", bridge_address, sop_id);
        let result = async {
            let resp = crate::bridge::with_bearer_auth(reqwest::Client::new().get(&url))
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Bridge returned error status: {}", resp.status()));
            }
            let data: serde_json::Value =
                resp.json().await.map_err(|e| format!("JSON parse failed: {e}"))?;
            let events: Vec<SopTraceEvent> = serde_json::from_value(data["events"].clone())
                .map_err(|e| format!("Trace event parse failed: {e}"))?;
            Ok(events)
        }
        .await;
        let _ = tx.send(match result {
            Ok(trace) => BgOp::SopTraceFetched { sop_id, trace },
            Err(e) => BgOp::SopTraceFailed(e),
        });
    });
    rx
}

/// Spawn fetch cognitive trace.
pub fn spawn_fetch_cognitive_trace(bridge_address: String) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let url = format!("{}/api/cognitive/trace?limit=100", bridge_address);
        let result = async {
            let resp = crate::bridge::with_bearer_auth(reqwest::Client::new().get(&url))
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Bridge returned error status: {}", resp.status()));
            }
            let data: serde_json::Value =
                resp.json().await.map_err(|e| format!("JSON parse failed: {e}"))?;
            let rows: Vec<CognitiveTraceRow> = serde_json::from_value(data["entries"].clone())
                .map_err(|e| format!("Cognitive trace parse failed: {e}"))?;
            Ok(rows)
        }
        .await;
        let _ = tx.send(match result {
            Ok(rows) => BgOp::CognitiveTraceFetched { rows },
            Err(e) => BgOp::CognitiveTraceFailed(e),
        });
    });
    rx
}

/// Spawn fetch cognitive trace history.
pub fn spawn_fetch_cognitive_trace_history(
    bridge_address: String,
    sop_id: Option<String>,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let mut url = format!("{}/api/cognitive/trace/history?limit=100", bridge_address);
        if let Some(sop) = sop_id {
            // sop_id 仅含 ASCII 标识符（如 "sop-main"），直接拼接即可。
            url.push_str(&format!("&sop_id={sop}"));
        }
        let result = async {
            let resp = crate::bridge::with_bearer_auth(reqwest::Client::new().get(&url))
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Bridge returned error status: {}", resp.status()));
            }
            let data: serde_json::Value =
                resp.json().await.map_err(|e| format!("JSON parse failed: {e}"))?;
            let rows: Vec<CognitiveTraceRow> = serde_json::from_value(data["entries"].clone())
                .map_err(|e| format!("Cognitive trace parse failed: {e}"))?;
            Ok(rows)
        }
        .await;
        let _ = tx.send(match result {
            Ok(rows) => BgOp::CognitiveTraceHistoryFetched { rows },
            Err(e) => BgOp::CognitiveTraceFailed(e),
        });
    });
    rx
}

/// Spawn fetch cognitive summary.
pub fn spawn_fetch_cognitive_summary(
    bridge_address: String,
    sop_id: Option<String>,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let mut url = format!("{}/api/cognitive/summary?limit=1000", bridge_address);
        if let Some(sop) = sop_id {
            // sop_id 仅含 ASCII 标识符（如 "sop-main"），直接拼接即可。
            url.push_str(&format!("&sop_id={sop}"));
        }
        let result = async {
            let resp = crate::bridge::with_bearer_auth(reqwest::Client::new().get(&url))
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Bridge returned error status: {}", resp.status()));
            }
            let summary: CognitiveSummaryRow =
                resp.json().await.map_err(|e| format!("JSON parse failed: {e}"))?;
            Ok(summary)
        }
        .await;
        let _ = tx.send(match result {
            Ok(summary) => BgOp::CognitiveSummaryFetched { summary },
            Err(e) => BgOp::CognitiveSummaryFailed(e),
        });
    });
    rx
}

/// 拉取仪表盘异常明细（REST `/api/anomaly`）。
///
/// 不带 `sop_id` 时返回全部作用域内的异常；服务端对空日志返回空数组。
pub fn spawn_fetch_anomaly_report(bridge_address: String) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let url = format!("{}/api/anomaly", bridge_address);
        let result = async {
            let resp = crate::bridge::with_bearer_auth(reqwest::Client::new().get(&url))
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Bridge returned error status: {}", resp.status()));
            }
            let rows: Vec<AnomalyRow> =
                resp.json().await.map_err(|e| format!("JSON parse failed: {e}"))?;
            Ok(rows)
        }
        .await;
        let _ = tx.send(match result {
            Ok(rows) => BgOp::AnomalyReportFetched { rows },
            Err(e) => BgOp::AnomalyReportFailed(e),
        });
    });
    rx
}

/// Spawn fetch available skills.
pub fn spawn_fetch_available_skills(
    bridge_address: String,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let url = format!("{}/api/skills/available", bridge_address);
        let result = async {
            let resp = crate::bridge::with_bearer_auth(reqwest::Client::new().get(&url))
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Bridge returned error status: {}", resp.status()));
            }
            let skills: Vec<RemoteSkillEntry> =
                resp.json().await.map_err(|e| format!("JSON parse failed: {e}"))?;
            Ok(skills)
        }
        .await;
        let _ = tx.send(match result {
            Ok(skills) => BgOp::SkillsAvailable { skills },
            Err(e) => BgOp::BridgeCmd {
                entries: vec![super::LogEntry::Error(e)],
            },
        });
    });
    rx
}

/// Spawn fetch installed skills.
pub fn spawn_fetch_installed_skills(
    bridge_address: String,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let url = format!("{}/api/skills/installed", bridge_address);
        let result = async {
            let resp = crate::bridge::with_bearer_auth(reqwest::Client::new().get(&url))
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Bridge returned error status: {}", resp.status()));
            }
            let skills: Vec<RemoteSkillEntry> =
                resp.json().await.map_err(|e| format!("JSON parse failed: {e}"))?;
            Ok(skills)
        }
        .await;
        let _ = tx.send(match result {
            Ok(skills) => BgOp::SkillsInstalled { skills },
            Err(e) => BgOp::BridgeCmd {
                entries: vec![super::LogEntry::Error(e)],
            },
        });
    });
    rx
}

/// Spawn install skill.
pub fn spawn_install_skill(
    bridge_address: String,
    skill_id: String,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let url = format!("{}/api/skills/install", bridge_address);
        let result = async {
            let resp = crate::bridge::with_bearer_auth(reqwest::Client::new().post(&url))
                .json(&serde_json::json!({ "skill_id": skill_id }))
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Bridge returned error status: {}", resp.status()));
            }
            Ok(())
        }
        .await;
        let _ = tx.send(match result {
            Ok(_) => BgOp::SkillOpResult {
                success: true,
                message: "Skill installed successfully".into(),
            },
            Err(e) => BgOp::SkillOpResult {
                success: false,
                message: e,
            },
        });
    });
    rx
}

/// Spawn uninstall skill.
pub fn spawn_uninstall_skill(
    bridge_address: String,
    skill_id: String,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let url = format!("{}/api/skills/remote/{}", bridge_address, skill_id);
        let result = async {
            let resp = crate::bridge::with_bearer_auth(reqwest::Client::new().delete(&url))
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Bridge returned error status: {}", resp.status()));
            }
            Ok(())
        }
        .await;
        let _ = tx.send(match result {
            Ok(_) => BgOp::SkillOpResult {
                success: true,
                message: "Skill uninstalled successfully".into(),
            },
            Err(e) => BgOp::SkillOpResult {
                success: false,
                message: e,
            },
        });
    });
    rx
}

/// Spawn list workspaces.
pub fn spawn_list_workspaces(bridge_address: String) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let url = format!("{}/api/workspaces", bridge_address);
        let result = async {
            let resp = crate::bridge::with_bearer_auth(reqwest::Client::new().get(&url))
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Bridge returned error status: {}", resp.status()));
            }
            let workspaces: Vec<WorkspaceEntry> =
                resp.json().await.map_err(|e| format!("JSON parse failed: {e}"))?;
            Ok(workspaces)
        }
        .await;
        let _ = tx.send(match result {
            Ok(workspaces) => BgOp::WorkspacesListed { workspaces },
            Err(e) => BgOp::WorkspaceOpResult {
                success: false,
                message: e,
            },
        });
    });
    rx
}

/// Spawn switch workspace.
pub fn spawn_switch_workspace(
    bridge_address: String,
    workspace_id: String,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let url = format!("{}/api/workspaces/switch", bridge_address);
        let result = async {
            let resp = crate::bridge::with_bearer_auth(reqwest::Client::new().post(&url))
                .json(&serde_json::json!({ "id": workspace_id }))
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Bridge returned error status: {}", resp.status()));
            }
            Ok(())
        }
        .await;
        let _ = tx.send(match result {
            Ok(_) => BgOp::WorkspaceSwitched { id: workspace_id },
            Err(e) => BgOp::WorkspaceOpResult {
                success: false,
                message: e,
            },
        });
    });
    rx
}

/// Spawn delete workspace.
pub fn spawn_delete_workspace(
    bridge_address: String,
    workspace_id: String,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let url = format!("{}/api/workspaces/{}", bridge_address, workspace_id);
        let result = async {
            let resp = crate::bridge::with_bearer_auth(reqwest::Client::new().delete(&url))
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Bridge returned error status: {}", resp.status()));
            }
            Ok(())
        }
        .await;
        let _ = tx.send(match result {
            Ok(_) => BgOp::WorkspaceDeleted { id: workspace_id },
            Err(e) => BgOp::WorkspaceOpResult {
                success: false,
                message: e,
            },
        });
    });
    rx
}

/// Spawn fetch bridge `/health/deep`.
/// 解析 `all_ok` 与 `components` 两层结构，便于 TUI `:health` 面板渲染。
pub fn spawn_fetch_health_deep(bridge_address: String) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let url = format!("{}/health/deep", bridge_address);
        let result = async {
            let resp = crate::bridge::with_bearer_auth(reqwest::Client::new().get(&url))
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            // `/health/deep` 在组件降级时返回 207 Multi-Status；视作可解析的成功。
            let status = resp.status();
            if !(status.is_success() || status.as_u16() == 207) {
                return Err(format!("bridge returned {}", status));
            }
            let raw: serde_json::Value =
                resp.json().await.map_err(|e| format!("JSON parse failed: {e}"))?;
            parse_health_deep_response(&raw).ok_or_else(|| "missing fields".to_string())
        }
        .await;
        let _ = tx.send(match result {
            Ok((all_ok, components, raw)) => BgOp::HealthDeepFetched {
                all_ok,
                components,
                raw,
            },
            Err(e) => BgOp::HealthDeepFailed(e),
        });
    });
    rx
}

/// 解析 `/health/deep` JSON 响应到 `(all_ok, components, raw)`。
/// 该函数是纯函数，便于单元测试覆盖边界条件。
pub fn parse_health_deep_response(
    raw: &serde_json::Value,
) -> Option<(
    bool,
    serde_json::Map<String, serde_json::Value>,
    serde_json::Value,
)> {
    let components = raw.get("components").and_then(|v| v.as_object())?.clone();

    // The bridge reports the verdict as a top-level `status` of "ok" /
    // "degraded"; it has no `all_ok` member. Prefer that, and otherwise
    // derive it from the component statuses using the same rule the bridge
    // applies -- "absent" is not a degradation (a CLI-mode bridge has no
    // shared HTTP client by design).
    let all_ok = match raw.get("status").and_then(|v| v.as_str()) {
        Some("ok") => true,
        Some("degraded") => false,
        _ => components.values().all(|c| {
            matches!(
                c.get("status").and_then(|v| v.as_str()),
                Some("ok") | Some("absent")
            )
        }),
    };

    Some((all_ok, components, raw.clone()))
}

#[cfg(test)]
mod health_deep_contract_tests {
    use super::parse_health_deep_response;

    fn fixture_path(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../sisyphus-types/test-fixtures")
            .join(name)
    }

    fn load(name: &str) -> serde_json::Value {
        let path = fixture_path(name);
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read fixture {}: {e}", path.display()));
        serde_json::from_str(&raw).expect("fixture must be valid JSON")
    }

    /// 与 bridge 侧读同一份固定件：handler 改了 `/health/deep` 的形状而这里
    /// 没跟上时，这条会先红。
    #[test]
    fn parses_the_shared_ok_fixture() {
        let raw = load("health_deep_ok.json");
        let (all_ok, components, _) =
            parse_health_deep_response(&raw).expect("ok fixture must parse");
        assert!(all_ok, "an `ok` payload must not be reported as degraded");
        for name in ["session_db", "shared_http", "kernel", "token_budget"] {
            assert!(
                components.contains_key(name),
                "component `{name}` disappeared from the parsed payload; the bridge \
                 contract changed"
            );
        }
    }

    #[test]
    fn parses_the_shared_degraded_fixture() {
        let raw = load("health_deep_degraded.json");
        let (all_ok, _, _) = parse_health_deep_response(&raw).expect("degraded fixture must parse");
        assert!(
            !all_ok,
            "a `degraded` payload must not be reported as healthy"
        );
    }
}

#[cfg(test)]
mod health_deep_parse_tests {
    use super::parse_health_deep_response;

    /// Captured verbatim from `GET /health/deep` on a running bridge.
    fn live_ok_payload() -> serde_json::Value {
        serde_json::json!({
            "components": {
                "kernel": {"status": "ok"},
                "session_db": {"path": "data/sessions.json", "status": "ok"},
                "shared_http": {"status": "ok"},
                "token_budget": {"cap": null, "exhausted": false, "status": "ok", "usage": 0},
            },
            "status": "ok",
            "version": "0.1.0"
        })
    }

    /// 回归：解析器曾读顶层 `all_ok` 布尔字段，而 bridge 从不返回该字段，
    /// 于是 `:health` 面板永远显示 "DEGRADED (missing fields)" —— 尽管
    /// 请求本身成功。夹具当时凭空捏造了 `all_ok`，把 bug 藏住了。
    #[test]
    fn parses_the_actual_live_response() {
        let (all_ok, components, _back) =
            parse_health_deep_response(&live_ok_payload()).expect("live payload must parse");
        assert!(all_ok, "a healthy bridge must not render as degraded");
        assert_eq!(components.len(), 4);
        assert_eq!(components["kernel"]["status"], "ok");
        assert_eq!(components["token_budget"]["status"], "ok");
    }

    #[test]
    fn absent_component_is_not_a_degradation() {
        let raw = serde_json::json!({
            "components": {
                "shared_http": {"status": "absent"},
                "session_db": {"status": "ok"},
            },
            "status": "ok"
        });
        let (all_ok, _, _) = parse_health_deep_response(&raw).expect("parse");
        assert!(all_ok);
    }

    #[test]
    fn degraded_status_is_reported() {
        let raw = serde_json::json!({
            "components": {"session_db": {"status": "degraded"}},
            "status": "degraded"
        });
        let (all_ok, components, _) = parse_health_deep_response(&raw).expect("parse");
        assert!(!all_ok);
        assert_eq!(components["session_db"]["status"], "degraded");
    }

    /// No top-level `status`: fall back to deriving from components.
    #[test]
    fn derives_verdict_when_status_absent() {
        let ok = serde_json::json!({
            "components": {"a": {"status": "ok"}, "b": {"status": "absent"}}
        });
        assert!(parse_health_deep_response(&ok).expect("parse").0);

        let bad = serde_json::json!({
            "components": {"a": {"status": "degraded"}}
        });
        assert!(!parse_health_deep_response(&bad).expect("parse").0);
    }

    #[test]
    fn missing_components_returns_none() {
        let raw = serde_json::json!({"status": "ok"});
        assert!(parse_health_deep_response(&raw).is_none());
    }
}

#[cfg(test)]
mod dev_checkpoint_tests {
    use super::dev_checkpoint_dir;

    #[test]
    fn same_goal_maps_to_same_dir() {
        assert_eq!(
            dev_checkpoint_dir("修复登录"),
            dev_checkpoint_dir("修复登录")
        );
    }

    #[test]
    fn different_goals_map_to_different_dirs() {
        assert_ne!(dev_checkpoint_dir("目标 A"), dev_checkpoint_dir("目标 B"));
    }

    #[test]
    fn goal_with_path_separators_is_sanitized() {
        let dir = dev_checkpoint_dir("../../etc/passwd");
        let name = dir.file_name().expect("文件名").to_string_lossy().to_string();
        assert!(!name.contains('/'), "目录名不应含路径分隔符: {name}");
        assert!(!name.contains(".."), "目录名不应含 ..: {name}");
    }
}

#[cfg(test)]
mod dev_approver_tests {
    use super::*;
    use crate::permission::CliPermission;
    // `approve` 是 trait 方法：需要把 trait 引入作用域
    use crate::tools::developer::DevApprover;
    use serde_json::json;
    use std::path::{Path, PathBuf};

    /// 每个用例独占目录（并行执行时共用目录会互相覆盖规则文件）。
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sisyphus-dev-approver-{}-{tag}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("创建临时目录");
        dir
    }

    fn perms(dir: &Path, rules: &str, audit: &Path) -> CliPermission {
        let rules_path = dir.join("rules.json");
        std::fs::write(&rules_path, rules).expect("写规则");
        CliPermission::load(Some(rules_path))
            .expect("加载规则")
            .with_audit_path(audit.to_path_buf())
    }

    #[tokio::test]
    async fn deny_rule_blocks_without_prompting() {
        let dir = temp_dir("deny");
        let audit = dir.join("audit.jsonl");
        let (ask_tx, mut ask_rx) = tokio::sync::mpsc::unbounded_channel::<BgOp>();
        let (_answer_tx, answer_rx) = tokio::sync::mpsc::unbounded_channel::<bool>();
        let approver = DevAskApprover {
            ask: ask_tx,
            answer: tokio::sync::Mutex::new(answer_rx),
            perms: perms(
                &dir,
                r#"[{"pattern":"builtin:bash_exec","permission":"deny"}]"#,
                &audit,
            ),
        };

        let allowed =
            approver.approve("builtin:bash_exec", &json!({})).await.expect("判定不应出错");
        assert!(!allowed, "deny 规则应拒绝执行");
        assert!(
            ask_rx.try_recv().is_err(),
            "deny 不应弹出审批（无需人工确认）"
        );
        let logged = std::fs::read_to_string(&audit).unwrap_or_default();
        assert!(
            logged.contains("\"resolved\":\"denied\""),
            "应写入 denied 审计：{logged}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn ask_decision_prompts_and_records_user_answer() {
        let dir = temp_dir("ask");
        let audit = dir.join("audit.jsonl");
        let (ask_tx, mut ask_rx) = tokio::sync::mpsc::unbounded_channel::<BgOp>();
        let (answer_tx, answer_rx) = tokio::sync::mpsc::unbounded_channel::<bool>();
        let approver = DevAskApprover {
            ask: ask_tx,
            answer: tokio::sync::Mutex::new(answer_rx),
            // 危险工具默认 ask（无规则时 builtin:bash_exec 走危险默认）
            perms: perms(&dir, "[]", &audit),
        };

        let pending = tokio::spawn(async move {
            let approver = approver;
            approver.approve("builtin:bash_exec", &json!({})).await
        });

        let ask = ask_rx.recv().await.expect("应下发审批请求");
        match ask {
            BgOp::DevAsk { tool_id } => assert_eq!(tool_id, "builtin:bash_exec"),
            other => panic!("期望 DevAsk，实际 {other:?}"),
        }
        answer_tx.send(true).expect("应答");

        let allowed = pending.await.expect("审批任务应结束").expect("判定不应出错");
        assert!(allowed, "用户放行应通过");
        let logged = std::fs::read_to_string(&audit).unwrap_or_default();
        assert!(
            logged.contains("\"resolved\":\"asked-approved\""),
            "应写入 asked-approved 审计：{logged}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn allowed_tool_runs_without_prompting() {
        let dir = temp_dir("allow");
        let audit = dir.join("audit.jsonl");
        let (ask_tx, mut ask_rx) = tokio::sync::mpsc::unbounded_channel::<BgOp>();
        let (_answer_tx, answer_rx) = tokio::sync::mpsc::unbounded_channel::<bool>();
        let approver = DevAskApprover {
            ask: ask_tx,
            answer: tokio::sync::Mutex::new(answer_rx),
            perms: perms(&dir, "[]", &audit),
        };

        // 非危险工具（文件写）默认放行，不打扰用户
        let allowed =
            approver.approve("builtin:file_write", &json!({})).await.expect("判定不应出错");
        assert!(allowed, "非危险工具应直接放行");
        assert!(ask_rx.try_recv().is_err(), "放行路径不应弹窗");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
