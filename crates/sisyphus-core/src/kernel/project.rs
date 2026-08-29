//! 内核项目工作流：路线图执行与里程碑生成。
//!
//! `SisyphusKernel` 实现的一部分，为可读性从 `kernel.rs` 拆出。
//! 通过 `pub use kernel::*` 重新导出。

use tokio::sync::mpsc;

use crate::*;

impl SisyphusKernel {
    /// Strategic reflect.
    pub async fn strategic_reflect(
        &self,

        project_id: &str,
    ) -> Option<sisyphus_roadmap::strategic_reflection::StrategicReflection> {
        let reflector = self.strategic_reflector.as_ref()?;

        match reflector.reflect(&self.roadmap_manager, project_id).await {
            Ok(reflection) => {
                let _ = self.channel.publish(
                    "strategic_events",
                    AgentMessage {
                        from: "kernel".into(),

                        to: "*".into(),

                        kind: "StrategicReflectionCompleted".into(),

                        payload: serde_json::json!({

                            "project_id": reflection.project_id,

                            "health_assessment": reflection.health_assessment,

                            "trajectory": reflection.trajectory,

                            "recommendations": reflection.recommendations.len(),

                            "risk_factors": reflection.risk_factors.len(),

                            "confidence": reflection.confidence,

                        }),

                        correlation_id: format!("{}-strategic-reflect", project_id),
                    },
                );

                if let Some(proposal) = reflector.propose_pivot(&reflection, project_id) {
                    let prop_id = proposal.proposal_id.clone();

                    if let Err(e) = self
                        .proposal_manager
                        .submit_proposal(&proposal.project_id, proposal.clone())
                        .await
                    {
                        tracing::warn!(project_id = %project_id, error = %e, "Failed to submit strategic proposal");
                    } else {
                        let _ = self.channel.publish(
                            "strategic_events",
                            AgentMessage {
                                from: "kernel".into(),

                                to: "*".into(),

                                kind: "StrategicProposalCreated".into(),

                                payload: serde_json::json!({

                                    "proposal_id": prop_id,

                                    "project_id": project_id,

                                    "description": proposal.expected_impact,

                                }),

                                correlation_id: format!("{}-proposal-{}", project_id, prop_id),
                            },
                        );
                    }
                }

                Some(reflection)
            }

            Err(e) => {
                tracing::warn!(project_id = %project_id, error = %e, "Strategic reflection failed");

                None
            }
        }
    }

    /// Generate milestones.
    pub async fn generate_milestones(
        &self,

        project_id: &str,

        description: &str,
    ) -> Option<Vec<sisyphus_types::Milestone>> {
        let generator = self.milestone_generator.as_ref()?;

        match generator
            .generate_milestones(&self.roadmap_manager, project_id, description)
            .await
        {
            Ok(milestones) => {
                let _ = self.channel.publish(
                    "strategic_events",
                    AgentMessage {
                        from: "kernel".into(),

                        to: "*".into(),

                        kind: "MilestonesGenerated".into(),

                        payload: serde_json::json!({

                            "project_id": project_id,

                            "milestone_count": milestones.len(),

                        }),

                        correlation_id: format!("{}-milestones", project_id),
                    },
                );

                Some(milestones)
            }

            Err(e) => {
                tracing::warn!(project_id = %project_id, error = %e, "Milestone generation failed");

                None
            }
        }
    }

