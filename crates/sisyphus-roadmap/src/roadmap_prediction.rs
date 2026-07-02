use crate::types::*;
use crate::RoadmapError;
use crate::RoadmapManager;
use sisyphus_types::MilestoneStatus;

impl RoadmapManager {
    /// Find the next pending goal, respecting phase-level dependencies.
    ///
    /// Scans milestones in order, skipping completed ones. Within each
    /// milestone, phases whose dependencies are not met are skipped.
    /// Returns the first pending goal found, or `None` if all are done.
    pub async fn get_next_goal(
        &self,
        project_id: &str,
    ) -> Result<Option<sisyphus_types::GoalRef>, RoadmapError> {
        let project = self.get_project_or_err(project_id).await?;

        for milestone in &project.roadmap.milestones {
            if milestone.status == MilestoneStatus::Completed {
                continue;
            }

            for phase in &milestone.phases {
                let dependencies_met = phase.depends_on.iter().all(|dep_id| {
                    project
                        .roadmap
                        .milestones
                        .iter()
                        .flat_map(|m| &m.phases)
                        .any(|p| {
                            p.id == *dep_id
                                && p.goals
                                    .iter()
                                    .all(|g| g.status == MilestoneStatus::Completed)
                        })
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

        let total_goals: usize = project
            .roadmap
            .milestones
            .iter()
            .flat_map(|m| &m.phases)
            .map(|p| p.goals.len())
            .sum();
        let completed_goals: usize = project
            .roadmap
            .milestones
            .iter()
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
            if total_goals > 0 {
                completed_goals as f64 / total_goals as f64 * 100.0
            } else {
                0.0
            },
        );

        for milestone in &project.roadmap.milestones {
            let ms_progress = milestone.calculate_progress();
            context.push_str(&format!(
                "Milestone: {} [{:.0}%]\n",
                milestone.name,
                ms_progress * 100.0
            ));
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
                    context.push_str(&format!(
                        "{}{} ({:?})\n",
                        marker, goal.description, goal.status
                    ));
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

            let progress = if ms_total > 0 {
                ms_completed as f64 / ms_total as f64
            } else {
                1.0
            };
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

        let overall_pct = if total_goals > 0 {
            completed_goals as f64 / total_goals as f64 * 100.0
        } else {
            0.0
        };
        let remaining_goals = total_goals.saturating_sub(completed_goals);

        // highest progress first
        let mut sorted: Vec<&MilestonePrediction> = milestone_preds
            .iter()
            .filter(|m| m.risk_level != "done")
            .collect();
        sorted.sort_by(|a, b| {
            b.progress_pct
                .partial_cmp(&a.progress_pct)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let completion_order: Vec<String> = sorted.iter().map(|m| m.name.clone()).collect();

        let overall_risk = if risk_levels.contains(&"high") {
            "high"
        } else if risk_levels.contains(&"medium") {
            "medium"
        } else {
            "low"
        }
        .to_string();

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
}
