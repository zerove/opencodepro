//! StrategicManager 处理高层认知循环（预测 → 执行 → 反思 → 演进）。
//! 协调 IntelligenceManager 与 ExecutionManager 以达成收敛。
//!
//! 认知循环主体位于 [`cognitive_loop`] 子模块；本模块
//! 持有管理器状态、构建逻辑与支撑类型。

pub mod cognitive_loop;
pub use cognitive_loop::CognitiveEngine;

use parking_lot::{Mutex, RwLock};
use std::sync::Arc;

use sisyphus_intel::token_budget::TokenBudgetTracker;
use sisyphus_intel::{
    DeepReflector, FailurePatternLibrary, PlannerConfig, ProbabilisticPlanner, SimulationResult,
};
use sisyphus_types::config::CognitiveLoopConfig;
use sisyphus_types::{ExecutionTrace, SOPGraph};

use crate::cognitive_loop_v2::{AdaptiveDepthController, AnomalyDetector, AnomalyLog};
use crate::{
    execution_manager::ExecutionManager, CoreError, ExecutionContext, IntelligenceManager,
};

/// 认知执行循环中的单次迭代。
#[derive(Clone, Debug)]
pub struct CognitiveIteration {
    pub iteration: usize,
    pub prediction: SimulationResult,
    pub deviation_score: f64,
    pub suggestions: Vec<String>,
    pub evolved: bool,
}

/// 递归认知执行的完整报告。
#[derive(Debug, Clone)]
pub struct CognitiveReport {
    pub iterations: usize,
    pub converged: bool,
    pub final_deviation: f64,
    pub history: Vec<CognitiveIteration>,
    pub traces: Vec<ExecutionTrace>,
}

impl CognitiveReport {
    /// To mermaid.
    pub fn to_mermaid(&self) -> String {
        let mut mermaid = String::from("graph TD\n");
        for (i, trace) in self.traces.iter().enumerate() {
            let iter_label = format!("Iter{}", i + 1);
            mermaid.push_str(&format!("  subgraph {}[Iteration {}]\n", iter_label, i + 1));
            let mut prev_node: Option<String> = None;
            for point in &trace.points {
                let node_id = format!("{}_{}", iter_label, point.node_name);
                let node_label = format!("{}({:.2})", point.node_name, point.deviation_score);
                mermaid.push_str(&format!("    {}[\"{}\"]\n", node_id, node_label));
                if let Some(prev) = prev_node {
                    mermaid.push_str(&format!("    {} --> {}\n", prev, node_id));
                }
                prev_node = Some(node_id);
            }
            if i < self.traces.len() - 1 || !self.converged {
                if let Some(last_node) = prev_node {
                    mermaid.push_str(&format!(
                        "    {} -- PIVOT --> Evolution_{}\n",
                        last_node,
                        i + 1
                    ));
                }
            }
            mermaid.push_str("  end\n");
        }
        for i in 1..self.traces.len() {
            mermaid.push_str(&format!("  Evolution_{} --> Iter{}_Start\n", i, i + 1));
        }
        mermaid
    }
}

/// Strategic Manager.
pub struct StrategicManager {
    pub intelligence_manager: Arc<IntelligenceManager>,
    pub execution_manager: Arc<ExecutionManager>,
    pub config: RwLock<CognitiveLoopConfig>,
    pub channel: Arc<crate::AgentChannel>,
    pub benchmark: Arc<Mutex<crate::ContinuousBenchmark>>,
    pub meta_learner: Arc<Mutex<crate::MetaLearner>>,
    pub experience_bank: Arc<dyn sisyphus_types::experience::ExperienceBank>,
    pub pattern_analyzer: sisyphus_intel::pattern_analyzer::SemanticPatternAnalyzer,
    pub roadmap_manager: Arc<sisyphus_roadmap::RoadmapManager>,
    pub gap_analyzer: sisyphus_intel::GapAnalyzer,
    pub sp_manager: Arc<crate::self_programming_manager::SelfProgrammingManager>,
    pub intervention_manager: Arc<crate::intervention_manager::InterventionManager>,
    pub internalization_manager: Arc<crate::internalization_manager::InternalizationManager>,
    pub visual_manager: Option<Arc<sisyphus_visual::VisualPerceptionManager>>,
    /// Injected test runner. Previously `run_cognitive_loop` constructed a
    /// concrete one from `sisyphus-tools-executor`, which is why core had to
    /// depend on that crate in production for a role unrelated to tool
    /// execution. The kernel now hands down the instance it was given.
    pub test_executor: Arc<dyn sisyphus_types::TestExecutor>,
    /// 使用蒙特卡洛仿真结果的概率路径规划器
    pub probabilistic_planner: ProbabilisticPlanner,
    /// 用于针对性 SOP 演进的失败模式库
    pub failure_pattern_library: Arc<parking_lot::RwLock<FailurePatternLibrary>>,
    /// 用于多维偏差分析的深度反思器
    pub deep_reflector: Arc<parking_lot::RwLock<DeepReflector>>,
    /// 认知循环 v2：运行时异常检测
    pub anomaly_detector: Arc<Mutex<AnomalyDetector>>,
    /// 认知循环 v2：异常观测日志（`/api/anomaly` 与 gRPC 流的数据来源）。
    ///
    /// 与 `anomaly_detector` 分离：检测器是 per-run 的有状态基线，不能共享；
    /// 这条只追加的旁路日志才承载对外可观测性。
    pub anomaly_log: Arc<AnomalyLog>,
    /// 认知循环 v2：自适应深度控制
    pub depth_controller: AdaptiveDepthController,
    /// 反思/演进阶段 token 预算（review P2-8 step 2 接线）。
    /// 每次 `apply_cognitive_loop_config` 都会按 `cl.token_budget` 重置
    /// 上限/保留比例。`reflect_phase` / `evolve_phase` 入口通过
    /// `should_skip_llm()` 决定是否跳过 LLM 调用。
    pub token_budget: Arc<parking_lot::RwLock<TokenBudgetTracker>>,
}

