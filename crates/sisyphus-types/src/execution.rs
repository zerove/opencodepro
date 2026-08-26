use async_trait::async_trait;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::errors::TypesError;
use crate::types::PluginDescriptor;

/// Trait for WASM plugin lifecycle management.
/// Bridges the gap between `sisyphus-sandbox` (wasmtime) and consumers
/// so that crates depending only on this trait do not need wasmtime at compile time.
#[async_trait]
pub trait PluginManagerTrait: Send + Sync {
    /// Register a new plugin from its descriptor.
    async fn register_plugin(&self, descriptor: PluginDescriptor) -> Result<(), String>;
    /// Unload (remove) a previously registered plugin by name.
    fn unload_plugin(&self, name: &str) -> Result<(), String>;
    /// List all currently registered plugins.
    fn list_plugins(&self) -> Vec<PluginDescriptor>;
}

#[async_trait]
pub trait StateStore: Send + Sync {
    async fn get_state_json(
        &self,
        project_id: &str,
        key: &str,
    ) -> Result<Option<serde_json::Value>, TypesError>;
    async fn set_state_json(
        &self,
        project_id: &str,
        key: &str,
        value: serde_json::Value,
    ) -> Result<(), TypesError>;

    async fn get(&self, project_id: &str, key: &str) -> Result<Option<Vec<u8>>, TypesError>;
    async fn set(&self, project_id: &str, key: &str, value: &[u8]) -> Result<(), TypesError>;

    async fn get_traces_for_sop(
        &self,
        project_id: &str,
        sop_id: &str,
    ) -> Result<Vec<(String, String, String, i64, i64)>, TypesError>;
}

/// Strongly-typed wrapper around `serde_json::Value` for SOP node outputs.
/// Uses `Arc<Value>` internally to enable cheap cloning for concurrent access.
#[derive(Clone, Debug)]
pub struct NodeOutput(pub Arc<Value>);

impl Serialize for NodeOutput {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for NodeOutput {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(NodeOutput(Arc::new(Value::deserialize(deserializer)?)))
    }
}

impl NodeOutput {
    pub fn into_inner(self) -> Value {
        Arc::unwrap_or_clone(self.0)
    }

    /// Get a reference to the inner value.
    pub fn as_value(&self) -> &Value {
        &self.0
    }

    /// 显式节点状态提取（评审 P0-3：消灭字符串嗅探）。
    ///
    /// 按工具生态既有约定依次探测：`status`（字符串，"error"/"failed"
    /// 视为失败）、`success`（布尔）、`exit_code`（非零为失败）。
    /// 返回 `None` 表示输出未携带任何显式状态信号，
    /// 调用方应回退到各自的启发式（或视为未知）。
    pub fn explicit_status(&self) -> Option<bool> {
        let v = self.0.as_ref();
        // 1) status 字符串："error" / "failed" 为失败；其余非空值为成功
        if let Some(s) = v.get("status").and_then(|s| s.as_str()) {
            let lower = s.to_ascii_lowercase();
            return Some(!matches!(lower.as_str(), "error" | "failed"));
        }
        // 2) success 布尔
        if let Some(b) = v.get("success").and_then(|b| b.as_bool()) {
            return Some(b);
        }
        // 3) exit_code 整数：非零为失败
        if let Some(code) = v.get("exit_code").and_then(|c| c.as_i64()) {
            return Some(code == 0);
        }
        None
    }

    /// 便捷判定：显式状态优先；无显式状态时回退到文本嗅探
    /// （保持与旧逻辑兼容），完全空白输出视为健康。
    pub fn indicates_failure(&self) -> bool {
        if let Some(ok) = self.explicit_status() {
            return !ok;
        }
        let s = self
            .0
            .as_str()
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        let rendered = if s.is_empty() {
            self.0.to_string().to_ascii_lowercase()
        } else {
            s
        };
        rendered.contains("error") || rendered.contains("failed")
    }
}

impl Default for NodeOutput {
    fn default() -> Self {
        NodeOutput(Arc::new(Value::Null))
    }
}

impl From<Value> for NodeOutput {
    fn from(v: Value) -> Self {
        NodeOutput(Arc::new(v))
    }
}

impl From<Arc<Value>> for NodeOutput {
    fn from(v: Arc<Value>) -> Self {
        NodeOutput(v)
    }
}

impl std::ops::Deref for NodeOutput {
    type Target = Value;
    fn deref(&self) -> &Value {
        &self.0
    }
}

impl std::fmt::Display for NodeOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum SupervisorDecision {
    Continue {
        deviation: f64,
    },
    Pivot {
        reason: String,
        suggested_correction: Option<crate::graph::SOPGraphConfig>,
    },
}

