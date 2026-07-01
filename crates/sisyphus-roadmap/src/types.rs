//! Roadmap data types — progress tracking, execution outcomes, health evaluation.

use sisyphus_types::MilestoneStatus;

// ---------------------------------------------------------------------------
// Phase 2 — Progress tracking types
// ---------------------------------------------------------------------------

/// Per-goal progress detail.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GoalProgress {
    pub goal_id: String,
    pub description: String,
    pub status: MilestoneStatus,
}

/// Per-phase progress breakdown.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PhaseProgress {
    pub phase_id: String,
    pub name: String,
    pub goal_count: usize,
    pub completed_count: usize,
    pub progress: f64,
    pub goals: Vec<GoalProgress>,
}

/// Per-milestone progress breakdown.
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

/// Full progress report for a project.
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
// Phase 2 — Dynamic roadmap adjustment types
// ---------------------------------------------------------------------------

/// Outcome of a single goal execution.
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

/// Severity of a detected problem in the roadmap.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum ProblemSeverity {
    Warning,
    Critical,
}

/// A detected problem within the roadmap.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RoadmapProblem {
    pub severity: ProblemSeverity,
    pub message: String,
    pub milestone_id: Option<String>,
    pub phase_id: Option<String>,
    pub goal_id: Option<String>,
}

/// Suggested adjustment to address a roadmap problem.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdjustmentSuggestion {
    pub problem: String,
    pub action: String,
    pub target_goal_id: Option<String>,
}

/// Per-milestone completion prediction.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MilestonePrediction {
    pub name: String,
    pub progress_pct: f64,
    pub remaining_goals: usize,
    pub risk_level: String,
}

/// Overall completion prediction for a project.
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

/// Health evaluation result for a project's roadmap.
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
