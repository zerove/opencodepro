use crate::strategic_reflection::ProposalAction;
use crate::RoadmapError;
use anyhow::Result;
use sisyphus_mem::{MfsStore, RoadmapSqlStore};
use sisyphus_types::{GoalRef, Milestone, MilestoneStatus, Phase, Project, Roadmap};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// RoadmapManager
// ---------------------------------------------------------------------------
// RoadmapManager
// ---------------------------------------------------------------------------

/// Strategic planning manager that provides CRUD operations over a
/// Project → Milestone → Phase → Goal hierarchy, persisted via MfsStore.
pub struct RoadmapManager {
    pub(crate) store: Arc<MfsStore>,
    pub(crate) sql_store: Arc<RoadmapSqlStore>,
    pub(crate) proposal_manager: Arc<crate::proposal_manager::ProposalManager>,
}

impl RoadmapManager {
    /// Create a new `RoadmapManager` backed by the given `MfsStore` and `RoadmapSqlStore`.
    pub fn new(
        store: Arc<MfsStore>,
        sql_store: Arc<RoadmapSqlStore>,
        proposal_manager: Arc<crate::proposal_manager::ProposalManager>,
    ) -> Self {
        Self {
            store,
            sql_store,
            proposal_manager,
        }
    }

    // ── Project CRUD ────────────────────────────────────────────────

    /// Create a new project and persist it.
    pub async fn create_project(
        &self,
        id: &str,
        name: &str,
        description: &str,
    ) -> Result<Project, RoadmapError> {
        let project = Project::new(id.to_string(), name.to_string(), description.to_string());

        self.sql_store.insert_project(&project).await.map_err(|e| {
            if e.to_string()
                .contains("UNIQUE constraint failed: projects.id")
            {
                RoadmapError::DuplicateProject(id.to_string())
            } else {
                RoadmapError::Internal(e.to_string())
            }
        })?;

        Ok(project)
    }

