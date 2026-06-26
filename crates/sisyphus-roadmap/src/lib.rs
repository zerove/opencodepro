pub mod milestone_generator;
pub mod resource_optimizer;
pub mod strategic_reflection;

use serde_json::Value;
use sisyphus_mem::MfsStore;
use sisyphus_types::{
    Milestone, MilestoneStatus, Phase, Project, Roadmap, GoalRef, StateStore,
};
use std::sync::Arc;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Error, Debug)]
pub enum RoadmapError {
    #[error("Project '{0}' not found")]
    ProjectNotFound(String),
    #[error("Milestone '{0}' not found in project '{1}'")]
    MilestoneNotFound(String, String),
    #[error("Phase '{0}' not found in milestone '{1}'")]
    PhaseNotFound(String, String),
    #[error("Goal '{0}' not found in phase '{1}'")]
    GoalNotFound(String, String),
    #[error("Duplicate project id: '{0}' already exists")]
    DuplicateProject(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Persistence error: {0}")]
    Persistence(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<serde_json::Error> for RoadmapError {
    fn from(e: serde_json::Error) -> Self {
        RoadmapError::Serialization(e.to_string())
    }
}

impl From<String> for RoadmapError {
    fn from(e: String) -> Self {
        RoadmapError::Persistence(e)
    }
}

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

// ---------------------------------------------------------------------------
// Key conventions
// ---------------------------------------------------------------------------

/// Key storing the index of all project IDs.
const PROJECTS_INDEX_KEY: &str = "roadmap:projects";

/// Build a key for a specific project.
fn project_key(id: &str) -> String {
    format!("roadmap:project:{}", id)
}

/// Build an outcome index key for a project (list of outcome IDs).
fn outcome_index_key(project_id: &str) -> String {
    format!("roadmap:outcomes:{}", project_id)
}

/// Build an individual outcome key.
fn outcome_key(project_id: &str, milestone_id: &str, phase_id: &str, goal_id: &str) -> String {
    format!("roadmap:outcome:{}:{}:{}:{}", project_id, milestone_id, phase_id, goal_id)
}

// ---------------------------------------------------------------------------
// RoadmapManager
// ---------------------------------------------------------------------------

/// Strategic planning manager that provides CRUD operations over a
/// Project → Milestone → Phase → Goal hierarchy, persisted via MfsStore.
///
/// # Key conventions
/// - `roadmap:projects` — JSON array of all project IDs (index).
/// - `roadmap:project:{id}` — JSON-serialised `Project` (single source of truth).
pub struct RoadmapManager {
    store: Arc<MfsStore>,
}

impl RoadmapManager {
    /// Create a new `RoadmapManager` backed by the given `MfsStore`.
    pub fn new(store: Arc<MfsStore>) -> Self {
        Self { store }
    }

    // ── Project CRUD ────────────────────────────────────────────────

    /// Create a new project and persist it.
    ///
    /// Returns an error if a project with the same `id` already exists.
    pub async fn create_project(
        &self,
        id: &str,
        name: &str,
        description: &str,
    ) -> Result<Project, RoadmapError> {
        let key = project_key(id);

        // Check for duplicate
        if let Some(_existing) = self.store.get_state_json(&key).await? {
            return Err(RoadmapError::DuplicateProject(id.to_string()));
        }

        let project = Project::new(id.to_string(), name.to_string(), description.to_string());
        let value = serde_json::to_value(&project)?;
        self.store.set_state_json(&key, value).await?;

        // Update project index
        self.add_to_index(id).await?;

        Ok(project)
    }

    /// Retrieve a project by its ID.
    pub async fn get_project(&self, id: &str) -> Result<Option<Project>, RoadmapError> {
        let key = project_key(id);
        let value = self.store.get_state_json(&key).await?;
        match value {
            Some(v) if !v.is_null() => Ok(Some(serde_json::from_value(v)?)),
            _ => Ok(None),
        }
    }

    /// Update an existing project (replaces the full document).
    pub async fn update_project(&self, project: &Project) -> Result<(), RoadmapError> {
        let key = project_key(&project.id);

        // Verify it exists
        if self.store.get_state_json(&key).await?.is_none() {
            return Err(RoadmapError::ProjectNotFound(project.id.clone()));
        }

        let value = serde_json::to_value(project)?;
        self.store.set_state_json(&key, value).await?;
        Ok(())
    }

    /// Delete a project by its ID.
    pub async fn delete_project(&self, id: &str) -> Result<(), RoadmapError> {
        let key = project_key(id);

        if self.store.get_state_json(&key).await?.is_none() {
            return Err(RoadmapError::ProjectNotFound(id.to_string()));
        }

        // Overwrite with Null to effectively delete (MfsStore doesn't expose Delete)
        self.store.set_state_json(&key, Value::Null).await?;
        self.remove_from_index(id).await?;

        Ok(())
    }

    /// List all project IDs.
    pub async fn list_project_ids(&self) -> Result<Vec<String>, RoadmapError> {
        let index = self.load_index().await?;
        Ok(index)
    }

    /// List all full projects.
    pub async fn list_projects(&self) -> Result<Vec<Project>, RoadmapError> {
        let ids = self.list_project_ids().await?;
        let mut projects = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(project) = self.get_project(&id).await? {
                projects.push(project);
            }
        }
        Ok(projects)
    }

    // ── Milestone operations ────────────────────────────────────────

    /// Add a milestone to a project.
    pub async fn add_milestone(
        &self,
        project_id: &str,
        milestone: Milestone,
    ) -> Result<(), RoadmapError> {
        let mut project = self.get_project_or_err(project_id).await?;
        project.roadmap.milestones.push(milestone);
        self.update_project(&project).await
    }

    /// Update the status of a milestone within a project.
    pub async fn update_milestone_status(
        &self,
        project_id: &str,
        milestone_id: &str,
        status: MilestoneStatus,
    ) -> Result<(), RoadmapError> {
        let mut project = self.get_project_or_err(project_id).await?;
        let milestone = project
            .roadmap
            .milestones
            .iter_mut()
            .find(|m| m.id == milestone_id)
            .ok_or_else(|| {
                RoadmapError::MilestoneNotFound(milestone_id.to_string(), project_id.to_string())
            })?;
        milestone.status = status;
        self.update_project(&project).await
    }

    /// Remove a milestone from a project.
    pub async fn remove_milestone(
        &self,
        project_id: &str,
        milestone_id: &str,
    ) -> Result<(), RoadmapError> {
        let mut project = self.get_project_or_err(project_id).await?;
        let len_before = project.roadmap.milestones.len();
        project.roadmap.milestones.retain(|m| m.id != milestone_id);
        if project.roadmap.milestones.len() == len_before {
            return Err(RoadmapError::MilestoneNotFound(
                milestone_id.to_string(),
                project_id.to_string(),
            ));
        }
        self.update_project(&project).await
    }

    // ── Phase operations ────────────────────────────────────────────

    /// Add a phase to a milestone within a project.
    pub async fn add_phase(
        &self,
        project_id: &str,
        milestone_id: &str,
        phase: Phase,
    ) -> Result<(), RoadmapError> {
        let mut project = self.get_project_or_err(project_id).await?;
        let milestone = project
            .roadmap
            .milestones
            .iter_mut()
            .find(|m| m.id == milestone_id)
            .ok_or_else(|| {
                RoadmapError::MilestoneNotFound(milestone_id.to_string(), project_id.to_string())
            })?;
        milestone.phases.push(phase);
        self.update_project(&project).await
    }

    // ── Goal operations ─────────────────────────────────────────────

    /// Add a goal to a phase within a milestone.
    pub async fn add_goal(
        &self,
        project_id: &str,
        milestone_id: &str,
        phase_id: &str,
        goal: GoalRef,
    ) -> Result<(), RoadmapError> {
        let mut project = self.get_project_or_err(project_id).await?;
        let milestone = project
            .roadmap
            .milestones
            .iter_mut()
            .find(|m| m.id == milestone_id)
            .ok_or_else(|| {
                RoadmapError::MilestoneNotFound(milestone_id.to_string(), project_id.to_string())
            })?;
        let phase = milestone
            .phases
            .iter_mut()
            .find(|p| p.id == phase_id)
            .ok_or_else(|| {
                RoadmapError::PhaseNotFound(phase_id.to_string(), milestone_id.to_string())
            })?;
        phase.goals.push(goal);
        self.update_project(&project).await
    }

    /// Update the status of a specific goal.
    pub async fn update_goal_status(
        &self,
        project_id: &str,
        milestone_id: &str,
        phase_id: &str,
        goal_id: &str,
        status: MilestoneStatus,
    ) -> Result<(), RoadmapError> {
        let mut project = self.get_project_or_err(project_id).await?;
        let milestone = project
            .roadmap
            .milestones
            .iter_mut()
            .find(|m| m.id == milestone_id)
            .ok_or_else(|| {
                RoadmapError::MilestoneNotFound(milestone_id.to_string(), project_id.to_string())
            })?;
        let phase = milestone
            .phases
            .iter_mut()
            .find(|p| p.id == phase_id)
            .ok_or_else(|| {
                RoadmapError::PhaseNotFound(phase_id.to_string(), milestone_id.to_string())
            })?;
        let goal = phase
            .goals
            .iter_mut()
            .find(|g| g.goal_id == goal_id)
            .ok_or_else(|| {
                RoadmapError::GoalNotFound(goal_id.to_string(), phase_id.to_string())
            })?;
        goal.status = status;
        self.update_project(&project).await
    }

    // ── Progress ────────────────────────────────────────────────────

    /// Calculate overall progress for a project as a ratio [0.0, 1.0].
    ///
    /// Progress is computed from all milestones weighted equally:
    /// each milestone's progress contribution is the fraction of completed
    /// goals within that milestone.
    pub async fn calculate_progress(&self, project_id: &str) -> Result<f64, RoadmapError> {
        let project = self.get_project_or_err(project_id).await?;
        let milestones = &project.roadmap.milestones;
        if milestones.is_empty() {
            return Ok(0.0);
        }
        let total: f64 = milestones
            .iter()
            .map(|m| m.calculate_progress())
            .sum();
        Ok(total / milestones.len() as f64)
    }

    /// Get the roadmap (all milestones) for a project.
    pub async fn get_roadmap(&self, project_id: &str) -> Result<Option<Roadmap>, RoadmapError> {
        let project = self.get_project_or_err(project_id).await?;
        Ok(Some(project.roadmap))
    }

    // ── Phase 2 — Progress report ───────────────────────────────────

    /// Generate a detailed progress report with per-milestone and per-phase breakdown.
    pub async fn generate_progress_report(
        &self,
        project_id: &str,
    ) -> Result<ProgressReport, RoadmapError> {
        let project = self.get_project_or_err(project_id).await?;
        let m_count = project.roadmap.milestones.len();

        let mut milestones_progress: Vec<MilestoneProgress> = Vec::with_capacity(m_count);
        let mut total_goals: usize = 0;
        let mut completed_goals: usize = 0;

        for ms in &project.roadmap.milestones {
            let p_count = ms.phases.len();
            let mut phases_progress: Vec<PhaseProgress> = Vec::with_capacity(p_count);
            let mut ms_goals: usize = 0;
            let mut ms_done: usize = 0;

            for ph in &ms.phases {
                let ph_goals = ph.goals.len();
                let ph_done = ph.goals.iter().filter(|g| g.status == MilestoneStatus::Completed).count();
                ms_goals += ph_goals;
                ms_done += ph_done;

                let goals_detail: Vec<GoalProgress> = ph
                    .goals
                    .iter()
                    .map(|g| GoalProgress {
                        goal_id: g.goal_id.clone(),
                        description: g.description.clone(),
                        status: g.status.clone(),
                    })
                    .collect();

                phases_progress.push(PhaseProgress {
                    phase_id: ph.id.clone(),
                    name: ph.name.clone(),
                    goal_count: ph_goals,
                    completed_count: ph_done,
                    progress: if ph_goals > 0 { ph_done as f64 / ph_goals as f64 } else { 0.0 },
                    goals: goals_detail,
                });
            }

            total_goals += ms_goals;
            completed_goals += ms_done;

            milestones_progress.push(MilestoneProgress {
                milestone_id: ms.id.clone(),
                name: ms.name.clone(),
                status: ms.status.clone(),
                phase_count: p_count,
                goal_count: ms_goals,
                completed_count: ms_done,
                progress: ms.calculate_progress(),
                phases: phases_progress,
            });
        }

        Ok(ProgressReport {
            project_id: project.id.clone(),
            project_name: project.name.clone(),
            overall_progress: if total_goals > 0 { completed_goals as f64 / total_goals as f64 } else { 0.0 },
            milestone_count: m_count,
            total_goals,
            completed_goals,
            milestones: milestones_progress,
        })
    }

    // ── Phase 2 — Project context synthesis ─────────────────────────

    /// Format the project's current progress as a human-readable context string.
    ///
    /// This is designed to be injected into an `ExecutionContext` so that
    /// agents executing goals are aware of the broader project state.
    pub async fn synthesize_project_context(
        &self,
        project_id: &str,
    ) -> Result<String, RoadmapError> {
        let report = self.generate_progress_report(project_id).await?;
        let mut lines: Vec<String> = Vec::new();

        lines.push(format!(
            "## Project: {} ({})",
            report.project_name, report.project_id
        ));
        lines.push(format!(
            "Overall progress: {:.1}% ({}/{} goals completed across {} milestone(s))",
            report.overall_progress * 100.0,
            report.completed_goals,
            report.total_goals,
            report.milestone_count,
        ));
        lines.push(String::new());

        for ms in &report.milestones {
            let status_icon = match ms.status {
                MilestoneStatus::Completed => "[x]",
                MilestoneStatus::InProgress => "[~]",
                MilestoneStatus::Pending => "[ ]",
                MilestoneStatus::Blocked => "[!]",
            };
            lines.push(format!(
                "  {} {} — {:.1}% ({}/{})",
                status_icon, ms.name, ms.progress * 100.0, ms.completed_count, ms.goal_count,
            ));

            for ph in &ms.phases {
                lines.push(format!(
                    "      Phase {}: {:.1}% ({}/{})",
                    ph.name, ph.progress * 100.0, ph.completed_count, ph.goal_count,
                ));
                for g in &ph.goals {
                    let g_icon = match g.status {
                        MilestoneStatus::Completed => "[x]",
                        MilestoneStatus::InProgress => "[~]",
                        MilestoneStatus::Pending => "[ ]",
                        MilestoneStatus::Blocked => "[!]",
                    };
                    lines.push(format!("        {} {}", g_icon, g.description));
                }
            }
        }

        Ok(lines.join("\n"))
    }

    // ── Phase 2 — Auto-status propagation ───────────────────────────

    /// Update a goal's status and automatically recalculate milestone status.
    ///
    /// If all goals in a milestone are completed, the milestone is marked as
    /// `Completed`. If any goal is `InProgress`, the milestone is marked as
    /// `InProgress`. If any goal is `Blocked`, the milestone is marked as
    /// `Blocked`. Otherwise it stays `Pending`.
    pub async fn update_goal_status_with_propagation(
        &self,
        project_id: &str,
        milestone_id: &str,
        phase_id: &str,
        goal_id: &str,
        status: MilestoneStatus,
    ) -> Result<(), RoadmapError> {
        self.update_goal_status(project_id, milestone_id, phase_id, goal_id, status)
            .await?;

        let mut project = self.get_project_or_err(project_id).await?;
        let milestone = project
            .roadmap
            .milestones
            .iter_mut()
            .find(|m| m.id == milestone_id)
            .ok_or_else(|| {
                RoadmapError::MilestoneNotFound(milestone_id.to_string(), project_id.to_string())
            })?;

        let all_goals: Vec<&GoalRef> = milestone
            .phases
            .iter()
            .flat_map(|p| &p.goals)
            .collect();

        if all_goals.is_empty() {
            milestone.status = MilestoneStatus::Pending;
        } else if all_goals.iter().all(|g| g.status == MilestoneStatus::Completed) {
            milestone.status = MilestoneStatus::Completed;
        } else if all_goals.iter().any(|g| g.status == MilestoneStatus::Blocked) {
            milestone.status = MilestoneStatus::Blocked;
        } else if all_goals.iter().any(|g| g.status == MilestoneStatus::InProgress) {
            milestone.status = MilestoneStatus::InProgress;
        } else {
            milestone.status = MilestoneStatus::Pending;
        }

        self.update_project(&project).await
    }

    /// Auto-propagate: update all milestone statuses based on their goal completion.
    /// Useful as a batch repair after manual edits.
    pub async fn auto_propagate_all(&self, project_id: &str) -> Result<(), RoadmapError> {
        let mut project = self.get_project_or_err(project_id).await?;

        for ms in &mut project.roadmap.milestones {
            let all_goals: Vec<&GoalRef> = ms.phases.iter().flat_map(|p| &p.goals).collect();

            ms.status = if all_goals.is_empty() {
                MilestoneStatus::Pending
            } else if all_goals.iter().all(|g| g.status == MilestoneStatus::Completed) {
                MilestoneStatus::Completed
            } else if all_goals.iter().any(|g| g.status == MilestoneStatus::Blocked) {
                MilestoneStatus::Blocked
            } else if all_goals.iter().any(|g| g.status == MilestoneStatus::InProgress) {
                MilestoneStatus::InProgress
            } else {
                MilestoneStatus::Pending
            };
        }

        self.update_project(&project).await
    }

    // ── Phase 2 — Dynamic roadmap adjustment ────────────────────────

    /// Record a goal execution outcome into persistent storage.
    ///
    /// Outcomes are keyed by `(project, milestone, phase, goal)` so each
    /// goal retains only its **most recent** outcome.
    pub async fn record_outcome(&self, outcome: &ExecutionOutcome) -> Result<(), RoadmapError> {
        let key = outcome_key(
            &outcome.project_id,
            &outcome.milestone_id,
            &outcome.phase_id,
            &outcome.goal_id,
        );
        let value = serde_json::to_value(outcome)?;
        self.store.set_state_json(&key, value).await?;

        // Update outcome index (deduplicated list of (project,ms,ph,goal) tuples)
        let idx_key = outcome_index_key(&outcome.project_id);
        let mut ids: Vec<String> = match self.store.get_state_json(&idx_key).await? {
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            _ => Vec::new(),
        };
        let entry = format!(
            "{}:{}:{}",
            outcome.milestone_id, outcome.phase_id, outcome.goal_id
        );
        if !ids.contains(&entry) {
            ids.push(entry);
            // Keep only the 100 most recent entries (FIFO)
            if ids.len() > 100 {
                ids.drain(0..ids.len() - 100);
            }
            let arr: Value = ids.iter().map(|s| Value::String(s.clone())).collect();
            self.store.set_state_json(&idx_key, arr).await?;
        }

        Ok(())
    }

    /// Retrieve all recorded outcomes for a project.
    pub async fn get_outcomes(&self, project_id: &str) -> Result<Vec<ExecutionOutcome>, RoadmapError> {
        let idx_key = outcome_index_key(project_id);
        let ids: Vec<String> = match self.store.get_state_json(&idx_key).await? {
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            _ => return Ok(Vec::new()),
        };

        let mut outcomes = Vec::with_capacity(ids.len());
        for entry in &ids {
            let parts: Vec<&str> = entry.split(':').collect();
            if parts.len() == 3 {
                let key = outcome_key(project_id, parts[0], parts[1], parts[2]);
                if let Some(v) = self.store.get_state_json(&key).await? {
                    if let Ok(o) = serde_json::from_value::<ExecutionOutcome>(v) {
                        outcomes.push(o);
                    }
                }
            }
        }
        Ok(outcomes)
    }

    /// Evaluate roadmap health based on recent execution outcomes and goal states.
    ///
    /// Returns a `RoadmapHealth` with detected problems and suggested adjustments:
    /// - Goals with recent failures → flagged as problematic
    /// - Blocked milestones → listed
    /// - Goals that are Pending but have been superseded by failures → suggestions
    pub async fn evaluate_health(&self, project_id: &str) -> Result<RoadmapHealth, RoadmapError> {
        let project = self.get_project_or_err(project_id).await?;
        let outcomes = self.get_outcomes(project_id).await?;
        let recent_failures = outcomes.iter().filter(|o| !o.success).count();

        let mut problems: Vec<RoadmapProblem> = Vec::new();
        let mut suggestions: Vec<AdjustmentSuggestion> = Vec::new();
        let mut blocked_milestones: Vec<String> = Vec::new();
        let mut stale_goals: Vec<String> = Vec::new();

        // Check for milestones that are blocked
        for ms in &project.roadmap.milestones {
            if ms.status == MilestoneStatus::Blocked {
                blocked_milestones.push(ms.id.clone());
                problems.push(RoadmapProblem {
                    severity: ProblemSeverity::Critical,
                    message: format!("Milestone '{}' is blocked", ms.name),
                    milestone_id: Some(ms.id.clone()),
                    phase_id: None,
                    goal_id: None,
                });
                suggestions.push(AdjustmentSuggestion {
                    problem: format!("Milestone '{}' blocked", ms.name),
                    action: "Investigate blocking goals and resolve dependencies".into(),
                    target_goal_id: None,
                });
            }

            let has_blocked_goal = ms
                .phases
                .iter()
                .any(|ph| ph.goals.iter().any(|g| g.status == MilestoneStatus::Blocked));
            if has_blocked_goal && ms.status != MilestoneStatus::Blocked {
                problems.push(RoadmapProblem {
                    severity: ProblemSeverity::Warning,
                    message: format!(
                        "Milestone '{}' has blocked goal(s) but hasn't been marked Blocked",
                        ms.name
                    ),
                    milestone_id: Some(ms.id.clone()),
                    phase_id: None,
                    goal_id: None,
                });
                suggestions.push(AdjustmentSuggestion {
                    problem: format!("Goals blocked in '{}'", ms.name),
                    action: "Consider marking milestone as Blocked or unblocking the goals".into(),
                    target_goal_id: None,
                });
            }

            // Check for stale goals (Pending for too long when others are Completed)
            for ph in &ms.phases {
                let has_completed = ph.goals.iter().any(|g| g.status == MilestoneStatus::Completed);
                let pending: Vec<&GoalRef> = ph
                    .goals
                    .iter()
                    .filter(|g| g.status == MilestoneStatus::Pending)
                    .collect();

                if has_completed && !pending.is_empty() && ms.status != MilestoneStatus::Blocked {
                    for g in &pending {
                        stale_goals.push(g.goal_id.clone());
                    }
                    // Only report one problem per phase with stale goals
                    problems.push(RoadmapProblem {
                        severity: ProblemSeverity::Warning,
                        message: format!(
                            "Phase '{}' has {} stale pending goal(s) with completed peers",
                            ph.name,
                            pending.len()
                        ),
                        milestone_id: Some(ms.id.clone()),
                        phase_id: Some(ph.id.clone()),
                        goal_id: None,
                    });
                }

                // Check for failed goals
                for g in &ph.goals {
                    let has_failure = outcomes.iter().any(|o| {
                        o.goal_id == g.goal_id
                            && o.phase_id == ph.id
                            && o.milestone_id == ms.id
                            && !o.success
                    });
                    if has_failure && g.status != MilestoneStatus::Blocked {
                        suggestions.push(AdjustmentSuggestion {
                            problem: format!("Goal '{}' has recent execution failures", g.description),
                            action: "Consider marking as Blocked and adding a remediation goal".into(),
                            target_goal_id: Some(g.goal_id.clone()),
                        });
                    }
                }
            }
        }

        let healthy = problems.is_empty();
        Ok(RoadmapHealth {
            project_id: project_id.to_string(),
            healthy,
            blocked_milestones,
            stale_goals,
            recent_failures,
            problems,
            suggestions,
        })
    }

    /// Apply automatic blocking: mark goals as Blocked if a preceding goal
    /// in the same phase has recent failures.
    ///
    /// Uses `depends_on` field in `Phase` to determine ordering.
    pub async fn apply_blocking(&self, project_id: &str) -> Result<RoadmapHealth, RoadmapError> {
        let health = self.evaluate_health(project_id).await?;

        // Get failed goal IDs from recent outcomes
        let outcomes = self.get_outcomes(project_id).await?;
        let failed_goals: Vec<String> = outcomes
            .iter()
            .filter(|o| !o.success)
            .map(|o| o.goal_id.clone())
            .collect();

        if failed_goals.is_empty() {
            return Ok(health);
        }

        let mut project = self.get_project_or_err(project_id).await?;

        for ms in &mut project.roadmap.milestones {
            for ph in &mut ms.phases {
                let failed_in_phase: Vec<String> = ph
                    .goals
                    .iter()
                    .filter(|g| failed_goals.contains(&g.goal_id))
                    .map(|g| g.goal_id.clone())
                    .collect();

                if failed_in_phase.is_empty() {
                    continue;
                }

                let failed_positions: Vec<usize> = ph
                    .goals
                    .iter()
                    .enumerate()
                    .filter(|(_, g)| failed_in_phase.contains(&g.goal_id))
                    .map(|(i, _)| i)
                    .collect();

                let to_block: Vec<usize> = ph
                    .goals
                    .iter()
                    .enumerate()
                    .filter(|(i, g)| {
                        if failed_in_phase.contains(&g.goal_id)
                            || g.status == MilestoneStatus::Completed
                        {
                            return false;
                        }
                        let depends_on_failed = ph.depends_on.iter().any(|d| {
                            ph.goals.iter().any(|pg| {
                                pg.goal_id == *d && failed_in_phase.contains(&pg.goal_id)
                            })
                        });
                        let any_failed_before = failed_positions.iter().any(|fp| *fp < *i);
                        depends_on_failed || any_failed_before
                    })
                    .map(|(i, _)| i)
                    .collect();

                for i in to_block {
                    ph.goals[i].status = MilestoneStatus::Blocked;
                }
            }
        }

        self.update_project(&project).await?;

        // Re-evaluate health after blocking
        self.evaluate_health(project_id).await
    }

    // ── Phase 2+ — Next goal, goal context, and completion prediction ──

    /// Find the next pending goal, respecting phase-level dependencies.
    ///
    /// Scans milestones in order, skipping completed ones. Within each
    /// milestone, phases whose dependencies are not met are skipped.
    /// Returns the first pending goal found, or `None` if all are done.
    pub async fn get_next_goal(&self, project_id: &str) -> Result<Option<GoalRef>, RoadmapError> {
        let project = self.get_project_or_err(project_id).await?;

        for milestone in &project.roadmap.milestones {
            if milestone.status == MilestoneStatus::Completed {
                continue;
            }

            for phase in &milestone.phases {
                let dependencies_met = phase.depends_on.iter().all(|dep_id| {
                    project.roadmap.milestones.iter()
                        .flat_map(|m| &m.phases)
                        .any(|p| p.id == *dep_id && p.goals.iter().all(|g| g.status == MilestoneStatus::Completed))
                });

                if !dependencies_met {
                    continue;
                }

                for goal in &phase.goals {
                    if goal.status == MilestoneStatus::Pending {
                        return Ok(Some(goal.clone()));
                    }
                }
            }
        }
        Ok(None)
    }

    /// Retrieve formatted context about where a specific goal fits in the project.
    ///
    /// Returns a human-readable string describing project progress,
    /// milestone/phase placement, and surrounding goals, with the
    /// target goal highlighted by a `>>>` marker.
    pub async fn get_goal_context(
        &self,
        project_id: &str,
        goal_id: &str,
    ) -> Result<String, RoadmapError> {
        let project = self.get_project_or_err(project_id).await?;

        let total_goals: usize = project.roadmap.milestones.iter()
            .flat_map(|m| &m.phases)
            .map(|p| p.goals.len())
            .sum();
        let completed_goals: usize = project.roadmap.milestones.iter()
            .flat_map(|m| &m.phases)
            .flat_map(|p| &p.goals)
            .filter(|g| g.status == MilestoneStatus::Completed)
            .count();

        let mut context = format!(
            "Project: {} ({})\nOverall Progress: {}/{} goals completed ({:.0}%)\n\n",
            project.name,
            project.description,
            completed_goals,
            total_goals,
            if total_goals > 0 { completed_goals as f64 / total_goals as f64 * 100.0 } else { 0.0 },
        );

        for milestone in &project.roadmap.milestones {
            let ms_progress = milestone.calculate_progress();
            context.push_str(&format!("Milestone: {} [{:.0}%]\n", milestone.name, ms_progress * 100.0));
            for phase in &milestone.phases {
                context.push_str(&format!("  Phase: {}\n", phase.name));
                for goal in &phase.goals {
                    let marker = if goal.goal_id == goal_id {
                        ">>> "
                    } else {
                        match goal.status {
                            MilestoneStatus::Completed => "  \u{2713} ",
                            MilestoneStatus::InProgress => "  \u{25B6} ",
                            _ => "    ",
                        }
                    };
                    context.push_str(&format!("{}{} ({:?})\n", marker, goal.description, goal.status));
                }
            }
        }
        Ok(context)
    }

    /// Predict milestone completion based on current progress.
    ///
    /// Uses goal completion ratios and milestone status to estimate
    /// risk and ordering. Returns a `ProgressPrediction` with per-milestone
    /// breakdown and an estimated completion order.
    pub async fn predict_completion(
        &self,
        project_id: &str,
    ) -> Result<ProgressPrediction, RoadmapError> {
        let project = self.get_project_or_err(project_id).await?;

        let mut total_goals = 0usize;
        let mut completed_goals = 0usize;
        let mut milestone_preds = Vec::new();
        let mut risk_levels = Vec::new();

        for milestone in &project.roadmap.milestones {
            let mut ms_total = 0usize;
            let mut ms_completed = 0usize;

            for phase in &milestone.phases {
                for goal in &phase.goals {
                    ms_total += 1;
                    total_goals += 1;
                    if goal.status == MilestoneStatus::Completed {
                        ms_completed += 1;
                        completed_goals += 1;
                    }
                }
            }

            let progress = if ms_total > 0 { ms_completed as f64 / ms_total as f64 } else { 1.0 };
            let remaining = ms_total.saturating_sub(ms_completed);

            let risk = match milestone.status {
                MilestoneStatus::Completed => "done",
                MilestoneStatus::InProgress if progress > 0.5 => "low",
                MilestoneStatus::InProgress => "medium",
                _ => "high",
            };
            risk_levels.push(risk);

            milestone_preds.push(MilestonePrediction {
                name: milestone.name.clone(),
                progress_pct: progress * 100.0,
                remaining_goals: remaining,
                risk_level: risk.to_string(),
            });
        }

        let overall_pct = if total_goals > 0 { completed_goals as f64 / total_goals as f64 * 100.0 } else { 0.0 };
        let remaining_goals = total_goals.saturating_sub(completed_goals);

        // highest progress first
        let mut sorted: Vec<&MilestonePrediction> = milestone_preds.iter()
            .filter(|m| m.risk_level != "done")
            .collect();
        sorted.sort_by(|a, b| b.progress_pct.partial_cmp(&a.progress_pct).unwrap_or(std::cmp::Ordering::Equal));
        let completion_order: Vec<String> = sorted.iter().map(|m| m.name.clone()).collect();

        let overall_risk = if risk_levels.contains(&"high") { "high"
            } else if risk_levels.contains(&"medium") { "medium"
            } else { "low" }.to_string();

        Ok(ProgressPrediction {
            project_id: project.id,
            project_name: project.name,
            overall_progress_pct: overall_pct,
            remaining_goals,
            milestone_predictions: milestone_preds,
            estimated_completion_order: completion_order,
            risk_level: overall_risk,
        })
    }

    // ── Internal helpers ────────────────────────────────────────────

    /// Get a project or return `ProjectNotFound` error.
    async fn get_project_or_err(&self, id: &str) -> Result<Project, RoadmapError> {
        self.get_project(id)
            .await?
            .ok_or_else(|| RoadmapError::ProjectNotFound(id.to_string()))
    }

    /// Load the project index from the store.
    async fn load_index(&self) -> Result<Vec<String>, RoadmapError> {
        match self.store.get_state_json(PROJECTS_INDEX_KEY).await? {
            Some(Value::Array(arr)) => {
                let ids: Vec<String> = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                Ok(ids)
            }
            _ => Ok(Vec::new()),
        }
    }

    /// Save the project index to the store.
    async fn save_index(&self, ids: &[String]) -> Result<(), RoadmapError> {
        let arr: Value = ids.iter().map(|s| Value::String(s.clone())).collect();
        self.store.set_state_json(PROJECTS_INDEX_KEY, arr).await?;
        Ok(())
    }

    /// Add a project ID to the index.
    async fn add_to_index(&self, id: &str) -> Result<(), RoadmapError> {
        let mut ids = self.load_index().await?;
        if !ids.contains(&id.to_string()) {
            ids.push(id.to_string());
            self.save_index(&ids).await?;
        }
        Ok(())
    }

    /// Remove a project ID from the index.
    async fn remove_from_index(&self, id: &str) -> Result<(), RoadmapError> {
        let mut ids = self.load_index().await?;
        ids.retain(|i| i != id);
        self.save_index(&ids).await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sisyphus_types::MilestoneStatus;

    async fn setup_manager() -> RoadmapManager {
        let store = Arc::new(
            MfsStore::new("sqlite::memory:")
                .await
                .expect("Failed to create MfsStore"),
        );
        RoadmapManager::new(store)
    }

    fn make_goal(id: &str, desc: &str, status: MilestoneStatus) -> GoalRef {
        GoalRef {
            goal_id: id.to_string(),
            description: desc.to_string(),
            status,
        }
    }

    fn make_phase(id: &str, name: &str, goals: Vec<GoalRef>) -> Phase {
        Phase {
            id: id.to_string(),
            name: name.to_string(),
            goals,
            depends_on: Vec::new(),
        }
    }

    fn make_milestone(id: &str, name: &str, phases: Vec<Phase>, status: MilestoneStatus) -> Milestone {
        Milestone {
            id: id.to_string(),
            name: name.to_string(),
            phases,
            status,
        }
    }

    // ── Project CRUD ────────────────────────────────────────────────

    #[tokio::test]
    async fn test_create_and_get_project() {
        let mgr = setup_manager().await;
        let project = mgr
            .create_project("proj-1", "Test Project", "A test project")
            .await
            .unwrap();

        assert_eq!(project.id, "proj-1");
        assert_eq!(project.name, "Test Project");
        assert!(project.roadmap.milestones.is_empty());

        let loaded = mgr.get_project("proj-1").await.unwrap().unwrap();
        assert_eq!(loaded.id, "proj-1");
        assert_eq!(loaded.name, "Test Project");
    }

    #[tokio::test]
    async fn test_create_duplicate_project() {
        let mgr = setup_manager().await;
        mgr.create_project("dup", "First", "desc").await.unwrap();
        let err = mgr
            .create_project("dup", "Second", "other")
            .await
            .unwrap_err();
        assert!(matches!(err, RoadmapError::DuplicateProject(_)));
    }

    #[tokio::test]
    async fn test_get_nonexistent_project() {
        let mgr = setup_manager().await;
        let result = mgr.get_project("nope").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_update_project() {
        let mgr = setup_manager().await;
        mgr.create_project("p1", "Original", "desc").await.unwrap();

        let mut project = mgr.get_project("p1").await.unwrap().unwrap();
        project.name = "Updated".to_string();
        mgr.update_project(&project).await.unwrap();

        let loaded = mgr.get_project("p1").await.unwrap().unwrap();
        assert_eq!(loaded.name, "Updated");
    }

    #[tokio::test]
    async fn test_delete_project() {
        let mgr = setup_manager().await;
        mgr.create_project("p1", "To Delete", "bye").await.unwrap();
        mgr.delete_project("p1").await.unwrap();

        let result = mgr.get_project("p1").await.unwrap();
        assert!(result.is_none());

        let ids = mgr.list_project_ids().await.unwrap();
        assert!(!ids.contains(&"p1".to_string()));
    }

    #[tokio::test]
    async fn test_list_projects() {
        let mgr = setup_manager().await;
        mgr.create_project("a", "A", "desc").await.unwrap();
        mgr.create_project("b", "B", "desc").await.unwrap();
        mgr.create_project("c", "C", "desc").await.unwrap();

        let ids = mgr.list_project_ids().await.unwrap();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&"a".to_string()));
        assert!(ids.contains(&"b".to_string()));
        assert!(ids.contains(&"c".to_string()));

        let projects = mgr.list_projects().await.unwrap();
        assert_eq!(projects.len(), 3);
    }

    // ── Milestone operations ────────────────────────────────────────

    #[tokio::test]
    async fn test_add_milestone() {
        let mgr = setup_manager().await;
        mgr.create_project("p1", "P1", "desc").await.unwrap();

        let ms = make_milestone("ms-1", "Release v1", vec![], MilestoneStatus::Pending);
        mgr.add_milestone("p1", ms).await.unwrap();

        let project = mgr.get_project("p1").await.unwrap().unwrap();
        assert_eq!(project.roadmap.milestones.len(), 1);
        assert_eq!(project.roadmap.milestones[0].name, "Release v1");
    }

    #[tokio::test]
    async fn test_update_milestone_status() {
        let mgr = setup_manager().await;
        mgr.create_project("p1", "P1", "desc").await.unwrap();

        let ms = make_milestone("ms-1", "Release", vec![], MilestoneStatus::Pending);
        mgr.add_milestone("p1", ms).await.unwrap();

        mgr.update_milestone_status("p1", "ms-1", MilestoneStatus::InProgress)
            .await
            .unwrap();

        let project = mgr.get_project("p1").await.unwrap().unwrap();
        assert_eq!(
            project.roadmap.milestones[0].status,
            MilestoneStatus::InProgress
        );
    }

    #[tokio::test]
    async fn test_remove_milestone() {
        let mgr = setup_manager().await;
        mgr.create_project("p1", "P1", "desc").await.unwrap();

        let ms = make_milestone("ms-1", "Release", vec![], MilestoneStatus::Pending);
        mgr.add_milestone("p1", ms).await.unwrap();
        assert_eq!(
            mgr.get_project("p1").await.unwrap().unwrap()
                .roadmap.milestones.len(),
            1
        );

        mgr.remove_milestone("p1", "ms-1").await.unwrap();
        assert_eq!(
            mgr.get_project("p1").await.unwrap().unwrap()
                .roadmap.milestones.len(),
            0
        );
    }

    // ── Phase & Goal operations ─────────────────────────────────────

    #[tokio::test]
    async fn test_add_phase() {
        let mgr = setup_manager().await;
        mgr.create_project("p1", "P1", "desc").await.unwrap();

        let ms = make_milestone("ms-1", "Release", vec![], MilestoneStatus::Pending);
        mgr.add_milestone("p1", ms).await.unwrap();

        let phase = make_phase("ph-1", "Setup", vec![]);
        mgr.add_phase("p1", "ms-1", phase).await.unwrap();

        let project = mgr.get_project("p1").await.unwrap().unwrap();
        assert_eq!(project.roadmap.milestones[0].phases.len(), 1);
        assert_eq!(project.roadmap.milestones[0].phases[0].name, "Setup");
    }

    #[tokio::test]
    async fn test_add_and_update_goal() {
        let mgr = setup_manager().await;
        mgr.create_project("p1", "P1", "desc").await.unwrap();

        let ms = make_milestone("ms-1", "Release", vec![], MilestoneStatus::Pending);
        mgr.add_milestone("p1", ms).await.unwrap();

        let phase = make_phase("ph-1", "Dev", vec![]);
        mgr.add_phase("p1", "ms-1", phase).await.unwrap();

        let goal = make_goal("g-1", "Implement feature X", MilestoneStatus::Pending);
        mgr.add_goal("p1", "ms-1", "ph-1", goal).await.unwrap();

        // Verify goal added
        let project = mgr.get_project("p1").await.unwrap().unwrap();
        assert_eq!(
            project.roadmap.milestones[0].phases[0].goals.len(),
            1
        );
        assert_eq!(
            project.roadmap.milestones[0].phases[0].goals[0].description,
            "Implement feature X"
        );

        // Update goal status
        mgr.update_goal_status("p1", "ms-1", "ph-1", "g-1", MilestoneStatus::Completed)
            .await
            .unwrap();

        let project = mgr.get_project("p1").await.unwrap().unwrap();
        assert_eq!(
            project.roadmap.milestones[0].phases[0].goals[0].status,
            MilestoneStatus::Completed
        );
    }

    // ── Phase 2 — Progress report ────────────────────────────────────

    #[tokio::test]
    async fn test_progress_report() {
        let mgr = setup_manager().await;
        mgr.create_project("p1", "Phase2", "test").await.unwrap();

        let g1 = make_goal("g1", "Done", MilestoneStatus::Completed);
        let g2 = make_goal("g2", "Todo", MilestoneStatus::Pending);
        let ph = make_phase("ph1", "Dev", vec![g1, g2]);
        let ms = make_milestone("ms1", "M1", vec![ph], MilestoneStatus::InProgress);
        mgr.add_milestone("p1", ms).await.unwrap();

        let report = mgr.generate_progress_report("p1").await.unwrap();
        assert_eq!(report.project_name, "Phase2");
        assert_eq!(report.milestone_count, 1);
        assert!(report.overall_progress > 0.49 && report.overall_progress < 0.51);
        assert_eq!(report.total_goals, 2);
        assert_eq!(report.completed_goals, 1);
        assert_eq!(report.milestones[0].phases[0].goal_count, 2);
        assert_eq!(report.milestones[0].phases[0].completed_count, 1);
    }

    #[tokio::test]
    async fn test_progress_report_empty_project() {
        let mgr = setup_manager().await;
        mgr.create_project("empty", "Empty", "").await.unwrap();
        let report = mgr.generate_progress_report("empty").await.unwrap();
        assert_eq!(report.total_goals, 0);
        assert_eq!(report.completed_goals, 0);
        assert!((report.overall_progress - 0.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_progress_report_multi_milestone() {
        let mgr = setup_manager().await;
        mgr.create_project("p1", "Multi", "desc").await.unwrap();

        let ph = make_phase("ph1", "Phase 1", vec![
            make_goal("g1", "Done", MilestoneStatus::Completed),
        ]);
        let ms1 = make_milestone("ms1", "M1", vec![ph], MilestoneStatus::Completed);
        mgr.add_milestone("p1", ms1).await.unwrap();

        let ph2 = make_phase("ph2", "Phase 2", vec![
            make_goal("g2", "Todo", MilestoneStatus::Pending),
            make_goal("g3", "Also Todo", MilestoneStatus::Pending),
        ]);
        let ms2 = make_milestone("ms2", "M2", vec![ph2], MilestoneStatus::Pending);
        mgr.add_milestone("p1", ms2).await.unwrap();

        let report = mgr.generate_progress_report("p1").await.unwrap();
        assert_eq!(report.milestone_count, 2);
        assert_eq!(report.total_goals, 3);
        assert!((report.overall_progress - 0.333).abs() < 0.01);
    }

    // ── Phase 2 — Project context ────────────────────────────────────

    #[tokio::test]
    async fn test_synthesize_project_context() {
        let mgr = setup_manager().await;
        mgr.create_project("ctx", "ContextTest", "desc").await.unwrap();

        let g1 = make_goal("g1", "Task A", MilestoneStatus::Completed);
        let g2 = make_goal("g2", "Task B", MilestoneStatus::InProgress);
        let ph = make_phase("ph1", "Setup", vec![g1, g2]);
        let ms = make_milestone("ms1", "Release", vec![ph], MilestoneStatus::InProgress);
        mgr.add_milestone("ctx", ms).await.unwrap();

        let ctx = mgr.synthesize_project_context("ctx").await.unwrap();
        assert!(ctx.contains("ContextTest"));
        assert!(ctx.contains("50.0%"));
        assert!(ctx.contains("[~]"));
        assert!(ctx.contains("[x]"));
        assert!(ctx.contains("Task A"));
        assert!(ctx.contains("Task B"));
    }

    #[tokio::test]
    async fn test_context_empty_project() {
        let mgr = setup_manager().await;
        mgr.create_project("empty", "Empty", "").await.unwrap();
        let ctx = mgr.synthesize_project_context("empty").await.unwrap();
        assert!(ctx.contains("0/0 goals completed"));
    }

    // ── Phase 2 — Auto-status propagation ────────────────────────────

    #[tokio::test]
    async fn test_update_goal_with_propagation_completes_milestone() {
        let mgr = setup_manager().await;
        mgr.create_project("p1", "P1", "desc").await.unwrap();

        let g1 = make_goal("g1", "Task A", MilestoneStatus::Pending);
        let ph = make_phase("ph1", "Phase 1", vec![g1]);
        let ms = make_milestone("ms1", "M1", vec![ph], MilestoneStatus::Pending);
        mgr.add_milestone("p1", ms).await.unwrap();

        mgr.update_goal_status_with_propagation("p1", "ms1", "ph1", "g1", MilestoneStatus::Completed)
            .await
            .unwrap();

        let project = mgr.get_project("p1").await.unwrap().unwrap();
        assert_eq!(project.roadmap.milestones[0].status, MilestoneStatus::Completed);
    }

    #[tokio::test]
    async fn test_update_goal_with_propagation_blocked() {
        let mgr = setup_manager().await;
        mgr.create_project("p1", "P1", "desc").await.unwrap();

        let g1 = make_goal("g1", "Task A", MilestoneStatus::Pending);
        let g2 = make_goal("g2", "Task B", MilestoneStatus::Pending);
        let ph = make_phase("ph1", "Phase 1", vec![g1, g2]);
        let ms = make_milestone("ms1", "M1", vec![ph], MilestoneStatus::Pending);
        mgr.add_milestone("p1", ms).await.unwrap();

        mgr.update_goal_status_with_propagation("p1", "ms1", "ph1", "g1", MilestoneStatus::Blocked)
            .await
            .unwrap();

        let project = mgr.get_project("p1").await.unwrap().unwrap();
        assert_eq!(project.roadmap.milestones[0].status, MilestoneStatus::Blocked);
    }

    #[tokio::test]
    async fn test_auto_propagate_all() {
        let mgr = setup_manager().await;
        mgr.create_project("p1", "P1", "desc").await.unwrap();

        let g1 = make_goal("g1", "Done", MilestoneStatus::Completed);
        let g2 = make_goal("g2", "Done2", MilestoneStatus::Completed);
        let ph = make_phase("ph1", "Phase 1", vec![g1, g2]);
        // Intentionally wrong status: Pending but both goals are Completed
        let ms = make_milestone("ms1", "M1", vec![ph], MilestoneStatus::Pending);
        mgr.add_milestone("p1", ms).await.unwrap();

        mgr.auto_propagate_all("p1").await.unwrap();

        let project = mgr.get_project("p1").await.unwrap().unwrap();
        assert_eq!(project.roadmap.milestones[0].status, MilestoneStatus::Completed);
    }

    // ── Phase 2 — Dynamic roadmap adjustment ───────────────────────────

    #[tokio::test]
    async fn test_record_and_get_outcomes() {
        let mgr = setup_manager().await;
        mgr.create_project("p1", "P1", "desc").await.unwrap();

        let outcome = ExecutionOutcome {
            goal_id: "g1".into(),
            project_id: "p1".into(),
            milestone_id: "ms1".into(),
            phase_id: "ph1".into(),
            success: true,
            deviation_score: Some(0.1),
            error: None,
            timestamp: 1000,
        };
        mgr.record_outcome(&outcome).await.unwrap();

        let outcomes = mgr.get_outcomes("p1").await.unwrap();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].goal_id, "g1");
        assert!(outcomes[0].success);
    }

    #[tokio::test]
    async fn test_record_outcome_overwrites() {
        let mgr = setup_manager().await;
        mgr.create_project("p1", "P1", "desc").await.unwrap();

        let fail = ExecutionOutcome {
            goal_id: "g1".into(),
            project_id: "p1".into(),
            milestone_id: "ms1".into(),
            phase_id: "ph1".into(),
            success: false,
            deviation_score: None,
            error: Some("timeout".into()),
            timestamp: 1000,
        };
        mgr.record_outcome(&fail).await.unwrap();

        let success = ExecutionOutcome {
            goal_id: "g1".into(),
            project_id: "p1".into(),
            milestone_id: "ms1".into(),
            phase_id: "ph1".into(),
            success: true,
            deviation_score: Some(0.05),
            error: None,
            timestamp: 2000,
        };
        mgr.record_outcome(&success).await.unwrap();

        // Should only have 1 outcome entry (last one wins for same goal)
        let outcomes = mgr.get_outcomes("p1").await.unwrap();
        assert_eq!(outcomes.len(), 1);
        assert!(outcomes[0].success);
    }

    #[tokio::test]
    async fn test_evaluate_health_healthy() {
        let mgr = setup_manager().await;
        mgr.create_project("p1", "P1", "desc").await.unwrap();

        let g1 = make_goal("g1", "Done", MilestoneStatus::Completed);
        let ph = make_phase("ph1", "Phase 1", vec![g1]);
        let ms = make_milestone("ms1", "M1", vec![ph], MilestoneStatus::Completed);
        mgr.add_milestone("p1", ms).await.unwrap();

        let health = mgr.evaluate_health("p1").await.unwrap();
        assert!(health.healthy);
        assert!(health.problems.is_empty());
    }

    #[tokio::test]
    async fn test_evaluate_health_blocked_milestone() {
        let mgr = setup_manager().await;
        mgr.create_project("p1", "P1", "desc").await.unwrap();

        let g1 = make_goal("g1", "Stuck", MilestoneStatus::Pending);
        let ph = make_phase("ph1", "Phase 1", vec![g1]);
        let ms = make_milestone("ms1", "M1", vec![ph], MilestoneStatus::Blocked);
        mgr.add_milestone("p1", ms).await.unwrap();

        let health = mgr.evaluate_health("p1").await.unwrap();
        assert!(!health.healthy);
        assert_eq!(health.blocked_milestones.len(), 1);
        assert!(health.problems.iter().any(|p| matches!(p.severity, ProblemSeverity::Critical)));
    }

    #[tokio::test]
    async fn test_evaluate_health_stale_goals() {
        let mgr = setup_manager().await;
        mgr.create_project("p1", "P1", "desc").await.unwrap();

        let g1 = make_goal("g1", "Done", MilestoneStatus::Completed);
        let g2 = make_goal("g2", "Stale", MilestoneStatus::Pending);
        let ph = make_phase("ph1", "Phase 1", vec![g1, g2]);
        let ms = make_milestone("ms1", "M1", vec![ph], MilestoneStatus::InProgress);
        mgr.add_milestone("p1", ms).await.unwrap();

        let health = mgr.evaluate_health("p1").await.unwrap();
        assert!(!health.healthy);
        assert!(!health.stale_goals.is_empty());
    }

    #[tokio::test]
    async fn test_apply_blocking() {
        let mgr = setup_manager().await;
        mgr.create_project("p1", "P1", "desc").await.unwrap();

        // Phase with 3 goals, first one has failed execution
        let g1 = make_goal("g1", "First", MilestoneStatus::Pending);
        let g2 = make_goal("g2", "Second", MilestoneStatus::Pending);
        let g3 = make_goal("g3", "Third", MilestoneStatus::Pending);
        let ph = make_phase("ph1", "Phase 1", vec![g1, g2, g3]);
        let ms = make_milestone("ms1", "M1", vec![ph], MilestoneStatus::InProgress);
        mgr.add_milestone("p1", ms).await.unwrap();

        // Record failure for first goal
        let outcome = ExecutionOutcome {
            goal_id: "g1".into(),
            project_id: "p1".into(),
            milestone_id: "ms1".into(),
            phase_id: "ph1".into(),
            success: false,
            deviation_score: None,
            error: Some("crash".into()),
            timestamp: 1000,
        };
        mgr.record_outcome(&outcome).await.unwrap();

        // Apply blocking — should block g2 and g3 since g1 failed
        let health = mgr.apply_blocking("p1").await.unwrap();
        assert!(!health.healthy);

        let project = mgr.get_project("p1").await.unwrap().unwrap();
        let goals = &project.roadmap.milestones[0].phases[0].goals;
        assert_eq!(goals[0].status, MilestoneStatus::Pending); // g1 stays pending (it failed but wasn't explicitly blocked)
        assert_eq!(goals[1].status, MilestoneStatus::Blocked); // g2 blocked after failure
        assert_eq!(goals[2].status, MilestoneStatus::Blocked); // g3 blocked too
    }

    // ── Error cases ─────────────────────────────────────────────────

    #[tokio::test]
    async fn test_update_nonexistent_project() {
        let mgr = setup_manager().await;
        let project = Project::new("nope".into(), "Nope".into(), "".into());
        let err = mgr.update_project(&project).await.unwrap_err();
        assert!(matches!(err, RoadmapError::ProjectNotFound(_)));
    }

    #[tokio::test]
    async fn test_milestone_not_found() {
        let mgr = setup_manager().await;
        mgr.create_project("p1", "P1", "desc").await.unwrap();

        let err = mgr
            .update_milestone_status("p1", "nonexistent", MilestoneStatus::Completed)
            .await
            .unwrap_err();
        assert!(matches!(err, RoadmapError::MilestoneNotFound(_, _)));
    }
}
