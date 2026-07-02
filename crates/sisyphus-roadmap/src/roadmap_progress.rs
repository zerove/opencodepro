use crate::types::*;
use crate::RoadmapError;
use crate::RoadmapManager;
use sisyphus_types::GoalRef;
use sisyphus_types::MilestoneStatus;

impl RoadmapManager {
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
                let ph_done = ph
                    .goals
                    .iter()
                    .filter(|g| g.status == MilestoneStatus::Completed)
                    .count();
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
                    progress: if ph_goals > 0 {
                        ph_done as f64 / ph_goals as f64
                    } else {
                        0.0
                    },
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
            overall_progress: if total_goals > 0 {
                completed_goals as f64 / total_goals as f64
            } else {
                0.0
            },
            milestone_count: m_count,
            total_goals,
            completed_goals,
            milestones: milestones_progress,
        })
    }

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
                status_icon,
                ms.name,
                ms.progress * 100.0,
                ms.completed_count,
                ms.goal_count,
            ));

            for ph in &ms.phases {
                lines.push(format!(
                    "      Phase {}: {:.1}% ({}/{})",
                    ph.name,
                    ph.progress * 100.0,
                    ph.completed_count,
                    ph.goal_count,
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

        let all_goals: Vec<&GoalRef> = milestone.phases.iter().flat_map(|p| &p.goals).collect();

        if all_goals.is_empty() {
            milestone.status = MilestoneStatus::Pending;
        } else if all_goals
            .iter()
            .all(|g| g.status == MilestoneStatus::Completed)
        {
            milestone.status = MilestoneStatus::Completed;
        } else if all_goals
            .iter()
            .any(|g| g.status == MilestoneStatus::Blocked)
        {
            milestone.status = MilestoneStatus::Blocked;
        } else if all_goals
            .iter()
            .any(|g| g.status == MilestoneStatus::InProgress)
        {
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
            } else if all_goals
                .iter()
                .all(|g| g.status == MilestoneStatus::Completed)
            {
                MilestoneStatus::Completed
            } else if all_goals
                .iter()
                .any(|g| g.status == MilestoneStatus::Blocked)
            {
                MilestoneStatus::Blocked
            } else if all_goals
                .iter()
                .any(|g| g.status == MilestoneStatus::InProgress)
            {
                MilestoneStatus::InProgress
            } else {
                MilestoneStatus::Pending
            };
        }

        self.update_project(&project).await
    }
}
