//! 认知循环 v2 — 自适应深度、并行分支评估与运行时异常检测。
//!
//! ## 相对 v1 的增强
//! - **ParallelBranchPredictor**：并发运行多个仿真/执行分支，
//!   根据成功概率、成本与稳定性选出最优结果。
//! - **AnomalyDetector**：监控每个节点的执行指标（燃料、耗时、错误率），
//!   检测到异常时触发内联反思。
//! - **AdaptiveDepthController**：基于置信度与收敛速度决定是否
//!   继续迭代，而不是使用固定的 max_iterations 上限。

use crate::CoreError;
use parking_lot::Mutex;
use sisyphus_intel::simulation::{LogicalSimulationResult, SimulationResult};
use sisyphus_intel::ComplexityAnalyzer;
use sisyphus_types::experience::ExperienceBank;
use sisyphus_types::ExecutionContext;
use sisyphus_types::SOPGraph;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, instrument, warn};

// ---------------------------------------------------------------------------
// ParallelBranchPredictor（并行分支预测器）
// ---------------------------------------------------------------------------

/// 单个已评估分支及其得分。
#[derive(Debug, Clone)]
pub struct BranchEvaluation {
    pub branch_id: String,
    pub simulation: SimulationResult,
    pub logical_result: Option<LogicalSimulationResult>,
    pub confidence: f64,
    pub cost: u64,
}

impl BranchEvaluation {
    /// 综合得分：越高越好。
    /// 综合平衡成功概率、成本效率与逻辑有效性。
    pub fn composite_score(&self) -> f64 {
        let success_weight = 0.5;
        let cost_weight = 0.3;
        let stability_weight = 0.2;

        let logical_penalty = self
            .logical_result
            .as_ref()
            .map(|lr| {
                if lr.is_valid {
                    0.0
                } else {
                    let critical_count = lr
                        .flaws
                        .iter()
                        .filter(|f| {
                            matches!(
                                f.severity,
                                sisyphus_intel::simulation::FlawSeverity::Critical
                            )
                        })
                        .count() as f64;
                    critical_count * 0.2
                }
            })
            .unwrap_or(0.0);

        let success_score = self.simulation.success_probability * success_weight;
        let cost_efficiency = if self.cost > 0 {
            (1.0 / (self.cost as f64).max(1.0).ln_1p()) * cost_weight
        } else {
            0.0
        };
        let stability_score = self.simulation.predicted_stability * stability_weight;

        (success_score + cost_efficiency + stability_score) - logical_penalty
    }
}

/// 并发运行多个仿真并选择最佳路径。
pub struct ParallelBranchPredictor {
    /// 可并行评估的最大分支数。
    pub max_branches: usize,
    /// 分支仿真的并发上限。
    pub concurrency: usize,
}

impl Default for ParallelBranchPredictor {
    fn default() -> Self {
        Self {
            max_branches: 3,
            concurrency: 3,
        }
    }
}

impl ParallelBranchPredictor {
    pub fn new(max_branches: usize, concurrency: usize) -> Self {
        Self {
            max_branches,
            concurrency,
        }
    }

