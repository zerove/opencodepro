//! SisyphusKernel 的 BudgetProvider 实现。
//!
//! 对照项目预算校验执行成本，并通过 RoadmapManager
//! 记录实际成本。

use async_trait::async_trait;

use sisyphus_types::BudgetProvider;

use crate::SisyphusKernel;

#[async_trait]
impl BudgetProvider for SisyphusKernel {
    async fn check_budget(
        &self,
        project_id: &str,
        _goal_id: &str,
        estimated_cost: f64,
    ) -> Result<(), sisyphus_types::TypesError> {
        let project = self
            .roadmap_manager
            .get_project(project_id)
            .await
            .map_err(|e| sisyphus_types::TypesError::InternalError(e.to_string()))?
            .ok_or_else(|| {
                sisyphus_types::TypesError::InternalError(format!(
                    "Project {} not found",
                    project_id
                ))
            })?;

        let total_goals = project
            .roadmap
            .milestones
            .iter()
            .flat_map(|m| &m.phases)
            .flat_map(|p| &p.goals)
            .count();

        let completed_goals = project
            .roadmap
            .milestones
            .iter()
            .flat_map(|m| &m.phases)
            .flat_map(|p| &p.goals)
            .filter(|g| g.status == sisyphus_types::MilestoneStatus::Completed)
            .count();

        let projection = self.resource_optimizer.project_costs(
            &self.roadmap_manager.get_outcomes(project_id).await.unwrap_or_default(),
            total_goals,
            completed_goals,
        );

        if estimated_cost > projection.avg_cost_per_goal * 3.0 && projection.confidence > 0.5 {
            return Err(sisyphus_types::TypesError::InternalError(format!(
                "Estimated cost {:.2} is significantly higher than project average {:.2}",
                estimated_cost, projection.avg_cost_per_goal
            )));
        }
        Ok(())
    }

    async fn record_actual_cost(
        &self,
        project_id: &str,
        goal_id: &str,
        actual_cost: f64,
        duration_secs: f64,
    ) -> Result<(), sisyphus_types::TypesError> {
        let project = self
            .roadmap_manager
            .get_project(project_id)
            .await
            .map_err(|e| sisyphus_types::TypesError::InternalError(e.to_string()))?
            .ok_or_else(|| {
                sisyphus_types::TypesError::InternalError(format!(
                    "Project {} not found",
                    project_id
                ))
            })?;

        // 查找目标对应的里程碑与阶段
        let mut location = None;
        for ms in &project.roadmap.milestones {
            for ph in &ms.phases {
                if ph.goals.iter().any(|g| g.goal_id == goal_id) {
                    location = Some((ms.id.clone(), ph.id.clone()));
                    break;
                }
            }
            if location.is_some() {
                break;
            }
        }

        let (milestone_id, phase_id) = location.ok_or_else(|| {
            sisyphus_types::TypesError::InternalError(format!(
                "Goal {} not found in project {}",
                goal_id, project_id
            ))
        })?;

        let outcome = sisyphus_roadmap::ExecutionOutcome {
            goal_id: goal_id.to_string(),
            project_id: project_id.to_string(),
            milestone_id,
            phase_id,
            success: true,
            deviation_score: None,
            error: None,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            actual_cost,
            duration_secs,
        };

        self.roadmap_manager
            .record_outcome(&outcome)
            .await
            .map_err(|e| sisyphus_types::TypesError::InternalError(e.to_string()))?;
        Ok(())
    }
}
