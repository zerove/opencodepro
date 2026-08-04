use crate::types::*;
use crate::RoadmapError;
use crate::RoadmapManager;
use sisyphus_types::GoalRef;
use sisyphus_types::MilestoneStatus;

impl RoadmapManager {
    /// 生成含逐里程碑与逐阶段明细的详细进度报告。
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

    /// 将项目当前进度格式化为人类可读的上下文字符串。
    ///
    /// 设计用于注入 `ExecutionContext`，使执行目标的
    /// 代理了解更广泛的项目状态。
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

    /// 更新目标状态并自动重算里程碑状态。
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

        let project = self.get_project_or_err(project_id).await?;
        let milestone = project
            .roadmap
            .milestones
            .iter()
            .find(|m| m.id == milestone_id)
            .ok_or_else(|| {
                RoadmapError::MilestoneNotFound(milestone_id.to_string(), project_id.to_string())
            })?;

        let all_goals: Vec<&GoalRef> = milestone.phases.iter().flat_map(|p| &p.goals).collect();

        let new_status = if all_goals.is_empty() {
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

        self.update_milestone_status(project_id, milestone_id, new_status)
            .await
    }

    /// 自动传播：根据目标完成情况更新所有里程碑状态。
    pub async fn auto_propagate_all(&self, project_id: &str) -> Result<(), RoadmapError> {
        let project = self.get_project_or_err(project_id).await?;

        for ms in &project.roadmap.milestones {
            let all_goals: Vec<&GoalRef> = ms.phases.iter().flat_map(|p| &p.goals).collect();

            let new_status = if all_goals.is_empty() {
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

            self.update_milestone_status(project_id, &ms.id, new_status)
                .await?;
        }

        Ok(())
    }
}