impl StrategicManager {
    #[allow(clippy::too_many_arguments)]
    /// New.
    pub fn new(
        intelligence_manager: Arc<IntelligenceManager>,
        execution_manager: Arc<ExecutionManager>,
        config: CognitiveLoopConfig,
        channel: Arc<crate::AgentChannel>,
        benchmark: Arc<Mutex<crate::ContinuousBenchmark>>,
        meta_learner: Arc<Mutex<crate::MetaLearner>>,
        experience_bank: Arc<dyn sisyphus_types::experience::ExperienceBank>,
        knowledge_manager: Arc<sisyphus_knowledge::AssetManager>,
        roadmap_manager: Arc<sisyphus_roadmap::RoadmapManager>,
        intervention_manager: Arc<crate::intervention_manager::InterventionManager>,
        internalization_manager: Arc<crate::internalization_manager::InternalizationManager>,
        visual_manager: Option<Arc<sisyphus_visual::VisualPerceptionManager>>,
        test_executor: Arc<dyn sisyphus_types::TestExecutor>,
    ) -> Self {
        Self {
            intelligence_manager: intelligence_manager.clone(),
            execution_manager: execution_manager.clone(),
            config: RwLock::new(config),
            channel,
            benchmark,
            meta_learner,
            experience_bank,
            pattern_analyzer: sisyphus_intel::pattern_analyzer::SemanticPatternAnalyzer::new(3),
            roadmap_manager,
            gap_analyzer: sisyphus_intel::GapAnalyzer::new(intelligence_manager.llm_client.clone()),
            sp_manager: Arc::new(
                crate::self_programming_manager::SelfProgrammingManager::new(
                    intelligence_manager.llm_client.clone(),
                    execution_manager.clone(),
                    knowledge_manager.clone(),
                    None,
                ),
            ),
            intervention_manager,
            internalization_manager,
            visual_manager,
            test_executor,
            probabilistic_planner: ProbabilisticPlanner::new(PlannerConfig::default()),
            failure_pattern_library: Arc::new(parking_lot::RwLock::new(
                FailurePatternLibrary::new(),
            )),
            deep_reflector: Arc::new(parking_lot::RwLock::new(DeepReflector::new())),
            anomaly_detector: Arc::new(Mutex::new(AnomalyDetector::default())),
            anomaly_log: Arc::new(AnomalyLog::new()),
            depth_controller: AdaptiveDepthController::default(),
            token_budget: Arc::new(parking_lot::RwLock::new(TokenBudgetTracker::unbounded())),
        }
    }