    /// 通过选取下一个可用目标来自主执行项目路线图。
    pub async fn run_project_roadmap(
        &self,

        project_id: &str,

        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
    ) -> Result<(), CoreError> {
        tracing::info!(project_id, "Starting autonomous roadmap execution");

        loop {
            // 1. 主动健康检查：在执行前仿真剩余路线图，检测潜在风险。

            if let Some(_reflector) = self.strategic_reflector.as_ref() {
                if let Some(reflection) = self.strategic_reflect(project_id).await {
                    if reflection.confidence > 0.7 && !reflection.risk_factors.is_empty() {
                        tracing::warn!(

                            project_id,

                            risks = ?reflection.risk_factors,

                            "Proactive reflection detected risks. Consider reviewing roadmap."

                        );

                        // 我们不中断循环，但生成的提案会进入待审批状态。
                    }
                }
            }

            // 2. 查找下一个待处理目标

            let next_goal_ref = self
                .roadmap_manager
                .get_next_goal(project_id)
                .await
                .map_err(|e| CoreError::Internal(format!("Failed to get next goal: {}", e)))?;

            let goal_ref = match next_goal_ref {
                Some(g) => g,

                None => {
                    tracing::info!(
                        project_id,
                        "No more pending goals in roadmap. Project complete."
                    );

                    break;
                }
            };

            // 2. 解析目标上下文（里程碑、阶段）

            let project = self
                .roadmap_manager
                .get_project_or_err(project_id)
                .await
                .map_err(|e| CoreError::Internal(format!("Project not found: {}", e)))?;

            let mut milestone_id = None;

            let mut phase_id = None;

            for ms in &project.roadmap.milestones {
                for ph in &ms.phases {
                    if ph.goals.iter().any(|g| g.goal_id == goal_ref.goal_id) {
                        milestone_id = Some(ms.id.clone());

                        phase_id = Some(ph.id.clone());

                        break;
                    }
                }

                if milestone_id.is_some() {
                    break;
                }
            }

            let (m_id, p_id) = match (milestone_id, phase_id) {
                (Some(m), Some(p)) => (m, p),

                _ => {
                    return Err(CoreError::Internal(format!(
                        "Goal {} not found in any phase/milestone",
                        goal_ref.goal_id
                    )))
                }
            };

            // 3. 执行目标

            tracing::info!(project_id, goal_id = %goal_ref.goal_id, "Executing next goal from roadmap");

            let result = self
                .execute_and_record(
                    &goal_ref.description,
                    &format!("sop-{}", goal_ref.goal_id),
                    project_id,
                    &m_id,
                    &p_id,
                    &goal_ref.goal_id,
                    event_tx.clone(),
                )
                .await;

            match result {
                Ok(_) => {
                    tracing::info!(project_id, goal_id = %goal_ref.goal_id, "Successfully completed goal");
                }

                Err(CoreError::StrategicPivot(reason)) => {
                    tracing::warn!(project_id, goal_id = %goal_ref.goal_id, reason, "Strategic pivot triggered. Re-evaluating roadmap...");
                }

                Err(e) => {
                    tracing::error!(project_id, goal_id = %goal_ref.goal_id, error = %e, "Goal execution failed");

                    return Err(e);
                }
            }
        }

        Ok(())
    }

    /// 从高层愿景初始化新项目，自动生成
    /// 包含里程碑、阶段与目标的结构化路线图。
    pub async fn project_costs(
        &self,

        project_id: &str,

        total_goals: usize,

        completed_goals: usize,
    ) -> sisyphus_roadmap::types::CostProjection {
        let outcomes = self
            .roadmap_manager
            .get_outcomes(project_id)
            .await
            .unwrap_or_default();

        self.resource_optimizer
            .project_costs(&outcomes, total_goals, completed_goals)
    }

    /// 从高层愿景初始化新项目，自动生成
    /// 包含里程碑、阶段与目标的结构化路线图。
    pub async fn initialize_project_from_vision(
        &self,

        project_id: &str,

        vision: &str,
    ) -> Result<sisyphus_types::Project, CoreError> {
        tracing::info!(project_id, vision = %vision, "Initializing project from vision");

        // 1. 创建项目元数据

        let project_name = project_id.replace('_', " ").to_uppercase(); // Simple fallback name

        let project = self
            .roadmap_manager
            .create_project(project_id, &project_name, vision)
            .await
            .map_err(|e| CoreError::Internal(format!("Failed to create project: {}", e)))?;

        // 2. 生成路线图里程碑

        if let Some(ref generator) = self.milestone_generator {
            tracing::info!(project_id, "Generating autonomous roadmap milestones...");

            generator
                .generate_milestones(&self.roadmap_manager, project_id, vision)
                .await
                .map_err(|e| CoreError::Internal(format!("Milestone generation failed: {}", e)))?;
        } else {
            return Err(CoreError::Internal(
                "Milestone generator not configured".into(),
            ));
        }

        tracing::info!(project_id, "Project successfully initialized from vision");

        Ok(project)
    }
}
