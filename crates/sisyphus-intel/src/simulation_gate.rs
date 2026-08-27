//! `SimulationGate` — predict → execute 之间的可插拔拦截图层。
//!
//! 动机（EVOLUTION_PLAN §2.1）：
//! - 在 SOP 实际执行前，基于 [`LogicalSimulator`] 评估图的结构合理性
//! - 拦截 `is_valid == false` 或包含 `Critical` 缺陷的 SOP
//! - 提供"接受/拒绝/重试"决策，附带修复建议
//!
//! 设计目标：
//! - 纯函数 + 单一 trait 依赖，便于单元测试
//! - 不强行侵入 `cognitive_loop`；调用方（bridge / kernel）显式调用
//! - 拦截率可量化：每次 gate 决策写入 `gate_history`，便于 nightly 评测
//!
//! 验收标准（与 EVOLUTION_PLAN §2.1 一致）：
//! - 至少能拦截包含 `Critical` 缺陷的 SOP
//! - 对合法 SOP 不产生误报（`false_positive_rate` 需在 `tests` 中验证）
//! - 提供 `intercept_rate` 统计（仅作观测，不强制 ≥20%，由真实数据驱动）
//!
//! 接入点（待调用方决定）：
//! - `bridge` 在 `ExecuteSop` 收到 SOP 时先调 `gate.check()`
//! - `kernel.execute_goal` 在合成 SOP 后、首次执行前调 `gate.check()`

use async_trait::async_trait;
use parking_lot::Mutex;
use sisyphus_types::{ExecutionContext, IntelError, SOPGraph};
use std::sync::Arc;
use std::time::Instant;

pub use crate::simulation::{FlawSeverity, LogicalFlaw, LogicalSimulationResult, LogicalSimulator};

/// 单次 gate 决策记录（用于 `intercept_rate` 统计与 nightly 趋势）。
#[derive(Debug, Clone, PartialEq)]
pub struct GateDecision {
    pub sop_id: String,
    pub verdict: GateVerdict,
    pub flaw_count: usize,
    pub critical_count: usize,
    pub confidence: f64,
    pub elapsed_micros: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateVerdict {
    /// 通过：可执行
    Allow,
    /// 拦截：含 Critical 缺陷，需修复后再试
    Reject,
    /// 跳过：gate 未启用（配置或 fallback）
    Skipped,
}

impl GateVerdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            GateVerdict::Allow => "allow",
            GateVerdict::Reject => "reject",
            GateVerdict::Skipped => "skipped",
        }
    }
}

/// Gate 决策历史（线程安全、内存累积）。
#[derive(Debug, Default)]
pub struct GateHistory {
    inner: Mutex<Vec<GateDecision>>,
}

impl GateHistory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&self, decision: GateDecision) {
        self.inner.lock().push(decision);
    }

    pub fn snapshot(&self) -> Vec<GateDecision> {
        self.inner.lock().clone()
    }

    /// 计算拦截率 = Reject / (Allow + Reject)。Skipped 不计入分母。
    pub fn intercept_rate(&self) -> f64 {
        let snap = self.snapshot();
        let allow = snap.iter().filter(|d| d.verdict == GateVerdict::Allow).count();
        let reject = snap.iter().filter(|d| d.verdict == GateVerdict::Reject).count();
        let denom = allow + reject;
        if denom == 0 {
            0.0
        } else {
            reject as f64 / denom as f64
        }
    }

    /// 误报率：allow 但实际有 Critical 缺陷的次数 / allow 总数。
    /// （仅作 hook：调用方可在执行后回调标记；目前始终返回 0.0）
    pub fn false_positive_rate(&self) -> f64 {
        0.0
    }
}

/// `SimulationGate` 配置。
#[derive(Debug, Clone)]
pub struct GateConfig {
    /// 若 `LogicalSimulator` 报错（LLM 不可用等），是否回退为 Skipped（不拦截）。
    pub fallback_on_error: bool,
    /// 是否启用 gate。`false` 时所有决策为 `Skipped`。
    pub enabled: bool,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            fallback_on_error: true,
            enabled: true,
        }
    }
}

/// `SimulationGate` — 拦截图层。
///
/// 持有 [`LogicalSimulator`] + [`GateConfig`] + [`GateHistory`]。
/// 每次 `check` 跑一次 `simulate_logic`，按规则产出 `GateDecision`。
pub struct SimulationGate {
    simulator: Arc<dyn LogicalSimulator>,
    config: GateConfig,
    history: Arc<GateHistory>,
}

impl SimulationGate {
    pub fn new(simulator: Arc<dyn LogicalSimulator>, config: GateConfig) -> Self {
        Self {
            simulator,
            config,
            history: Arc::new(GateHistory::new()),
        }
    }

