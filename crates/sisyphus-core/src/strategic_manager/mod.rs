//! StrategicManager 处理高层认知循环（预测 → 执行 → 反思 → 演进）。
//! 协调 IntelligenceManager 与 ExecutionManager 以达成收敛。
//!
//! 认知循环主体位于 [`cognitive_loop`] 子模块；本模块
//! 持有管理器状态、构建逻辑与支撑类型。

mod cognitive_loop;

use parking_lot::{Mutex, RwLock};
use std::sync::Arc;

use sisyphus_intel::{
    DeepReflector, FailurePatternLibrary, PlannerConfig, ProbabilisticPlanner, SimulationResult,
};
use sisyphus_types::config::CognitiveLoopConfig;
use sisyphus_types::{ExecutionTrace, SOPGraph};

use crate::cognitive_loop_v2::{AdaptiveDepthController, AnomalyDetector};
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
    /// 使用蒙特卡洛仿真结果的概率路径规划器
    pub probabilistic_planner: ProbabilisticPlanner,
    /// 用于针对性 SOP 演进的失败模式库
    pub failure_pattern_library: Arc<parking_lot::RwLock<FailurePatternLibrary>>,
    /// 用于多维偏差分析的深度反思器
    pub deep_reflector: Arc<parking_lot::RwLock<DeepReflector>>,
    /// 认知循环 v2：运行时异常检测
    pub anomaly_detector: Arc<AnomalyDetector>,
    /// 认知循环 v2：自适应深度控制
    pub depth_controller: AdaptiveDepthController,
}

impl StrategicManager {
    #[allow(clippy::too_many_arguments)]
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
            probabilistic_planner: ProbabilisticPlanner::new(PlannerConfig::default()),
            failure_pattern_library: Arc::new(parking_lot::RwLock::new(
                FailurePatternLibrary::new(),
            )),
            deep_reflector: Arc::new(parking_lot::RwLock::new(DeepReflector::new())),
            anomaly_detector: Arc::new(AnomalyDetector::default()),
            depth_controller: AdaptiveDepthController::default(),
        }
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
                if out.to_string().contains("Error") || out.to_string().contains("Failed") {
                    "error".into()
                } else {
                    "ok".into()
                }
            } else if Some(name.to_string()) == active_node {
                "running".into()
            } else {
                "pending".into()
            };

            let deviation = 0.0; // In a real system, we'd track per-node deviation from the simulator

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
