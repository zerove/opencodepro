//! 认知循环 v2 — 自适应深度与运行时异常检测。
//!
//! ## 相对 v1 的增强
//! - **AnomalyDetector**：监控每个节点的执行指标（燃料、耗时、错误率），
//!   检测到异常时触发内联反思。
//! - **AdaptiveDepthController**：基于置信度与收敛速度决定是否
//!   继续迭代，而不是使用固定的 max_iterations 上限。
//!
//! 注：ParallelBranchPredictor 已于 2026-08-26 移除——其唯一入口
//! `evaluate_branches` 从未被主认知循环调用（仅 benches 引用），
//! 属未接线的死代码。决策记录见 docs/adr/0005-cognitive-loop-v2.md。

use parking_lot::Mutex;
use sisyphus_intel::ComplexityAnalyzer;
use sisyphus_types::experience::ExperienceBank;
use sisyphus_types::SOPGraph;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tracing::{info, instrument, warn};

/// 异常观测环形缓冲的容量。
///
/// 端点只做「最近发生了什么」的可观测性展示，不需要无限历史；
/// 有界缓冲可以避免长时间运行的进程把内存耗在异常列表上。
pub const RECENT_ANOMALY_CAPACITY: usize = 256;

// ---------------------------------------------------------------------------
// AnomalyDetector（异常检测器）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
/// Anomaly Severity.
pub enum AnomalySeverity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone)]
/// Node Anomaly.
pub struct NodeAnomaly {
    pub node_name: String,
    pub severity: AnomalySeverity,
    pub metric: String,
    pub observed: f64,
    pub expected: f64,
    pub description: String,
}

#[derive(Debug, Clone)]
/// Anomaly Report.
pub struct AnomalyReport {
    pub anomalies: Vec<NodeAnomaly>,
    pub should_extra_reflect: bool,
    pub overall_severity: AnomalySeverity,
}

/// 一条带上下文的异常观测记录。
///
/// [`AnomalyDetector::observe`] 的返回值只在调用点短暂存在，无法支撑
/// `/api/anomaly` 之类的查询端点。认知循环因此把每次检测到的异常额外
/// 记进 [`AnomalyLog`]，附带产生它的运行标识（`sop_id`）与检测时刻。
#[derive(Debug, Clone)]
pub struct ObservedAnomaly {
    pub anomaly: NodeAnomaly,
    /// 产生该异常的认知循环运行标识（`sop_id`）。
    pub scope: String,
    /// 检测时刻（Unix 秒）。
    pub detected_at: u64,
    /// 日志内单调递增的序号，供流式端点做增量推送（避免重复）。
    pub seq: u64,
}

/// 有界的异常观测日志，供 `/api/anomaly` 与 gRPC 流式端点读取。
///
/// 它与 [`AnomalyDetector`] **刻意分离**：检测器是有状态的 EWMA 基线，
/// 每次认知循环运行都持有自己的实例，从而让并发运行互不污染基线
/// （见 `test_concurrent_loops_have_independent_depth_history`）。
/// 把观测镜像进共享检测器会破坏这一隔离，因此可观测性走这条只追加的
/// 旁路日志：per-run 检测器照旧做控制决策，结果写入这里供外部查询。
pub struct AnomalyLog {
    recent: Mutex<VecDeque<ObservedAnomaly>>,
    next_seq: AtomicU64,
}

impl AnomalyLog {
    /// 新建一个空的异常日志。
    pub fn new() -> Self {
        Self {
            recent: Mutex::new(VecDeque::with_capacity(RECENT_ANOMALY_CAPACITY)),
            next_seq: AtomicU64::new(0),
        }
    }

    /// 记录一批检测结果（`scope` 是产生它们的运行标识，通常是 `sop_id`）。
    ///
    /// 空切片是常态（大多数观测不触发异常），直接返回。
    pub fn record(&self, anomalies: &[NodeAnomaly], scope: &str) {
        if anomalies.is_empty() {
            return;
        }
        let detected_at = now_unix_secs();
        let mut recent = self.recent.lock();
        for anomaly in anomalies {
            while recent.len() >= RECENT_ANOMALY_CAPACITY {
                recent.pop_front();
            }
            recent.push_back(ObservedAnomaly {
                anomaly: anomaly.clone(),
                scope: scope.to_string(),
                detected_at,
                seq: self.next_seq.fetch_add(1, Ordering::Relaxed),
            });
        }
    }