    pub fn with_history(
        simulator: Arc<dyn LogicalSimulator>,
        config: GateConfig,
        history: Arc<GateHistory>,
    ) -> Self {
        Self {
            simulator,
            config,
            history,
        }
    }

    pub fn history(&self) -> &GateHistory {
        &self.history
    }

    pub fn config(&self) -> &GateConfig {
        &self.config
    }

    /// 决策：跑 simulate，按规则产出 verdict。
    ///
    /// 规则：
    /// 1. `config.enabled == false` → `Skipped`
    /// 2. simulator 错误 + `fallback_on_error=true` → `Skipped`（不阻塞生产）
    /// 3. `result.is_valid == false` → `Reject`
    /// 4. 含 `Critical` 缺陷 → `Reject`
    /// 5. 其他 → `Allow`
    pub async fn check(
        &self,
        graph: Arc<SOPGraph>,
        context: &ExecutionContext,
        sop_id: &str,
        goal_text: Option<&str>,
    ) -> Result<GateDecision, IntelError> {
        if !self.config.enabled {
            let decision = GateDecision {
                sop_id: sop_id.to_string(),
                verdict: GateVerdict::Skipped,
                flaw_count: 0,
                critical_count: 0,
                confidence: 0.0,
                elapsed_micros: 0,
            };
            self.history.record(decision.clone());
            return Ok(decision);
        }

        let start = Instant::now();
        let result = self
            .simulator
            .simulate_logic(graph, context, sop_id, goal_text)
            .await;

        match result {
            Ok(sim) => {
                let critical_count = sim
                    .flaws
                    .iter()
                    .filter(|f| f.severity == FlawSeverity::Critical)
                    .count();
                let verdict = if !sim.is_valid || critical_count > 0 {
                    GateVerdict::Reject
                } else {
                    GateVerdict::Allow
                };
                let decision = GateDecision {
                    sop_id: sop_id.to_string(),
                    verdict,
                    flaw_count: sim.flaws.len(),
                    critical_count,
                    confidence: sim.confidence,
                    elapsed_micros: start.elapsed().as_micros(),
                };
                self.history.record(decision.clone());
                Ok(decision)
            }
            Err(e) if self.config.fallback_on_error => {
                // 不阻塞生产：回退为 Skipped，并记录（让 nightly 看到失败率）
                let decision = GateDecision {
                    sop_id: sop_id.to_string(),
                    verdict: GateVerdict::Skipped,
                    flaw_count: 0,
                    critical_count: 0,
                    confidence: 0.0,
                    elapsed_micros: start.elapsed().as_micros(),
                };
                self.history.record(decision.clone());
                eprintln!("[simulation_gate] simulator error (fallback Skipped): {e}");
                Ok(decision)
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Arc;

    /// Mock simulator：返回预编程的 `LogicalSimulationResult`。
    struct MockSim {
        next: parking_lot::Mutex<Option<LogicalSimulationResult>>,
    }

    impl MockSim {
        fn new(result: LogicalSimulationResult) -> Self {
            Self {
                next: parking_lot::Mutex::new(Some(result)),
            }
        }
    }

    #[async_trait]
    impl LogicalSimulator for MockSim {
        async fn simulate_logic(
            &self,
            _graph: Arc<SOPGraph>,
            _ctx: &ExecutionContext,
            _sop_id: &str,
            _goal: Option<&str>,
        ) -> Result<LogicalSimulationResult, sisyphus_types::IntelError> {
            Ok(self.next.lock().take().unwrap_or(LogicalSimulationResult {
                is_valid: true,
                flaws: vec![],
                predicted_path: vec![],
                confidence: 1.0,
            }))
        }
    }

    fn empty_graph() -> Arc<SOPGraph> {
        Arc::new(SOPGraph::new())
    }

    fn empty_ctx() -> ExecutionContext {
        ExecutionContext::default()
    }

    fn empty_sop() -> LogicalSimulationResult {
        LogicalSimulationResult {
            is_valid: true,
            flaws: vec![],
            predicted_path: vec![],
            confidence: 0.9,
        }
    }

    fn critical_sop() -> LogicalSimulationResult {
        LogicalSimulationResult {
            is_valid: true, // 即使 is_valid=true，critical 仍应拦截
            flaws: vec![LogicalFlaw {
                node_name: "n1".into(),
                description: "deadlock".into(),
                severity: FlawSeverity::Critical,
            }],
            predicted_path: vec!["n1".into()],
            confidence: 0.5,
        }
    }

    fn invalid_sop() -> LogicalSimulationResult {
        LogicalSimulationResult {
            is_valid: false,
            flaws: vec![],
            predicted_path: vec![],
            confidence: 0.2,
        }
    }

    #[tokio::test]
    async fn gate_disabled_returns_skipped() {
        let sim = Arc::new(MockSim::new(empty_sop()));
        let gate = SimulationGate::new(
            sim,
            GateConfig {
                enabled: false,
                fallback_on_error: false,
            },
        );
        let d = gate
            .check(empty_graph(), &empty_ctx(), "sop-x", None)
            .await
            .expect("ok");
        assert_eq!(d.verdict, GateVerdict::Skipped);
    }

    #[tokio::test]
    async fn gate_allows_valid_sop() {
        let sim = Arc::new(MockSim::new(empty_sop()));
        let gate = SimulationGate::new(sim, GateConfig::default());
        let d = gate
            .check(empty_graph(), &empty_ctx(), "sop-ok", None)
            .await
            .expect("ok");
        assert_eq!(d.verdict, GateVerdict::Allow);
        assert_eq!(d.flaw_count, 0);
    }

    #[tokio::test]
    async fn gate_rejects_critical_flaw() {
        let sim = Arc::new(MockSim::new(critical_sop()));
        let gate = SimulationGate::new(sim, GateConfig::default());
        let d = gate
            .check(empty_graph(), &empty_ctx(), "sop-crit", None)
            .await
            .expect("ok");
        assert_eq!(d.verdict, GateVerdict::Reject);
        assert_eq!(d.critical_count, 1);
        assert_eq!(d.flaw_count, 1);
    }

    #[tokio::test]
    async fn gate_rejects_invalid_sop() {
        let sim = Arc::new(MockSim::new(invalid_sop()));
        let gate = SimulationGate::new(sim, GateConfig::default());
        let d = gate
            .check(empty_graph(), &empty_ctx(), "sop-inv", None)
            .await
            .expect("ok");
        assert_eq!(d.verdict, GateVerdict::Reject);
    }

    /// Warning severity 不应被拦截。
    #[tokio::test]
    async fn gate_allows_warning_only_sop() {
        let r = LogicalSimulationResult {
            is_valid: true,
            flaws: vec![LogicalFlaw {
                node_name: "n1".into(),
                description: "soft warning".into(),
                severity: FlawSeverity::Warning,
            }],
            predicted_path: vec!["n1".into()],
            confidence: 0.8,
        };
        let sim = Arc::new(MockSim::new(r));
        let gate = SimulationGate::new(sim, GateConfig::default());
        let d = gate
            .check(empty_graph(), &empty_ctx(), "sop-warn", None)
            .await
            .expect("ok");
        assert_eq!(d.verdict, GateVerdict::Allow);
        assert_eq!(d.critical_count, 0);
    }

    #[tokio::test]
    async fn gate_intercept_rate_calculation() {
        // 手动构造 history：1 allow, 1 reject → rate = 0.5
        let h = GateHistory::new();
        h.record(GateDecision {
            sop_id: "a".into(),
            verdict: GateVerdict::Allow,
            flaw_count: 0,
            critical_count: 0,
            confidence: 1.0,
            elapsed_micros: 0,
        });
        h.record(GateDecision {
            sop_id: "b".into(),
            verdict: GateVerdict::Reject,
            flaw_count: 1,
            critical_count: 1,
            confidence: 0.3,
            elapsed_micros: 0,
        });
        assert!((h.intercept_rate() - 0.5).abs() < 1e-9);
    }

    #[tokio::test]
    async fn gate_skipped_does_not_count_toward_rate() {
        let h = GateHistory::new();
        h.record(GateDecision {
            sop_id: "a".into(),
            verdict: GateVerdict::Skipped,
            flaw_count: 0,
            critical_count: 0,
            confidence: 0.0,
            elapsed_micros: 0,
        });
        // 只有 skipped → rate = 0（避免除零）
        assert_eq!(h.intercept_rate(), 0.0);
    }

    #[tokio::test]
    async fn gate_shared_history_aggregates() {
        let sim1 = Arc::new(MockSim::new(empty_sop()));
        let sim2 = Arc::new(MockSim::new(critical_sop()));
        let shared = Arc::new(GateHistory::new());
        let g1 = SimulationGate::with_history(sim1, GateConfig::default(), shared.clone());
        let g2 = SimulationGate::with_history(sim2, GateConfig::default(), shared.clone());
        g1.check(empty_graph(), &empty_ctx(), "a", None).await.unwrap();
        g2.check(empty_graph(), &empty_ctx(), "b", None).await.unwrap();
        let snap = shared.snapshot();
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].verdict, GateVerdict::Allow);
        assert_eq!(snap[1].verdict, GateVerdict::Reject);
        assert!((shared.intercept_rate() - 0.5).abs() < 1e-9);
    }
}