    /// Retrieve a project by its ID and hydrate it from granular storage.
    pub async fn get_project(&self, id: &str) -> Result<Option<Project>, RoadmapError> {
        self.sql_store
            .get_project(id)
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))
    }

    /// Update an existing project (replaces the full document).
    pub async fn update_project(&self, project: &Project) -> Result<(), RoadmapError> {
        self.sql_store
            .insert_project(project)
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))
    }

    /// Delete a project by its ID.
    pub async fn delete_project(&self, id: &str) -> Result<(), RoadmapError> {
        self.sql_store
            .delete_project(id)
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))?;
        Ok(())
    }

    /// List all project IDs.
    pub async fn list_project_ids(&self) -> Result<Vec<String>, RoadmapError> {
        self.sql_store
            .list_projects()
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))
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
        self.sql_store
            .insert_milestone(
                &milestone.id,
                project_id,
                &milestone.name,
                &milestone.status.to_string(),
            )
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))?;

        for phase in &milestone.phases {
            let deps = phase.depends_on.join(",");
            self.sql_store
                .insert_phase(
                    project_id,
                    &phase.id,
                    &milestone.id,
                    &phase.name,
                    Some(deps),
                )
                .await
                .map_err(|e| RoadmapError::Internal(e.to_string()))?;

            for goal in &phase.goals {
                self.sql_store
                    .insert_goal(
                        project_id,
                        &goal.goal_id,
                        &phase.id,
                        &goal.description,
                        &goal.status.to_string(),
                    )
                    .await
                    .map_err(|e| RoadmapError::Internal(e.to_string()))?;
            }
        }
        Ok(())
    }

    /// Update the status of a milestone within a project.
    pub async fn update_milestone_status(
        &self,
        project_id: &str,
        milestone_id: &str,
        status: MilestoneStatus,
    ) -> Result<(), RoadmapError> {
        self.sql_store
            .update_milestone_status(project_id, milestone_id, &status.to_string())
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))
    }

    /// Remove a milestone from a project.
    pub async fn remove_milestone(
        &self,
        project_id: &str,
        milestone_id: &str,
    ) -> Result<(), RoadmapError> {
        if self
            .sql_store
            .get_project(project_id)
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))?
            .is_none()
        {
            return Err(RoadmapError::ProjectNotFound(project_id.to_string()));
        }

        self.sql_store
            .delete_milestone(project_id, milestone_id)
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))?;

        Ok(())
    }

    // ── Phase operations ────────────────────────────────────────────

    /// Add a phase to a milestone within a project.
    pub async fn add_phase(
        &self,
        project_id: &str,
        milestone_id: &str,
        phase: Phase,
    ) -> Result<(), RoadmapError> {
        let deps = phase.depends_on.join(",");
        self.sql_store
            .insert_phase(project_id, &phase.id, milestone_id, &phase.name, Some(deps))
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))?;

        for goal in &phase.goals {
            self.sql_store
                .insert_goal(
                    project_id,
                    &goal.goal_id,
                    &phase.id,
                    &goal.description,
                    &goal.status.to_string(),
                )
                .await
                .map_err(|e| RoadmapError::Internal(e.to_string()))?;
        }
        Ok(())
    }

    /// Submits a strategic shift proposal based on cluster failure analysis.
    pub async fn submit_strategic_shift(
        &self,
        project_id: &str,
        common_failure_mode: &str,
        evidence_count: usize,
        suggested_shift: &str,
    ) -> Result<(), RoadmapError> {
        let proposal_id = format!(
            "shift-{}-{}",
            project_id,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        );

        let proposal = crate::strategic_reflection::StrategicProposal {
            proposal_id,
            project_id: project_id.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            trigger_reflection: crate::strategic_reflection::StrategicReflection {
                project_id: project_id.to_string(),
                project_name: "Cluster Failure Detected".to_string(),
                health_assessment: format!(
                    "Systematic failure detected in {} similar tasks.",
                    evidence_count
                ),
                trajectory: "Stagnating".to_string(),
                risk_factors: vec![common_failure_mode.to_string()],
                recommendations: vec![crate::strategic_reflection::StrategicRecommendation {
                    action: suggested_shift.to_string(),
                    rationale: format!(
                        "Cluster of {} failures showing pattern: {}",
                        evidence_count, common_failure_mode
                    ),
                    priority: "high".to_string(),
                    target_milestone: None,
                }],
                confidence: 0.9,
            },
            action: crate::strategic_reflection::ProposalAction::PivotTrajectory {
                new_trajectory: suggested_shift.to_string(),
                rationale: format!("Systematic failure mode: {}", common_failure_mode),
            },
            expected_impact: "Resolution of systemic failure cluster".to_string(),
            status: crate::strategic_reflection::ProposalStatus::Pending,
            reviewer: None,
        };

        self.proposal_manager
            .submit_proposal(project_id, proposal)
            .await
    }

    /// Add a goal to a phase within a milestone.
    pub async fn add_goal(
        &self,
        project_id: &str,
        _milestone_id: &str,
        _phase_id: &str,
        goal: GoalRef,
    ) -> Result<(), RoadmapError> {
        self.sql_store
            .insert_goal(
                project_id,
                &goal.goal_id,
                _phase_id,
                &goal.description,
                &goal.status.to_string(),
            )
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))
    }

    /// Update the status of a specific goal.
    pub async fn update_goal_status(
        &self,
        project_id: &str,
        _milestone_id: &str,
        _phase_id: &str,
        goal_id: &str,
        status: MilestoneStatus,
    ) -> Result<(), RoadmapError> {
        self.sql_store
            .update_goal_status(project_id, goal_id, &status.to_string())
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))
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
    pub async fn get_project_or_err(&self, id: &str) -> Result<Project, RoadmapError> {
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
            ProposalAction::InsertMilestone {
                name,
                description: _description,
                after_milestone_id: _after_milestone_id,
            } => {
                let milestone_id = format!(
                    "ms-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                );

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
            ProposalAction::DeleteGoal {
                goal_id,
                rationale: _,
            } => self.delete_goal(project_id, goal_id).await,
            ProposalAction::Reprioritize {
                target_id,
                new_priority,
            } => {
                self.update_goal_priority(project_id, target_id, new_priority)
                    .await
            }
            ProposalAction::PivotTrajectory {
                new_trajectory,
                rationale: _,
            } => self.pivot_trajectory(project_id, new_trajectory).await,
        }
    }

    /// Delete a goal from the roadmap and update its parent phase.
    pub async fn delete_goal(&self, project_id: &str, goal_id: &str) -> Result<(), RoadmapError> {
        self.sql_store
            .delete_goal(project_id, goal_id)
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))
    }

    pub async fn get_goal_description(
        &self,
        project_id: &str,
        goal_id: &str,
    ) -> Result<Option<String>, RoadmapError> {
        self.sql_store
            .get_goal_description(project_id, goal_id)
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))
    }

    /// Update the priority of a goal (simplified implementation).
    pub async fn update_goal_priority(
        &self,
        project_id: &str,
        goal_id: &str,
        new_priority: &str,
    ) -> Result<(), RoadmapError> {
        // In current GoalRef, priority is not a first-class field.
        // We'll simulate this by updating the description or logging it.
        tracing::info!(project_id, goal_id, new_priority, "Goal reprioritised");
        Ok(())
    }

    /// Update project metadata to reflect a strategic trajectory pivot.
    pub async fn pivot_trajectory(
        &self,
        project_id: &str,
        new_trajectory: &str,
    ) -> Result<(), RoadmapError> {
        self.sql_store
            .update_project_trajectory(project_id, new_trajectory)
            .await
            .map_err(|e| RoadmapError::Internal(e.to_string()))
    }

    /// Load the project index from the store.
    /// Migrates data from legacy MfsStore (JSON blobs) to structured RoadmapSqlStore.
    pub async fn migrate_to_sql(&self, sql_store: &RoadmapSqlStore) -> Result<usize> {
        let project_ids = self
            .store
            .list_keys_with_prefix(None, "roadmap:project:")
            .await?
            .into_iter()
            .filter(|k| {
                !k.contains(":milestones")
                    && !k.contains(":milestone:")
                    && !k.contains(":phase:")
                    && !k.contains(":goals")
            })
            .collect::<Vec<_>>();

        let mut migrated_count = 0;

        for pid in project_ids {
            if let Some(project) = self.get_project(&pid).await? {
                sql_store.insert_project(&project).await?;
                for milestone in &project.roadmap.milestones {
                    sql_store
                        .insert_milestone(
                            &milestone.id,
                            &pid,
                            &milestone.name,
                            &milestone.status.to_string(),
                        )
                        .await?;
                    for phase in &milestone.phases {
                        let deps = phase.depends_on.join(",");
                        sql_store
                            .insert_phase(&pid, &phase.id, &milestone.id, &phase.name, Some(deps))
                            .await?;
                        for goal in &phase.goals {
                            sql_store
                                .insert_goal(
                                    &pid,
                                    &goal.goal_id,
                                    &phase.id,
                                    &goal.description,
                                    &goal.status.to_string(),
                                )
                                .await?;
                        }
                    }
                }
                migrated_count += 1;
            }
        }
        Ok(migrated_count)
    }
}