    /// 最近的异常观测（按检测顺序，最旧在前）。
    ///
    /// `scope` 为 `Some` 时只返回该运行标识（`sop_id`）下的记录。
    pub fn recent(&self, scope: Option<&str>) -> Vec<ObservedAnomaly> {
        let recent = self.recent.lock();
        recent
            .iter()
            .filter(|entry| match scope {
                Some(scope) => entry.scope == scope,
                None => true,
            })
            .cloned()
            .collect()
    }

    /// 当前保存的记录条数（不超过 [`RECENT_ANOMALY_CAPACITY`]）。
    pub fn len(&self) -> usize {
        self.recent.lock().len()
    }

    /// 日志是否为空。
    pub fn is_empty(&self) -> bool {
        self.recent.lock().is_empty()
    }

    /// 清空日志并重置序号。
    pub fn clear(&self) {
        self.recent.lock().clear();
        self.next_seq.store(0, Ordering::Relaxed);
    }
}

impl Default for AnomalyLog {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for AnomalyLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnomalyLog")
            .field("len", &self.len())
            .field("next_seq", &self.next_seq.load(Ordering::Relaxed))
            .finish()
    }
}

/// 跟踪每个节点的指标并在运行时检测执行异常。
/// 设计用于集成到认知循环的执行步骤中。
#[allow(clippy::type_complexity)]
pub struct AnomalyDetector {
    /// 每节点历史指标：node_name -> (avg_fuel, avg_duration_ms, error_count)
    history: Arc<Mutex<HashMap<String, (f64, f64, u64)>>>,
    /// 超过该阈值倍数时标记为异常。
    pub fuel_threshold: f64,
    pub duration_threshold: f64,
    pub error_rate_threshold: f64,
    /// 移动平均的窗口大小。
    pub history_window: usize,
    /// 已跟踪的总执行次数计数。
    total_executions: Arc<Mutex<u64>>,
}

impl Default for AnomalyDetector {
    fn default() -> Self {
        Self {
            history: Arc::new(Mutex::new(HashMap::new())),
            fuel_threshold: 2.0,       // 2x expected fuel is anomalous
            duration_threshold: 3.0,   // 3x expected duration is anomalous
            error_rate_threshold: 0.3, // >30% error rate is anomalous
            history_window: 20,
            total_executions: Arc::new(Mutex::new(0)),
        }
    }
}

impl AnomalyDetector {
    /// New.
    pub fn new(fuel_threshold: f64, duration_threshold: f64, error_rate_threshold: f64) -> Self {
        Self::with_history_window(fuel_threshold, duration_threshold, error_rate_threshold, 20)
    }

    /// 使用完整参数（含 `history_window`）构造。
    /// 当前实现尚未在 EWMA 中消费 `history_window`，但保留字段供
    /// 未来切换为滑动窗口策略时直接使用。
    pub fn with_history_window(
        fuel_threshold: f64,
        duration_threshold: f64,
        error_rate_threshold: f64,
        history_window: usize,
    ) -> Self {
        Self {
            history: Arc::new(Mutex::new(HashMap::new())),
            fuel_threshold,
            duration_threshold,
            error_rate_threshold,
            history_window: history_window.max(1),
            total_executions: Arc::new(Mutex::new(0)),
        }
    }

    /// 从类型层 `AnomalyDetectorConfig` 构造。
    /// 在 `StrategicManager` 初始化阶段被调用，使阈值可配置而非硬编码。
    pub fn from_config(cfg: &sisyphus_types::config::AnomalyDetectorConfig) -> Self {
        Self::with_history_window(
            cfg.fuel_threshold,
            cfg.duration_threshold,
            cfg.error_rate_threshold,
            cfg.history_window,
        )
    }

    /// 运行时覆盖阈值（用于 `apply_cognitive_loop_config`）。
    /// 保留 `history` 缓冲以避免中断正在进行的 EWMA 跟踪。
    /// 外部调用方需在 `Arc<Mutex<AnomalyDetector>>` 下持有写锁。
    pub fn apply_config(
        &mut self,
        fuel_threshold: f64,
        duration_threshold: f64,
        error_rate_threshold: f64,
        history_window: usize,
    ) {
        self.fuel_threshold = fuel_threshold;
        self.duration_threshold = duration_threshold;
        self.error_rate_threshold = error_rate_threshold;
        self.history_window = history_window.max(1);
    }

