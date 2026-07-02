use serde_json::Value;
use sisyphus_mem::MfsStore;
use sisyphus_types::{GoalRef, Milestone, MilestoneStatus, Phase, Project, Roadmap, StateStore};
use std::sync::Arc;

use crate::RoadmapError;

// ---------------------------------------------------------------------------
// Key conventions
// ---------------------------------------------------------------------------

/// Key storing the index of all project IDs.
const PROJECTS_INDEX_KEY: &str = "roadmap:projects";

/// Build a key for a specific project.
fn project_key(id: &str) -> String {
    format!("roadmap:project:{}", id)
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
    pub(crate) store: Arc<MfsStore>,
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
            .ok_or_else(|| RoadmapError::GoalNotFound(goal_id.to_string(), phase_id.to_string()))?;
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
        let total: f64 = milestones.iter().map(|m| m.calculate_progress()).sum();
        Ok(total / milestones.len() as f64)
    }

    /// Get the roadmap (all milestones) for a project.
    pub async fn get_roadmap(&self, project_id: &str) -> Result<Option<Roadmap>, RoadmapError> {
        let project = self.get_project_or_err(project_id).await?;
        Ok(Some(project.roadmap))
    }

    // ── Internal helpers ────────────────────────────────────────────

    /// Get a project or return `ProjectNotFound` error.
    pub(crate) async fn get_project_or_err(&self, id: &str) -> Result<Project, RoadmapError> {
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