    /// 对同一图谱的多个变体运行并行仿真。
    /// `variant_graphs` 应包含不同的 SOP 图谱（例如演进变体、原始图谱）。
    /// 返回最佳分支评估以及全部评估结果供分析。
    #[allow(clippy::too_many_arguments)]
    #[instrument(skip_all, fields(num_variants = variant_graphs.len()))]
    pub async fn evaluate_branches(
        &self,
        variant_graphs: Vec<Arc<SOPGraph>>,
        ctx: Arc<ExecutionContext>,
        project_id: &str,
        sop_id: &str,
        goal_text: Option<&str>,
        simulator: Arc<dyn sisyphus_intel::Simulator>,
        logical_simulator: Arc<dyn sisyphus_intel::simulation::LogicalSimulator>,
        experience_bank: Arc<dyn ExperienceBank>,
    ) -> Result<(BranchEvaluation, Vec<BranchEvaluation>), CoreError> {
        if variant_graphs.is_empty() {
            return Err(CoreError::Internal(
                "No variant graphs provided for parallel evaluation".into(),
            ));
        }

        // 限制分支数量
        let graphs: Vec<_> = variant_graphs.into_iter().take(self.max_branches).collect();

        let num_branches = graphs.len();
        let mut handles = Vec::with_capacity(num_branches);

        for (i, graph) in graphs.into_iter().enumerate() {
            let branch_id = format!("branch_{}", i);
            let project_id = project_id.to_string();
            let sop_id = sop_id.to_string();
            let goal_text = goal_text.map(|s| s.to_string());
            let ctx_ref = ctx.clone();
            let sim_clone = simulator.clone();
            let log_sim_clone = logical_simulator.clone();

            handles.push(tokio::spawn(async move {
                // 仿真
                let sim_result = sim_clone
                    .simulate(
                        Arc::clone(&graph),
                        &ctx_ref,
                        &project_id,
                        &sop_id,
                        goal_text.as_deref(),
                    )
                    .await;

                // 逻辑仿真
                let log_result = log_sim_clone
                    .simulate_logic(Arc::clone(&graph), &ctx_ref, &sop_id, goal_text.as_deref())
                    .await;

                let sim = sim_result.map_err(|e| CoreError::Internal(e.to_string()))?;
                let log = log_result.ok();

                let cost_estimate = sim.estimated_cost;

                Ok::<_, CoreError>(BranchEvaluation {
                    branch_id,
                    simulation: sim,
                    logical_result: log,
                    confidence: 0.0, // will be computed
                    cost: cost_estimate,
                })
            }));
        }

        // 收集结果
        let mut evaluations = Vec::with_capacity(num_branches);
        for handle in handles {
            match handle.await {
                Ok(Ok(eval)) => evaluations.push(eval),
                Ok(Err(e)) => warn!("Branch evaluation failed: {}", e),
                Err(e) => warn!("Branch evaluation task panicked: {}", e),
            }
        }

        if evaluations.is_empty() {
            return Err(CoreError::Internal("All branch evaluations failed".into()));
        }

        // 使用经验库计算每个分支的置信度
        for eval in &mut evaluations {
            let query = sisyphus_types::experience::ExperienceQuery {
                query_text: format!("{} {} cost={}", sop_id, eval.branch_id, eval.cost),
                context_hash: String::new(),
                project_id: Some(project_id.to_string()),
                action_id: Some(eval.branch_id.clone()),
                visual_embedding: None,
                limit: 5,
            };

            let confidence = if let Ok(exps) = experience_bank.query_similar(query).await {
                if exps.is_empty() {
                    0.5 // neutral default
                } else {
                    let successes = exps.iter().filter(|e| e.outcome_success).count();
                    successes as f64 / exps.len() as f64
                }
            } else {
                0.5
            };
            eval.confidence = confidence;
        }

        // 按综合得分选出最优
        let best = evaluations
            .iter()
            .max_by(|a, b| {
                a.composite_score()
                    .partial_cmp(&b.composite_score())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned()
            .ok_or_else(|| CoreError::Internal("No evaluable branch found".into()))?;

        info!(
            branch_id = %best.branch_id,
            score = best.composite_score(),
            confidence = best.confidence,
            cost = best.cost,
            "ParallelBranchPredictor: selected best branch"
        );

        Ok((best, evaluations))
    }
}

// ---------------------------------------------------------------------------
// AnomalyDetector（异常检测器）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnomalySeverity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone)]
pub struct NodeAnomaly {
    pub node_name: String,
    pub severity: AnomalySeverity,
    pub metric: String,
    pub observed: f64,
    pub expected: f64,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct AnomalyReport {
    pub anomalies: Vec<NodeAnomaly>,
    pub should_extra_reflect: bool,
    pub overall_severity: AnomalySeverity,
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
    pub fn new(fuel_threshold: f64, duration_threshold: f64, error_rate_threshold: f64) -> Self {
        Self {
            history: Arc::new(Mutex::new(HashMap::new())),
            fuel_threshold,
            duration_threshold,
            error_rate_threshold,
            history_window: 20,
            total_executions: Arc::new(Mutex::new(0)),
        }
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

        let (ref mut avg_fuel, ref mut avg_dur, ref mut err_count) = history
            .entry(node_name.to_string())
            .or_insert((0.0, 0.0, 0));

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
        let mut total = self.total_executions.lock();
        *total = 0;
    }
}

// ---------------------------------------------------------------------------
// AdaptiveDepthController（自适应深度控制器）
// ---------------------------------------------------------------------------

/// 基于收敛速度与置信度控制认知循环深度。
/// 不使用固定 max_iterations，而是采用目标置信度与最小改进量。
pub struct AdaptiveDepthController {
    /// 停止前要达到的目标置信度（0.0 - 1.0）。
    pub target_confidence: f64,
    /// 继续迭代所需的相邻轮次最小改进量。
    pub min_improvement: f64,
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
            target_confidence: 0.95,
            min_improvement: 0.01,
            max_iterations: 30,
            min_iterations: 2,
            convergence_window: 3,
            confidence_history: Arc::new(Mutex::new(Vec::with_capacity(30))),
        }
    }
}

impl AdaptiveDepthController {
    pub fn new(
        target_confidence: f64,
        min_improvement: f64,
        max_iterations: usize,
        min_iterations: usize,
    ) -> Self {
        Self {
            target_confidence,
            min_improvement,
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
    ) -> usize {
        let depth = ComplexityAnalyzer::analyze(experience_bank, sop_graph, goal_text).await;
        let (d_max, _) = depth.to_config();
        (d_max as usize)
            .min(self.max_iterations)
            .max(self.min_iterations)
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
        if confidence >= self.target_confidence {
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

            if improvement.abs() < self.min_improvement {
                info!(
                    "AdaptiveDepthController: convergence stalled (improvement={:.4} < min={:.4})",
                    improvement, self.min_improvement
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

    #[test]
    fn test_parallel_branch_predictor_composite_score() {
        let eval = BranchEvaluation {
            branch_id: "test".into(),
            simulation: SimulationResult {
                path_id: "path".into(),
                success_probability: 0.8,
                estimated_cost: 1000,
                predicted_stability: 0.9,
                predicted_effort: 50,
                predicted_outcome: "success".into(),
            },
            logical_result: None,
            confidence: 0.7,
            cost: 1000,
        };
        let score = eval.composite_score();
        assert!(score > 0.0, "Score should be positive, got {}", score);
        assert!(score < 2.0, "Score should be reasonable, got {}", score);
    }

    #[test]
    fn test_parallel_branch_predictor_logical_penalty() {
        let log_result = LogicalSimulationResult {
            is_valid: false,
            flaws: vec![sisyphus_intel::simulation::LogicalFlaw {
                node_name: "test".into(),
                description: "Critical flaw".into(),
                severity: sisyphus_intel::simulation::FlawSeverity::Critical,
            }],
            predicted_path: vec![],
            confidence: 1.0,
        };

        let eval_with_flaw = BranchEvaluation {
            branch_id: "flawed".into(),
            simulation: SimulationResult {
                path_id: "path".into(),
                success_probability: 0.8,
                estimated_cost: 500,
                predicted_stability: 0.9,
                predicted_effort: 25,
                predicted_outcome: "success".into(),
            },
            logical_result: Some(log_result),
            confidence: 0.7,
            cost: 500,
        };

        let eval_clean = BranchEvaluation {
            branch_id: "clean".into(),
            simulation: SimulationResult {
                path_id: "path".into(),
                success_probability: 0.75,
                estimated_cost: 500,
                predicted_stability: 0.85,
                predicted_effort: 25,
                predicted_outcome: "success".into(),
            },
            logical_result: None,
            confidence: 0.7,
            cost: 500,
        };

        // 干净的分支尽管成功概率略低，综合得分应更高
        assert!(
            eval_clean.composite_score() > eval_with_flaw.composite_score(),
            "Clean branch should outscore flawed branch despite logical flaws"
        );
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
        let err_anomalies: Vec<_> = anomalies
            .iter()
            .filter(|a| a.metric == "error_rate")
            .collect();
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
}