    /// 记录一次节点执行观测并检查异常。
    /// 返回本次观测检测到的异常（如果有）。
    #[instrument(skip(self), fields(node_name, fuel_consumed, duration_ms, success))]
    pub fn observe(
        &self,
        node_name: &str,
        fuel_consumed: u64,
        duration_ms: u64,
        success: bool,
    ) -> Vec<NodeAnomaly> {
        let mut anomalies = Vec::new();
        let mut history = self.history.lock();

        let (ref mut avg_fuel, ref mut avg_dur, ref mut err_count) =
            history.entry(node_name.to_string()).or_insert((0.0, 0.0, 0));

        // 指数加权移动平均更新
        let alpha = 0.3;
        if *avg_fuel == 0.0 {
            *avg_fuel = fuel_consumed as f64;
            *avg_dur = duration_ms as f64;
        } else {
            *avg_fuel = alpha * fuel_consumed as f64 + (1.0 - alpha) * *avg_fuel;
            *avg_dur = alpha * duration_ms as f64 + (1.0 - alpha) * *avg_dur;
        }
        if !success {
            *err_count += 1;
        }

        // 异常检测
        if fuel_consumed as f64 > *avg_fuel * self.fuel_threshold && *avg_fuel > 0.0 {
            anomalies.push(NodeAnomaly {
                node_name: node_name.to_string(),
                severity: AnomalySeverity::Medium,
                metric: "fuel".into(),
                observed: fuel_consumed as f64,
                expected: *avg_fuel,
                description: format!(
                    "Fuel consumption ({}) is {:.1}x above average ({:.0})",
                    fuel_consumed,
                    *avg_fuel,
                    fuel_consumed as f64 / *avg_fuel
                ),
            });
        }

        if duration_ms as f64 > *avg_dur * self.duration_threshold && *avg_dur > 0.0 {
            let sev = if duration_ms as f64 > *avg_dur * 5.0 {
                AnomalySeverity::High
            } else {
                AnomalySeverity::Low
            };
            anomalies.push(NodeAnomaly {
                node_name: node_name.to_string(),
                severity: sev,
                metric: "duration".into(),
                observed: duration_ms as f64,
                expected: *avg_dur,
                description: format!(
                    "Duration ({}ms) is {:.1}x above average ({:.0}ms)",
                    duration_ms,
                    duration_ms as f64 / *avg_dur,
                    *avg_dur
                ),
            });
        }

        let total = *err_count + 1;
        let error_rate = *err_count as f64 / total as f64;
        if error_rate > self.error_rate_threshold && total > 3 {
            anomalies.push(NodeAnomaly {
                node_name: node_name.to_string(),
                severity: AnomalySeverity::Critical,
                metric: "error_rate".into(),
                observed: error_rate,
                expected: self.error_rate_threshold,
                description: format!(
                    "Error rate ({:.1}%) exceeds threshold ({:.0}%)",
                    error_rate * 100.0,
                    self.error_rate_threshold * 100.0
                ),
            });
        }

        let mut total_exec = self.total_executions.lock();
        *total_exec += 1;
        drop(total_exec);
        drop(history);

        anomalies
    }

    fn anomaly_severity_at_least(severity: AnomalySeverity, threshold: AnomalySeverity) -> bool {
        use AnomalySeverity::*;
        match (&severity, &threshold) {
            (Critical, _) => true,
            (High, Critical) => false,
            (High, _) => true,
            (Medium, Low) | (Medium, Medium) => true,
            (Medium, _) => false,
            (Low, Low) => true,
            _ => false,
        }
    }

    /// 为当前状态生成完整的异常报告。
    pub fn generate_report(&self) -> AnomalyReport {
        let history = self.history.lock();
        let mut all_anomalies = Vec::new();
        let mut max_sev = AnomalySeverity::Low;

        // 这里不做重新扫描，因为异常已内联检测。
        // 此方法返回当前跟踪状态的摘要。
        let node_count = history.len();
        let total_error_count: u64 = history.values().map(|(_, _, err)| *err).sum();

        if total_error_count > 3 && node_count > 0 {
            let avg_errors = total_error_count as f64 / node_count as f64;
            if avg_errors > self.error_rate_threshold {
                all_anomalies.push(NodeAnomaly {
                    node_name: "__system__".into(),
                    severity: AnomalySeverity::Medium,
                    metric: "global_error_rate".into(),
                    observed: avg_errors,
                    expected: self.error_rate_threshold,
                    description: format!(
                        "Global error rate ({:.1}%) exceeds threshold",
                        avg_errors * 100.0
                    ),
                });
                max_sev = AnomalySeverity::Medium;
            }
        }

        let should_extra_reflect = Self::anomaly_severity_at_least(max_sev, AnomalySeverity::High)
            || all_anomalies.len() > 2
            || total_error_count > 10;

        AnomalyReport {
            anomalies: all_anomalies,
            should_extra_reflect,
            overall_severity: max_sev,
        }
    }