    /// 应用认知循环配置（评审 A4 接线）。
    ///
    /// 覆盖循环基线参数，并把 `AdaptiveDepthConfig` 映射到运行时控制器：
    /// - `convergence_threshold`（置信度收敛阈值）→ `target_confidence = 1 - threshold`，
    ///   收紧到 [0.5, 1.0] 以防退化配置；
    /// - `min_improvement` → 同名字段。
    pub fn apply_cognitive_loop_config(&self, cl: CognitiveLoopConfig) {
        *self.config.write() = cl.clone();
        let ad = &cl.adaptive_depth;
        // 控制器可调字段为 RwLock 内部可变，&self 即可完成运行时重配。
        self.depth_controller.apply_config(
            (1.0 - ad.convergence_threshold).clamp(0.5, 1.0),
            ad.min_improvement,
        );

        // AnomalyDetector 阈值（review 7.1）：配置变化时同步覆盖。
        // 保留 `history` 缓冲以避免中断正在进行的 EWMA 跟踪。
        let an = &cl.anomaly_detector;
        self.anomaly_detector.lock().apply_config(
            an.fuel_threshold,
            an.duration_threshold,
            an.error_rate_threshold,
            an.history_window,
        );

        // 反思/演进阶段 token 预算（review P2-8 step 2 接线）。
        // 配置变化时重建 tracker：`*lock = new_tracker` 换掉的是锁内值，
        // 共享的 `Arc<RwLock<TokenBudgetTracker>>` 句柄不变，因此所有持有
        // 该句柄的调用方会立刻看到新的 cap/reserve。重建意味着累计 usage
        // 归零——这符合「重新应用配置 = 重新开始计量」的语义。
        let tb = &cl.token_budget;
        let new_tracker = match tb.cap {
            Some(cap) => TokenBudgetTracker::new(cap, tb.reserve_ratio),
            None => TokenBudgetTracker::unbounded(),
        };
        *self.token_budget.write() = new_tracker;

        let an_thresholds = {
            let d = self.anomaly_detector.lock();
            (
                d.fuel_threshold,
                d.duration_threshold,
                d.error_rate_threshold,
            )
        };
        tracing::info!(
            max_iterations = cl.max_iterations,
            convergence_threshold = cl.convergence_threshold,
            initial_budget = ad.initial_budget,
            decay = ad.decay,
            anomaly_fuel_threshold = an_thresholds.0,
            anomaly_duration_threshold = an_thresholds.1,
            anomaly_error_rate_threshold = an_thresholds.2,
            token_budget_cap = ?tb.cap,
            token_budget_reserve = tb.reserve_ratio,
            "Cognitive loop config applied"
        );
    }

    /// 记录一次 LLM 阶段的 token 消耗。
    ///
    /// `LlmClient` 只返回文本，不透出 provider 报的 `usage` 字段，因此这里
    /// 按「实际 payload 字节数 / 4」估算（UTF-8 下中英文混排大致落在
    /// 3-5 字节/token）；payload 在调用方不可见时退回
    /// [`sisyphus_intel::token_budget::DEFAULT_ESTIMATED_TOKENS_PER_CALL`]。
    ///
    /// 这不是精确计费，但它让 `should_skip_llm()` 的预算语义真正生效：
    /// 在此之前生产路径从不调用 `record()`，`usage` 恒为 0，有限的 cap
    /// 永远触发不了跳过，整个预算配置是死配置。
    pub(crate) fn record_llm_stage_usage(&self, payload_bytes: Option<usize>) {
        let tokens = match payload_bytes {
            Some(bytes) => (bytes as u64 / 4).max(1),
            None => sisyphus_intel::token_budget::DEFAULT_ESTIMATED_TOKENS_PER_CALL,
        };
        self.token_budget.read().record(tokens);
    }

    fn build_cog_map(
        sop_id: &str,
        goal_text: &str,
        graph: &SOPGraph,
        ctx: &ExecutionContext,
        iteration: usize,
        active_node: Option<String>,
    ) -> Result<sisyphus_types::CogMap, CoreError> {
        let mut nodes = Vec::new();
        for node_idx in graph.graph.node_indices() {
            let nw = graph
                .graph
                .node_weight(node_idx)
                .ok_or_else(|| CoreError::Internal("Node weight missing".to_string()))?;
            let name = nw.name();

            let status = if let Some(out) = ctx.get_result(name) {
                if out.indicates_failure() {
                    "error".into()
                } else {
                    "ok".into()
                }
            } else if Some(name.to_string()) == active_node {
                "running".into()
            } else {
                "pending".into()
            };

            // P2-9：从执行轨迹回填节点偏差真值（评审 B4：此前恒为 0）。
            // 轨迹点由运行时监督器逐节点记录，取该节点最近一次的
            // deviation_score；未执行或无记录的节点保持 0.0。
            let deviation = ctx
                .trace
                .read()
                .map(|trace| {
                    trace
                        .iter()
                        .rev()
                        .find(|p| p.node_name == name)
                        .map(|p| p.deviation_score)
                        .unwrap_or(0.0)
                })
                .unwrap_or(0.0);

            nodes.push(sisyphus_types::CogNode {
                id: node_idx.index().to_string(),
                label: name.to_string(),
                status,
                deviation,
                position: (0.0, 0.0), // Frontend will handle layout (e.g., Dagre)
            });
        }

        let mut edges = Vec::new();
        for edge_idx in graph.graph.edge_indices() {
            if let Some((source, target)) = graph.graph.edge_endpoints(edge_idx) {
                edges.push(sisyphus_types::CogEdge {
                    source: source.index().to_string(),
                    target: target.index().to_string(),
                    label: None,
                });
            }
        }

        Ok(sisyphus_types::CogMap {
            sop_id: sop_id.to_string(),
            goal_text: goal_text.to_string(),
            nodes,
            edges,
            active_node,
            iteration,
        })
    }
}
