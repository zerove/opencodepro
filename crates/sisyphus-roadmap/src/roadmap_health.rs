use serde_json::Value;

use crate::types::*;
use crate::RoadmapError;
use crate::RoadmapManager;
use sisyphus_types::{GoalRef, MilestoneStatus, StateStore};

impl RoadmapManager {
    /// Record a goal execution outcome into persistent storage.
    ///
    /// Outcomes are keyed by `(project, milestone, phase, goal)` so each
    /// goal retains only its **most recent** outcome.
    pub async fn record_outcome(&self, outcome: &ExecutionOutcome) -> Result<(), RoadmapError> {
        let key = format!(
            "roadmap:outcome:{}:{}:{}:{}",
            outcome.project_id, outcome.milestone_id, outcome.phase_id, outcome.goal_id
        );
        let value = serde_json::to_value(outcome)?;
        self.store
            .set_state_json(&outcome.project_id, &key, value)
            .await?;

        // Update outcome index (deduplicated list of (project,ms,ph,goal) tuples)
        let idx_key = format!("roadmap:outcomes:{}", outcome.project_id);
        let mut ids: Vec<String> = match self
            .store
            .get_state_json(&outcome.project_id, &idx_key)
            .await?
        {
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
            self.store
                .set_state_json(&outcome.project_id, &idx_key, arr)
                .await?;
        }

        Ok(())
    }

    /// Retrieve all recorded outcomes for a project.
    pub async fn get_outcomes(
        &self,
        project_id: &str,
    ) -> Result<Vec<ExecutionOutcome>, RoadmapError> {
        let idx_key = format!("roadmap:outcomes:{}", project_id);
        let ids: Vec<String> = match self.store.get_state_json(project_id, &idx_key).await? {
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
                let key = format!(
                    "roadmap:outcome:{}:{}:{}:{}",
                    project_id, parts[0], parts[1], parts[2]
                );
                if let Some(v) = self.store.get_state_json(project_id, &key).await? {
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

            let has_blocked_goal = ms.phases.iter().any(|ph| {
                ph.goals
                    .iter()
                    .any(|g| g.status == MilestoneStatus::Blocked)
            });
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
                let has_completed = ph
                    .goals
                    .iter()
                    .any(|g| g.status == MilestoneStatus::Completed);
                let pending: Vec<&GoalRef> = ph
                    .goals
                    .iter()
                    .filter(|g| g.status == MilestoneStatus::Pending)
                    .collect();

                if has_completed && !pending.is_empty() && ms.status != MilestoneStatus::Blocked {
                    for g in &pending {
                        stale_goals.push(g.goal_id.clone());
                    }
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
                            problem: format!(
                                "Goal '{}' has recent execution failures",
                                g.description
                            ),
                            action: "Consider marking as Blocked and adding a remediation goal"
                                .into(),
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

        let project = self.get_project_or_err(project_id).await?;

        for ms in &project.roadmap.milestones {
            for ph in &ms.phases {
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

                let to_block: Vec<(String, String, String)> = ph
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
                            ph.goals
                                .iter()
                                .any(|pg| pg.goal_id == *d && failed_in_phase.contains(&pg.goal_id))
                        });
                        let any_failed_before = failed_positions.iter().any(|fp| *fp < *i);
                        depends_on_failed || any_failed_before
                    })
                    .map(|(_, g)| (ms.id.clone(), ph.id.clone(), g.goal_id.clone()))
                    .collect();

                for (m_id, p_id, g_id) in to_block {
                    self.update_goal_status(
                        project_id,
                        &m_id,
                        &p_id,
                        &g_id,
                        MilestoneStatus::Blocked,
                    )
                    .await?;
                }
            }
        }

        // Ensure milestone statuses are propagated after blocking goals
        self.auto_propagate_all(project_id).await?;

        // Re-evaluate health after blocking
        self.evaluate_health(project_id).await
    }
}