    /// 为新一轮执行周期重置跟踪状态。
    pub fn reset(&self) {
        let mut history = self.history.lock();
        history.clear();
        drop(history);
        let mut total = self.total_executions.lock();
        *total = 0;
    }
}

/// 当前 Unix 时间戳（秒）。
fn now_unix_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()
}

// ---------------------------------------------------------------------------
// AdaptiveDepthController（自适应深度控制器）
// ---------------------------------------------------------------------------

/// 基于收敛速度与置信度控制认知循环深度。
/// 不使用固定 max_iterations，而是采用目标置信度与最小改进量。
pub struct AdaptiveDepthController {
    /// 停止前要达到的目标置信度（0.0 - 1.0）。
    /// RwLock 包装以支持运行时重配（评审 A4：配置接线）。
    pub target_confidence: parking_lot::RwLock<f64>,
    /// 继续迭代所需的相邻轮次最小改进量。
    pub min_improvement: parking_lot::RwLock<f64>,
    /// 无论进展如何的迭代次数硬上限。
    pub max_iterations: usize,
    /// 允许提前停止前的最小迭代次数。
    pub min_iterations: usize,
    /// 收敛窗口：计算速度时回看的迭代次数。
    pub convergence_window: usize,
    /// 每轮迭代的置信度历史。
    confidence_history: Arc<Mutex<Vec<f64>>>,
}

impl Default for AdaptiveDepthController {
    fn default() -> Self {
        Self {
            target_confidence: parking_lot::RwLock::new(0.95),
            min_improvement: parking_lot::RwLock::new(0.01),
            max_iterations: 30,
            min_iterations: 2,
            convergence_window: 3,
            confidence_history: Arc::new(Mutex::new(Vec::with_capacity(30))),
        }
    }
}

impl AdaptiveDepthController {
    /// 应用配置参数（评审 A4 接线）。&self 即可写入（内部可变性），
    /// 使 StrategicManager 在 Arc 共享下仍能运行时重配。
    pub fn apply_config(&self, target_confidence: f64, min_improvement: f64) {
        *self.target_confidence.write() = target_confidence;
        *self.min_improvement.write() = min_improvement;
    }

    /// New.
    pub fn new(
        target_confidence: f64,
        min_improvement: f64,
        max_iterations: usize,
        min_iterations: usize,
    ) -> Self {
        Self {
            target_confidence: parking_lot::RwLock::new(target_confidence),
            min_improvement: parking_lot::RwLock::new(min_improvement),
            max_iterations,
            min_iterations,
            convergence_window: 3,
            confidence_history: Arc::new(Mutex::new(Vec::with_capacity(max_iterations))),
        }
    }

    /// 分析任务复杂度以确定初始迭代预算。
    pub async fn initial_budget(
        &self,
        experience_bank: Arc<dyn ExperienceBank>,
        sop_graph: &SOPGraph,
        goal_text: &str,
        project_id: Option<&str>,
    ) -> usize {
        let depth =
            ComplexityAnalyzer::analyze(experience_bank, sop_graph, goal_text, project_id).await;
        let (d_max, _) = depth.to_config();
        self.budget_from_depth(d_max)
    }

    /// 根据调用方已有的复杂度分析结果计算初始预算（P1-4 去重）。
    /// 与 [`Self::initial_budget`] 语义一致，但不再重复执行分析。
    pub fn budget_from_depth(&self, d_max: u32) -> usize {
        (d_max as usize).min(self.max_iterations).max(self.min_iterations)
    }

    /// 在每轮迭代后记录置信度并决定是否继续。
    /// 循环应继续时返回 `true`，停止时返回 `false`。
    #[instrument(skip(self))]
    pub fn observe_iteration(&self, confidence: f64) -> bool {
        let mut hist = self.confidence_history.lock();
        hist.push(confidence);
        let len = hist.len();

        // 至少运行 min_iterations 次
        if len < self.min_iterations {
            return true;
        }

        // 硬上限
        if len >= self.max_iterations {
            info!(
                "AdaptiveDepthController: reached max iterations ({})",
                self.max_iterations
            );
            return false;
        }

        // 达到目标？
        if confidence >= *self.target_confidence.read() {
            info!(
                "AdaptiveDepthController: target confidence ({:.3}) reached",
                confidence
            );
            return false;
        }

        // 检查最近 N 轮的收敛速度
        if len >= self.convergence_window {
            let window = &hist[len - self.convergence_window..];
            let first = window.first().copied().unwrap_or(0.0);
            let last = window.last().copied().unwrap_or(0.0);
            let improvement = last - first;

            let min_improvement_now = *self.min_improvement.read();
            if improvement.abs() < min_improvement_now {
                info!(
                    "AdaptiveDepthController: convergence stalled (improvement={:.4} < min={:.4})",
                    improvement, min_improvement_now
                );
                return false;
            }
        }

        true
    }

