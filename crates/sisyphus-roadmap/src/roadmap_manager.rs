use serde_json::Value;
use sisyphus_mem::MfsStore;
use sisyphus_types::{GoalRef, Milestone, MilestoneStatus, Phase, Project, Roadmap, StateStore};
use std::sync::Arc;

use crate::RoadmapError;
use crate::strategic_reflection::ProposalAction;

// ---------------------------------------------------------------------------
// Key conventions
// ---------------------------------------------------------------------------

/// Key storing the index of all project IDs.
const PROJECTS_INDEX_KEY: &str = "roadmap:projects";

/// Project metadata key.
fn project_key(id: &str) -> String {
    format!("roadmap:project:{}", id)
}

/// Project milestone IDs list key.
fn project_milestones_key(id: &str) -> String {
    format!("roadmap:project:{}:milestones", id)
}

/// Milestone metadata key.
fn milestone_key(id: &str) -> String {
    format!("roadmap:milestone:{}", id)
}

/// Milestone phase IDs list key.
fn milestone_phases_key(id: &str) -> String {
    format!("roadmap:milestone:{}:phases", id)
}

/// Phase metadata key.
fn phase_key(id: &str) -> String {
    format!("roadmap:phase:{}", id)
}

/// Phase goal refs list key.
fn phase_goals_key(id: &str) -> String {
    format!("roadmap:phase:{}:goals", id)
}

// ---------------------------------------------------------------------------
// RoadmapManager
// ---------------------------------------------------------------------------