#[async_trait]
pub trait RuntimeSupervisor: Send + Sync {
    async fn evaluate(
        &self,
        node_name: &str,
        output: &NodeOutput,
        ctx: Arc<ExecutionContext>,
    ) -> Result<SupervisorDecision, crate::errors::TypesError>;
}

impl PartialEq<Value> for NodeOutput {
    fn eq(&self, other: &Value) -> bool {
        *self.0 == *other
    }
}

impl PartialEq<NodeOutput> for Value {
    fn eq(&self, other: &NodeOutput) -> bool {
        *self == *other.0
    }
}

impl PartialEq for NodeOutput {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CognitiveMetrics {
    pub iteration: usize,
    pub deviation: f64,
    pub confidence: f64,
    pub progress: f32,
    pub token_cost: u64,
    pub stability: f64,
    pub info_gain: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CritiqueResult {
    pub critical_findings: Vec<String>,
    pub risk_score: f32, // 0.0 (safe) to 1.0 (high risk)
    pub suggested_refinement: String,
    pub should_proceed: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ExecutionEvent {
    NodeStarted {
        node_name: String,
    },
    NodeCompleted {
        node_name: String,
        output: NodeOutput,
        fuel_consumed: i64,
        duration_ms: i64,
    },
    NodeFailed {
        node_name: String,
        error: String,
        fuel_consumed: i64,
        duration_ms: i64,
    },
    ExecutionComplete {
        result: NodeOutput,
    },
    /// A machine-executable plan generated by the cognitive engine for an external agent (e.g., jcode).
    CognitivePlan {
        plan: CognitivePlan,
    },
    /// A status update from the cognitive loop (e.g., "Predicting", "Reflecting", "Critiquing").
    CognitiveStep {
        step: String,
        message: String,
        metrics: Option<CognitiveMetrics>,
    },
    /// A real-time micro-reflection result for a single node execution.
    MicroReflection {
        node_name: String,
        prediction: String,
        actual: String,
        decision: String,
        updates: Option<HashMap<String, Value>>,
        reason: Option<String>,
    },
    /// The cognitive loop has converged and completed.
    CognitiveComplete {
        report: String,
    },
}

/// A machine-executable plan generated by the cognitive engine for an external agent (e.g., jcode).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CognitivePlan {
    pub plan_id: String,
    pub goal: String,
    pub steps: Vec<CognitiveStep>,
    pub edges: Vec<PlanEdge>,
    pub start_node_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CognitiveStep {
    pub id: String,
    pub name: String,
    pub action: CognitiveAction,
    pub verification: Option<VerificationCriteria>,
    pub priority: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum CognitiveAction {
    ToolCall { tool_id: String, args: Value },
    FileEdit { path: String, patch: String },
    ShellExecute { command: String, timeout: u64 },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VerificationCriteria {
    pub check_command: Option<String>,
    pub expected_pattern: Option<String>,
    pub description: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlanEdge {
    pub from: String,
    pub to: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HistoryRecord {
    pub timestamp: u64,
    pub trace: String,
    pub deviation_score: f64,
    pub success: bool,
    pub cost: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ExecutionHistory {
    pub sop_id: String,
    pub records: Vec<HistoryRecord>,
}

/// A detailed record of a single node's execution within a cognitive iteration.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TracePoint {
    pub node_name: String,
    pub timestamp: u64,
    pub output_summary: String,
    pub deviation_score: f64,
    /// 监督器判定的显式状态（"ok"/"error"/"pivot"），None 表示旧数据
    /// 或无判定。P0-3 延伸：消费方应优先使用本字段，文本嗅探仅作
    /// None 时的兜底，避免输出提及 "Error" 造成误判。
    #[serde(default)]
    pub status: Option<String>,
}

/// The full execution path of a single cognitive iteration.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ExecutionTrace {
    pub sop_id: String,
    pub iteration: usize,
    pub points: Vec<TracePoint>,
    pub final_decision: SupervisorDecision,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ExecutionFeedback {
    pub plan_id: String,
    pub step_id: String,
    pub status: FeedbackStatus,
    pub output: String,
    pub error: Option<String>,
    pub state_snapshot: Option<Value>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum FeedbackStatus {
    Success,
    Failed,
    Ambiguous,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EvolutionResponse {
    pub should_pivot: bool,
    pub revised_plan: Option<CognitivePlan>,
    pub reflection_report: String,
}

use crate::types::SisyphusNotepad;

#[derive(Debug)]
pub struct ExecutionContext {
    pub results: DashMap<String, NodeOutput>,
    pub notepad: Arc<RwLock<SisyphusNotepad>>,
    pub trace: Arc<RwLock<Vec<TracePoint>>>,
    pub last_node_completed: RwLock<Option<String>>,
    pub modified_files: Arc<RwLock<std::collections::HashSet<std::path::PathBuf>>>,
    /// 每节点真实执行测量（P2-9 燃料真相管道）。由 WasmSOPNode 写入，
    /// 执行器事件组装点读取；无测量的执行器（默认实现）不产生条目。
    pub measurements: DashMap<String, crate::node::ExecutionMeasurement>,
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionContext {
    pub fn new() -> Self {
        Self {
            results: DashMap::new(),
            notepad: Arc::new(RwLock::new(SisyphusNotepad::default())),
            trace: Arc::new(RwLock::new(Vec::new())),
            last_node_completed: RwLock::new(None),
            modified_files: Arc::new(RwLock::new(std::collections::HashSet::new())),
            measurements: DashMap::new(),
        }
    }

    /// Creates an ExecutionContext with a pre-allocated results map.
    /// Use when the approximate number of node outputs is known.
    pub fn with_capacity(n: usize) -> Self {
        Self {
            results: DashMap::with_capacity(n),
            notepad: Arc::new(RwLock::new(SisyphusNotepad::default())),
            trace: Arc::new(RwLock::new(Vec::new())),
            last_node_completed: RwLock::new(None),
            modified_files: Arc::new(RwLock::new(std::collections::HashSet::new())),
            measurements: DashMap::with_capacity(n),
        }
    }

    /// 记录某节点的真实执行测量（P2-9）。
    pub fn record_measurement(
        &self,
        node_name: &str,
        measurement: crate::node::ExecutionMeasurement,
    ) {
        self.measurements.insert(node_name.to_string(), measurement);
    }

    /// 读取并移除某节点的执行测量（事件组装后消费，避免跨轮残留）。
    pub fn take_measurement(&self, node_name: &str) -> Option<crate::node::ExecutionMeasurement> {
        self.measurements.remove(node_name).map(|(_, m)| m)
    }

    /// 只读查看测量（诊断用，不移除）。
    pub fn get_measurement(&self, node_name: &str) -> Option<crate::node::ExecutionMeasurement> {
        self.measurements.get(node_name).map(|m| *m)
    }

    pub fn record_modified_file(&self, path: std::path::PathBuf) {
        if let Ok(mut files) = self.modified_files.write() {
            files.insert(path);
        } else {
            tracing::warn!("ExecutionContext modified_files RwLock poisoned");
        }
    }

    pub fn set_result(&self, node_name: String, output: NodeOutput) {
        self.results.insert(node_name.clone(), output);
        if let Ok(mut last) = self.last_node_completed.write() {
            *last = Some(node_name);
        }
    }

    pub fn push_trace_point(&self, point: TracePoint) {
        if let Ok(mut trace) = self.trace.write() {
            trace.push(point);
        } else {
            tracing::warn!("ExecutionContext trace RwLock poisoned");
        }
    }

    pub fn get_result(&self, node_name: &str) -> Option<NodeOutput> {
        self.results.get(node_name).map(|r| r.clone())
    }

    /// Returns the output of the most recently completed node.
    pub fn last_output(&self) -> Option<NodeOutput> {
        let last_node = self.last_node_completed.read().ok()?.clone();
        last_node.and_then(|name| self.get_result(&name))
    }

    pub fn write_notepad(&self, section: String, content: String) {
        match self.notepad.write() {
            Ok(mut notepad) => {
                notepad.write_section(section, content);
            }
            Err(e) => {
                tracing::warn!(error = %e, "ExecutionContext RwLock poisoned in write_notepad");
            }
        }
    }

    pub fn read_notepad(&self, section: &str) -> Option<String> {
        match self.notepad.read() {
            Ok(notepad) => notepad.read_section(section).cloned(),
            Err(e) => {
                tracing::warn!(error = %e, "ExecutionContext RwLock poisoned in read_notepad");
                None
            }
        }
    }

    pub fn prune_notepad(&self, max_insights: usize) {
        match self.notepad.write() {
            Ok(mut notepad) => {
                notepad.prune(max_insights);
            }
            Err(e) => {
                tracing::warn!(error = %e, "ExecutionContext RwLock poisoned in prune_notepad");
            }
        }
    }

    pub fn export_trace(
        &self,
        sop_id: String,
        iteration: usize,
        final_decision: SupervisorDecision,
    ) -> ExecutionTrace {
        let points = self.trace.read().map(|p| p.clone()).unwrap_or_default();
        ExecutionTrace {
            sop_id,
            iteration,
            points,
            final_decision,
        }
    }

    pub fn interpolate(&self, template: &str) -> String {
        let mut output = template.to_string();
        for item in self.results.iter() {
            let placeholder = format!("{{{{{}.output}}}}", item.key());
            let val_str = match item.value().0.as_ref() {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            output = output.replace(&placeholder, &val_str);
        }
        output
    }
}

/// 认知决策历史聚合摘要（跨会话健康指标）。
///
/// 由 `cognitive_traces` 表的历史记录聚合而来，用于评估认知循环的
/// 稳定性与收敛质量：偏差/置信度均值反映预测质量，决策分布反映
/// 反思频率，token 成本反映资源消耗。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CognitiveTraceSummary {
    /// 参与聚合的决策总数。
    pub total_decisions: u64,
    /// `CognitiveStep`（认知循环阶段步进）数量。
    pub step_count: u64,
    /// `MicroReflection`（节点级微反思）数量。
    pub reflection_count: u64,
    /// 平均偏差（0.0 表示无指标样本）。
    pub avg_deviation: f64,
    /// 平均置信度。
    pub avg_confidence: f64,
    /// 平均稳定性。
    pub avg_stability: f64,
    /// 平均信息增益。
    pub avg_info_gain: f64,
    /// 累计 token 成本。
    pub total_token_cost: u64,
    /// 微反思决策分布（CONTINUE / ADJUST / PIVOT → 次数）。
    pub decision_counts: std::collections::HashMap<String, u64>,
    /// 聚合窗口内最早记录时间（epoch 毫秒）。
    pub first_timestamp_ms: u128,
    /// 聚合窗口内最近记录时间（epoch 毫秒）。
    pub last_timestamp_ms: u128,
}

#[cfg(test)]
mod trace_point_status_tests {
    use super::*;

    /// 旧序列化数据（无 status 字段）必须能反序列化，
    /// 且 status 为 None（serde(default) 兼容性保证）。
    #[test]
    fn test_deserialize_legacy_trace_point_without_status() {
        let legacy = r#"{
            "node_name": "step1",
            "timestamp": 1700000000,
            "output_summary": "done",
            "deviation_score": 0.05
        }"#;
        let tp: TracePoint = serde_json::from_str(legacy).expect("legacy data must deserialize");
        assert_eq!(tp.node_name, "step1");
        assert!(tp.status.is_none());
    }

    #[test]
    fn test_trace_point_status_roundtrip() {
        let tp = TracePoint {
            node_name: "n".into(),
            timestamp: 1,
            output_summary: "PIVOT: drifted".into(),
            deviation_score: 1.0,
            status: Some("pivot".into()),
        };
        let s = serde_json::to_string(&tp).unwrap();
        let back: TracePoint = serde_json::from_str(&s).unwrap();
        assert_eq!(back.status.as_deref(), Some("pivot"));
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_explicit_status_field() {
        // status 字符串约定："error"/"failed" 为失败，其余为成功
        assert_eq!(
            NodeOutput::from(json!({"status": "error"})).explicit_status(),
            Some(false)
        );
        assert_eq!(
            NodeOutput::from(json!({"status": "Failed"})).explicit_status(),
            Some(false)
        );
        assert_eq!(
            NodeOutput::from(json!({"status": "saved"})).explicit_status(),
            Some(true)
        );
        assert_eq!(
            NodeOutput::from(json!({"status": "ok"})).explicit_status(),
            Some(true)
        );
    }

    #[test]
    fn test_explicit_success_and_exit_code() {
        assert_eq!(
            NodeOutput::from(json!({"success": true})).explicit_status(),
            Some(true)
        );
        assert_eq!(
            NodeOutput::from(json!({"success": false})).explicit_status(),
            Some(false)
        );
        assert_eq!(
            NodeOutput::from(json!({"exit_code": 0})).explicit_status(),
            Some(true)
        );
        assert_eq!(
            NodeOutput::from(json!({"exit_code": 1})).explicit_status(),
            Some(false)
        );
        // 优先级：status > success > exit_code
        let v = NodeOutput::from(json!({"status": "ok", "exit_code": 3}));
        assert_eq!(v.explicit_status(), Some(true));
    }

    #[test]
    fn test_no_signal_returns_none() {
        assert_eq!(
            NodeOutput::from(json!({"stdout": "hello"})).explicit_status(),
            None
        );
        assert_eq!(NodeOutput::default().explicit_status(), None);
    }

    #[test]
    fn test_indicates_failure_prefers_explicit_over_text() {
        // 显式 status=ok 但文本含 "Error"：显式信号必须获胜（这正是
        // 字符串嗅探的误报场景——输出内容讨论错误而非发生错误）。
        let v = NodeOutput::from(json!({"status": "ok", "stdout": "no Error found"}));
        assert!(!v.indicates_failure());
        // 显式失败
        assert!(NodeOutput::from(json!({"exit_code": 2})).indicates_failure());
    }

    #[test]
    fn test_indicates_failure_falls_back_to_heuristic() {
        // 无显式状态：保留旧嗅探语义作为兜底
        assert!(NodeOutput::from(json!("Error: disk full")).indicates_failure());
        assert!(NodeOutput::from(json!("operation Failed")).indicates_failure());
        assert!(!NodeOutput::from(json!("all good")).indicates_failure());
        assert!(!NodeOutput::default().indicates_failure());
    }
}