    /// 为新的认知循环重置状态。
    pub fn reset(&self) {
        let mut hist = self.confidence_history.lock();
        hist.clear();
    }

    /// 获取当前置信度历史。
    pub fn confidence_history(&self) -> Vec<f64> {
        self.confidence_history.lock().clone()
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adaptive_depth_controller_min_iterations() {
        let ctrl = AdaptiveDepthController::new(0.95, 0.01, 30, 3);
        assert!(ctrl.observe_iteration(0.5)); // iter 0 - continue
        assert!(ctrl.observe_iteration(0.6)); // iter 1 - continue
        assert!(ctrl.observe_iteration(0.7)); // iter 2 - continue (min not reached yet)
    }

    #[test]
    fn test_adaptive_depth_controller_target_reached() {
        let ctrl = AdaptiveDepthController::new(0.8, 0.01, 30, 2);
        assert!(ctrl.observe_iteration(0.5));
        assert!(ctrl.observe_iteration(0.6));
        assert!(!ctrl.observe_iteration(0.85)); // target reached
    }

    #[test]
    fn test_adaptive_depth_controller_max_iterations() {
        let ctrl = AdaptiveDepthController::new(0.99, 0.001, 5, 2);
        for i in 0..5 {
            let should = ctrl.observe_iteration(0.5 + (i as f64) * 0.05);
            if i < 4 {
                assert!(should, "iter {} should continue", i);
            } else {
                assert!(!should, "iter {} should stop", i);
            }
        }
    }

    #[test]
    fn test_adaptive_depth_controller_apply_config_runtime() {
        // 评审 A4 接线：&self 即可运行时重配（内部可变性），且立即生效。
        let ctrl = AdaptiveDepthController::default();
        ctrl.apply_config(0.6, 0.05);
        assert_eq!(*ctrl.target_confidence.read(), 0.6);
        assert_eq!(*ctrl.min_improvement.read(), 0.05);

        // 新目标 0.7 应在下一轮观察中直接生效。
        ctrl.observe_iteration(0.65);
        assert!(!ctrl.observe_iteration(0.71));
    }

    #[test]
    fn test_adaptive_depth_controller_reset_clears_history() {
        // P0-1a：循环入口重置共享控制器，防止跨运行状态污染。
        let ctrl = AdaptiveDepthController::new(0.99, 0.001, 5, 2);
        for _ in 0..5 {
            ctrl.observe_iteration(0.5);
        }
        assert!(!ctrl.observe_iteration(0.5)); // 已达硬上限
        ctrl.reset();
        // 重置后历史清空，同样的输入重新从 min_iterations 起步。
        assert!(ctrl.observe_iteration(0.5));
        assert!(ctrl.observe_iteration(0.5));
    }

    #[test]
    fn test_adaptive_depth_controller_convergence_stalled() {
        let ctrl = AdaptiveDepthController::new(0.99, 0.1, 30, 5);
        // 模拟平稳置信度
        for i in 0..6 {
            let should = ctrl.observe_iteration(0.65 + (i as f64) * 0.001);
            if i < 4 {
                assert!(should, "iter {} should continue (min)", i);
            }
            // 第 5 轮之后应观察到收敛停滞
        }
        // 连续 6 轮改进小于 0.1 后应停止
        assert!(!ctrl.observe_iteration(0.661));
    }

    #[tokio::test]
    async fn test_anomaly_detector_normal() {
        let detector = AnomalyDetector::default();
        let anomalies = detector.observe("node_a", 100, 50, true);
        assert!(
            anomalies.is_empty(),
            "First observation should not flag anomaly"
        );

        let anomalies = detector.observe("node_a", 110, 55, true);
        assert!(
            anomalies.is_empty(),
            "Similar values should not flag anomaly"
        );
    }

    #[tokio::test]
    async fn test_anomaly_detector_fuel_spike() {
        let detector = AnomalyDetector::new(1.5, 3.0, 0.3);
        // 建立基线
        for _ in 0..3 {
            detector.observe("node_a", 100, 50, true);
        }
        // 尖峰
        let anomalies = detector.observe("node_a", 500, 50, true);
        assert!(!anomalies.is_empty(), "Fuel spike should be detected");
        assert!(anomalies.iter().any(|a| a.metric == "fuel"));
    }

    #[tokio::test]
    async fn test_anomaly_detector_duration_spike() {
        let detector = AnomalyDetector::new(2.0, 2.0, 0.3);
        // 建立基线
        for _ in 0..3 {
            detector.observe("node_b", 100, 50, true);
        }
        // 耗时尖峰
        let anomalies = detector.observe("node_b", 100, 300, true);
        assert!(!anomalies.is_empty(), "Duration spike should be detected");
        assert!(anomalies.iter().any(|a| a.metric == "duration"));
    }

    #[tokio::test]
    async fn test_anomaly_detector_error_rate() {
        let detector = AnomalyDetector::default();
        detector.observe("node_c", 100, 50, true);
        detector.observe("node_c", 100, 50, false);
        detector.observe("node_c", 100, 50, false);
        detector.observe("node_c", 100, 50, false);
        let anomalies = detector.observe("node_c", 100, 50, false);
        let err_anomalies: Vec<_> = anomalies.iter().filter(|a| a.metric == "error_rate").collect();
        assert!(
            !err_anomalies.is_empty(),
            "High error rate should be detected"
        );
    }

    #[tokio::test]
    async fn test_anomaly_detector_report() {
        let detector = AnomalyDetector::default();
        for _ in 0..5 {
            detector.observe("node_x", 100, 50, false);
        }
        let report = detector.generate_report();
        assert!(!report.anomalies.is_empty() || report.should_extra_reflect);
    }

    #[test]
    fn test_anomaly_detector_with_history_window_clamps_to_one() {
        // history_window=0 必须被静默纠正为 1（避免后续除零）
        let det = AnomalyDetector::with_history_window(2.0, 3.0, 0.3, 0);
        assert_eq!(det.history_window, 1);
    }

    #[test]
    fn test_anomaly_detector_apply_config_overrides_thresholds() {
        let mut det = AnomalyDetector::default();
        // 默认阈值是 2.0/3.0/0.3
        assert!((det.fuel_threshold - 2.0).abs() < 1e-9);

        det.apply_config(5.0, 7.0, 0.1, 50);

        assert!((det.fuel_threshold - 5.0).abs() < 1e-9);
        assert!((det.duration_threshold - 7.0).abs() < 1e-9);
        assert!((det.error_rate_threshold - 0.1).abs() < 1e-9);
        assert_eq!(det.history_window, 50);
    }

    #[test]
    fn test_anomaly_detector_apply_config_preserves_history() {
        // 应用新阈值不应清零 EWMA 历史——保留观测趋势。
        let mut det = AnomalyDetector::new(2.0, 3.0, 0.3);
        for _ in 0..3 {
            det.observe("node_h", 100, 50, true);
        }
        // 取出历史快照：在 Arc<Mutex<HashMap>> 内部
        let history_before = det.history.lock().get("node_h").cloned();
        assert!(history_before.is_some(), "history should be populated");

        det.apply_config(4.0, 4.0, 0.5, 30);

        let history_after = det.history.lock().get("node_h").cloned();
        assert_eq!(
            history_before, history_after,
            "EWMA history must be preserved across apply_config"
        );
    }

    #[test]
    fn test_anomaly_detector_from_config_matches_with_history_window() {
        use sisyphus_types::config::AnomalyDetectorConfig;
        let cfg = AnomalyDetectorConfig {
            fuel_threshold: 1.5,
            duration_threshold: 2.5,
            error_rate_threshold: 0.4,
            history_window: 10,
        };
        let det = AnomalyDetector::from_config(&cfg);
        assert!((det.fuel_threshold - 1.5).abs() < 1e-9);
        assert!((det.duration_threshold - 2.5).abs() < 1e-9);
        assert!((det.error_rate_threshold - 0.4).abs() < 1e-9);
        assert_eq!(det.history_window, 10);
    }

    #[test]
    fn test_anomaly_detector_apply_config_clamps_history_window() {
        let mut det = AnomalyDetector::default();
        det.apply_config(2.0, 3.0, 0.3, 0);
        assert_eq!(
            det.history_window, 1,
            "apply_config must clamp history_window to ≥ 1"
        );
    }
}
