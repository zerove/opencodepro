//! 路线图数据类型 — 进度跟踪、执行结果、健康评估。

use sisyphus_types::MilestoneStatus;

// ---------------------------------------------------------------------------
// 阶段 2 — 进度跟踪类型
// ---------------------------------------------------------------------------

/// 逐目标进度详情。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GoalProgress {
    pub goal_id: String,
    pub description: String,
    pub status: MilestoneStatus,
}

/// 逐阶段进度明细。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PhaseProgress {
    pub phase_id: String,
    pub name: String,
    pub goal_count: usize,
    pub completed_count: usize,
    pub progress: f64,
    pub goals: Vec<GoalProgress>,
}

/// 逐里程碑进度明细。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MilestoneProgress {
    pub milestone_id: String,
    pub name: String,
    pub status: MilestoneStatus,
    pub phase_count: usize,
    pub goal_count: usize,
    pub completed_count: usize,
    pub progress: f64,
    pub phases: Vec<PhaseProgress>,
}

/// 项目的完整进度报告。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProgressReport {
    pub project_id: String,
    pub project_name: String,
    pub overall_progress: f64,
    pub milestone_count: usize,
    pub total_goals: usize,
    pub completed_goals: usize,
    pub milestones: Vec<MilestoneProgress>,
}

// ---------------------------------------------------------------------------
// 阶段 2 — 动态路线图调整类型
// ---------------------------------------------------------------------------

/// 单次目标执行的结果。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExecutionOutcome {
    pub goal_id: String,
    pub project_id: String,
    pub milestone_id: String,
    pub phase_id: String,
    pub success: bool,
    pub deviation_score: Option<f64>,
    pub error: Option<String>,
    pub timestamp: u64,
    pub actual_cost: f64,
    pub duration_secs: f64,
}

/// 路线图中检测到问题的严重程度。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum ProblemSeverity {
    Warning,
    Critical,
}

/// 路线图中检测到的问题。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RoadmapProblem {
    pub severity: ProblemSeverity,
    pub message: String,
    pub milestone_id: Option<String>,
    pub phase_id: Option<String>,
    pub goal_id: Option<String>,
}

/// 解决路线图问题的建议调整。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdjustmentSuggestion {
    pub problem: String,
    pub action: String,
    pub target_goal_id: Option<String>,
}

/// 逐里程碑完成预测。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MilestonePrediction {
    pub name: String,
    pub progress_pct: f64,
    pub remaining_goals: usize,
    pub risk_level: String,
}

/// 项目的总体完成预测。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProgressPrediction {
    pub project_id: String,
    pub project_name: String,
    pub overall_progress_pct: f64,
    pub remaining_goals: usize,
    pub milestone_predictions: Vec<MilestonePrediction>,
    pub estimated_completion_order: Vec<String>,
    pub risk_level: String,
}

/// 带置信区间的逐里程碑时间预测。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MilestoneTimePrediction {
    pub milestone_id: String,
    pub name: String,
    /// 预计剩余秒数（均值）。
    pub estimated_remaining_secs: f64,
    /// 95% 置信区间下界（秒）。
    pub ci_lower_secs: f64,
    /// 95% 置信区间上界（秒）。
    pub ci_upper_secs: f64,
    /// 当前进度百分比。
    pub progress_pct: f64,
    /// 剩余目标数。
    pub remaining_goals: usize,
    /// 风险级别："low"、"medium"、"high"、"done"。
    pub risk_level: String,
    /// 历史速度（目标/秒）。
    pub velocity_goals_per_sec: f64,
}

/// 带置信区间的项目级时间完成预测。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompletionTimePrediction {
    pub project_id: String,
    pub project_name: String,
    pub overall_progress_pct: f64,
    pub remaining_goals: usize,
    pub estimated_total_remaining_secs: f64,
    pub ci_lower_total_secs: f64,
    pub ci_upper_total_secs: f64,
    pub milestone_predictions: Vec<MilestoneTimePrediction>,
    pub estimated_completion_order: Vec<String>,
    pub risk_level: String,
    /// 总体速度（目标/秒）。
    pub overall_velocity_goals_per_sec: f64,
    /// 预测是否基于充足的历史数据。
    pub has_sufficient_data: bool,
}

/// 项目路线图的健康评估结果。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RoadmapHealth {
    pub project_id: String,
    pub healthy: bool,
    pub blocked_milestones: Vec<String>,
    pub stale_goals: Vec<String>,
    pub recent_failures: usize,
    pub problems: Vec<RoadmapProblem>,
    pub suggestions: Vec<AdjustmentSuggestion>,
}

/// 有重大延迟风险的里程碑的延迟告警。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DelayAlert {
    pub project_id: String,
    pub milestone_id: String,
    pub milestone_name: String,
    pub estimated_remaining_secs: f64,
    pub ci_upper_secs: f64,
    pub threshold_multiplier: f64,
    pub severity: String, // "warning" or "critical"
    pub message: String,
}

// ---------------------------------------------------------------------------
// 阶段 3 — 资源优化类型
// ---------------------------------------------------------------------------

/// 项目剩余工作的成本预测。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CostProjection {
    pub total_goals: usize,
    pub completed_goals: usize,
    pub remaining_goals: usize,
    pub avg_cost_per_goal: f64,
    pub avg_duration_per_goal_secs: f64,
    pub projected_remaining_cost: f64,
    pub projected_remaining_duration_secs: f64,
    pub confidence: f64,
}

/// 执行流水线中检测到的瓶颈。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Bottleneck {
    pub goal_id: String,
    pub description: String,
    pub failure_count: usize,
    pub avg_duration_secs: f64,
    pub blocking_count: usize,
    pub severity: String, // "low", "medium", "high"
}

/// 并行执行目标的机会。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParallelizationOpportunity {
    pub goal_ids: Vec<String>,
    pub rationale: String,
    pub estimated_time_savings_secs: f64,
}

/// 逐目标资源估算。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GoalResourceEstimate {
    pub goal_id: String,
    pub predicted_cost: f64,
    pub predicted_duration_secs: f64,
    pub confidence: f64,
    pub historical_samples: usize,
}