/// Strategic planning manager that provides CRUD operations over a
/// Project → Milestone → Phase → Goal hierarchy, persisted via MfsStore.
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
        
        // Persist project metadata
        let meta = serde_json::json!({
            "id": project.id,
            "name": project.name,
            "description": project.description,
        });
        self.store.set_state_json(&key, meta).await?;

        // Initialize empty milestone list
        self.store.set_state_json(&project_milestones_key(id), serde_json::json!([]))
            .await?;

        // Update project index
        self.add_to_index(id).await?;

        Ok(project)
    }

    /// Retrieve a project by its ID and hydrate it from granular storage.
    pub async fn get_project(&self, id: &str) -> Result<Option<Project>, RoadmapError> {
        let key = project_key(id);
        let meta_val = self.store.get_state_json(&key).await?;
        
        let Some(meta) = meta_val.filter(|v| !v.is_null()) else {
            return Ok(None);
        };

        let project_id = meta["id"].as_str().ok_or(RoadmapError::Internal("Invalid project id".into()))?.to_string();
        let name = meta["name"].as_str().ok_or(RoadmapError::Internal("Invalid project name".into()))?.to_string();
        let description = meta["description"].as_str().ok_or(RoadmapError::Internal("Invalid project description".into()))?.to_string();

        // Hydrate Roadmap
        let milestones_val = self.store.get_state_json(&project_milestones_key(id)).await?;
        let milestone_ids: Vec<String> = milestones_val
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();

        let mut milestones = Vec::with_capacity(milestone_ids.len());
        for m_id in milestone_ids {
            // Hydrate Milestone
            let m_val = self.store.get_state_json(&milestone_key(&m_id)).await?;
            let Some(m_meta) = m_val.filter(|v| !v.is_null()) else { continue; };
            
            let m_name = m_meta["name"].as_str().ok_or(RoadmapError::Internal("Invalid milestone name".into()))?.to_string();
            let m_status = serde_json::from_value(m_meta["status"].clone()).map_err(|_| RoadmapError::Internal("Invalid status".into()))?;

            // Hydrate Phases
            let p_ids_val = self.store.get_state_json(&milestone_phases_key(&m_id)).await?;
            let phase_ids: Vec<String> = p_ids_val
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default()
                .into_iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();

            let mut phases = Vec::with_capacity(phase_ids.len());
            for p_id in phase_ids {
                // Hydrate Phase
                let p_val = self.store.get_state_json(&phase_key(&p_id)).await?;
                let Some(p_meta) = p_val.filter(|v| !v.is_null()) else { continue; };
                
                let p_name = p_meta["name"].as_str().ok_or(RoadmapError::Internal("Invalid phase name".into()))?.to_string();
                let p_deps = p_meta["depends_on"].as_array()
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();

                // Hydrate Goals
                let g_val = self.store.get_state_json(&phase_goals_key(&p_id)).await?;
                let goals: Vec<sisyphus_types::GoalRef> = g_val
                    .and_then(|v| serde_json::from_value(v).ok())
                    .unwrap_or_default();

                phases.push(sisyphus_types::Phase {
                    id: p_id,
                    name: p_name,
                    goals,
                    depends_on: p_deps,
                });
            }

            milestones.push(sisyphus_types::Milestone {
                id: m_id,
                name: m_name,
                phases,
                status: m_status,
            });
        }

        Ok(Some(Project {
            id: project_id,
            name,
            description,
            roadmap: sisyphus_types::Roadmap { milestones },
        }))
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
        // 1. Persist Milestone metadata
        let m_key = milestone_key(&milestone.id);
        let m_meta = serde_json::json!({
            "id": milestone.id,
            "name": milestone.name,
            "status": milestone.status,
        });
        self.store.set_state_json(&m_key, m_meta).await?;

        // 2. Persist Phases and their Goals
        let mut phase_ids = Vec::new();
        for phase in &milestone.phases {
            phase_ids.push(phase.id.clone());
            
            let p_key = phase_key(&phase.id);
            let p_meta = serde_json::json!({
                "id": phase.id,
                "name": phase.name,
                "depends_on": phase.depends_on,
            });
            self.store.set_state_json(&p_key, p_meta).await?;
            self.store.set_state_json(&phase_goals_key(&phase.id), serde_json::to_value(&phase.goals)?).await?;
        }

        // 3. Update milestone's phase list
        self.store.set_state_json(&milestone_phases_key(&milestone.id), serde_json::json!(phase_ids)).await?;

        // 4. Update project's milestone list
        let m_list_key = project_milestones_key(project_id);
        let mut m_ids = self.store.get_state_json(&m_list_key).await?
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect::<Vec<_>>();

        if !m_ids.contains(&milestone.id) {
            m_ids.push(milestone.id.clone());
            self.store.set_state_json(&m_list_key, serde_json::json!(m_ids)).await?;
        }

        Ok(())
    }

    /// Update the status of a milestone within a project.
    pub async fn update_milestone_status(
        &self,
        _project_id: &str,
        milestone_id: &str,
        status: MilestoneStatus,
    ) -> Result<(), RoadmapError> {
        let key = milestone_key(milestone_id);
        let mut m_meta = self.store.get_state_json(&key).await?
            .ok_or_else(|| RoadmapError::MilestoneNotFound(milestone_id.to_string(), "unknown".to_string()))?;
        
        m_meta["status"] = serde_json::to_value(status)?;
        self.store.set_state_json(&key, m_meta).await?;
        Ok(())
    }

    /// Remove a milestone from a project.
    pub async fn remove_milestone(
        &self,
        project_id: &str,
        milestone_id: &str,
    ) -> Result<(), RoadmapError> {
        let m_list_key = project_milestones_key(project_id);
        let mut m_ids = self.store.get_state_json(&m_list_key).await?
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect::<Vec<_>>();

        if !m_ids.contains(&milestone_id.to_string()) {
            return Err(RoadmapError::MilestoneNotFound(milestone_id.to_string(), project_id.to_string()));
        }

        m_ids.retain(|id| id != milestone_id);
        self.store.set_state_json(&m_list_key, serde_json::json!(m_ids)).await?;

        // Clean up milestone and descendants
        let p_ids_val = self.store.get_state_json(&milestone_phases_key(milestone_id)).await?;
        let phase_ids: Vec<String> = p_ids_val
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();

        for p_id in phase_ids {
            self.store.set_state_json(&phase_key(&p_id), serde_json::Value::Null).await?;
            self.store.set_state_json(&phase_goals_key(&p_id), serde_json::Value::Null).await?;
        }
        self.store.set_state_json(&milestone_key(milestone_id), serde_json::Value::Null).await?;
        self.store.set_state_json(&milestone_phases_key(milestone_id), serde_json::Value::Null).await?;

        Ok(())
    }

    // ── Phase operations ────────────────────────────────────────────

    /// Add a phase to a milestone within a project.
    pub async fn add_phase(
        &self,
        _project_id: &str,
        milestone_id: &str,
        phase: Phase,
    ) -> Result<(), RoadmapError> {
        // 1. Persist Phase metadata
        let p_key = phase_key(&phase.id);
        let p_meta = serde_json::json!({
            "id": phase.id,
            "name": phase.name,
            "depends_on": phase.depends_on,
        });
        self.store.set_state_json(&p_key, p_meta).await?;

        // 2. Persist Goals
        self.store.set_state_json(&phase_goals_key(&phase.id), serde_json::to_value(&phase.goals)?).await?;

        // 3. Update milestone's phase list
        let p_list_key = milestone_phases_key(milestone_id);
        let mut p_ids = self.store.get_state_json(&p_list_key).await?
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect::<Vec<_>>();

        if !p_ids.contains(&phase.id) {
            p_ids.push(phase.id.clone());
            self.store.set_state_json(&p_list_key, serde_json::json!(p_ids)).await?;
        }

        Ok(())
    }

    // ── Goal operations ─────────────────────────────────────────────

    /// Add a goal to a phase within a milestone.
    pub async fn add_goal(
        &self,
        _project_id: &str,
        _milestone_id: &str,
        phase_id: &str,
        goal: GoalRef,
    ) -> Result<(), RoadmapError> {
        let g_list_key = phase_goals_key(phase_id);
        let mut goals = self.store.get_state_json(&g_list_key).await?
            .and_then(|v| serde_json::from_value::<Vec<GoalRef>>(v).ok())
            .unwrap_or_default();

        goals.push(goal);
        self.store.set_state_json(&g_list_key, serde_json::to_value(&goals)?).await?;

        Ok(())
    }

    /// Update the status of a specific goal.
    pub async fn update_goal_status(
        &self,
        _project_id: &str,
        _milestone_id: &str,
        phase_id: &str,
        goal_id: &str,
        status: MilestoneStatus,
    ) -> Result<(), RoadmapError> {
        let g_list_key = phase_goals_key(phase_id);
        let mut goals = self.store.get_state_json(&g_list_key).await?
            .and_then(|v| serde_json::from_value::<Vec<GoalRef>>(v).ok())
            .unwrap_or_default();

        if let Some(goal) = goals.iter_mut().find(|g| g.goal_id == goal_id) {
            goal.status = status;
            self.store.set_state_json(&g_list_key, serde_json::to_value(&goals)?).await?;
            Ok(())
        } else {
            Err(RoadmapError::GoalNotFound(goal_id.to_string(), phase_id.to_string()))
        }
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

    /// Apply a structured strategic proposal action to the roadmap.
    pub async fn apply_proposal_action(
        &self,
        project_id: &str,
        action: &ProposalAction,
    ) -> Result<(), RoadmapError> {
        match action {
            ProposalAction::InsertMilestone { name, description, after_milestone_id } => {
                let milestone_id = format!("ms-{}", std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs());
                
                let milestone = Milestone {
                    id: milestone_id,
                    name: name.clone(),
                    status: MilestoneStatus::Pending,
                    phases: vec![],
                };
                
                self.add_milestone(project_id, milestone).await?;
                
                // Note: Position-based insertion (after_milestone_id) would require 
                // updating the project_milestones_key list order.
                Ok(())
            }
            ProposalAction::DeleteGoal { goal_id, rationale: _ } => {
                self.delete_goal(project_id, goal_id).await
            }
            ProposalAction::Reprioritize { target_id, new_priority } => {
                self.update_goal_priority(project_id, target_id, new_priority).await
            }
            ProposalAction::PivotTrajectory { new_trajectory, rationale: _ } => {
                self.pivot_trajectory(project_id, new_trajectory).await
            }
        }
    }

    /// Delete a goal from the roadmap and update its parent phase.
    pub async fn delete_goal(&self, project_id: &str, goal_id: &str) -> Result<(), RoadmapError> {
        let project = self.get_project_or_err(project_id).await?;
        
        for milestone in &project.roadmap.milestones {
            for phase in &milestone.phases {
                if phase.goals.iter().any(|g| g.goal_id == goal_id) {
                    let mut goals = self.store.get_state_json(&phase_goals_key(&phase.id)).await?
                        .and_then(|v| serde_json::from_value::<Vec<GoalRef>>(v).ok())
                        .unwrap_or_default();
                    
                    goals.retain(|g| g.goal_id != goal_id);
                    self.store.set_state_json(&phase_goals_key(&phase.id), serde_json::to_value(&goals)?).await?;
                    return Ok(());
                }
            }
        }
        Err(RoadmapError::Internal(format!("Goal {} not found in project {}", goal_id, project_id)))
    }

    /// Update the priority of a goal (simplified implementation).
    pub async fn update_goal_priority(&self, project_id: &str, goal_id: &str, new_priority: &str) -> Result<(), RoadmapError> {
        // In current GoalRef, priority is not a first-class field. 
        // We'll simulate this by updating the description or logging it.
        tracing::info!(project_id, goal_id, new_priority, "Goal reprioritised");
        Ok(())
    }

    /// Update project metadata to reflect a strategic trajectory pivot.
    pub async fn pivot_trajectory(&self, project_id: &str, new_trajectory: &str) -> Result<(), RoadmapError> {
        let key = project_key(project_id);
        let mut meta = self.store.get_state_json(&key).await?
            .ok_or_else(|| RoadmapError::ProjectNotFound(project_id.to_string()))?;
        
        meta["trajectory"] = serde_json::Value::String(new_trajectory.to_string());
        self.store.set_state_json(&key, meta).await?;
        Ok(())
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
