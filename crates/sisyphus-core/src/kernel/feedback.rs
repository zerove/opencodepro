//! 内核反馈：SOP 捕获、事件广播与提案审批。
//!
//! `SisyphusKernel` 实现的一部分，为可读性从 `kernel.rs` 拆出。
//! 通过 `pub use kernel::*` 重新导出。

use std::sync::Arc;

use crate::*;

impl SisyphusKernel {
    /// Handle feedback.
    pub async fn handle_feedback(
        &self,

        feedback: sisyphus_types::ExecutionFeedback,

        project_id: &str,
    ) -> Result<sisyphus_types::EvolutionResponse, CoreError> {
        tracing::info!(

            plan_id = %feedback.plan_id,

            step_id = %feedback.step_id,

            status = ?feedback.status,

            "Handling execution feedback"

        );

        let mut trace = String::new();

        if let Ok(Some(val)) = self
            .execution_manager
            .state_store
            .get_state_json(project_id, &format!("history:{}", feedback.plan_id))
            .await
        {
            trace = val.to_string();
        }

        let sop = self
            .load_sop(&feedback.plan_id, project_id)
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))?;

        let prediction = self
            .intelligence_manager
            .predict_outcome(sop, &ExecutionContext::new(), project_id, &feedback.plan_id)
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))?;

        let reflection = self
            .intelligence_manager
            .reflect_on_trace(&trace, &prediction, &ExecutionContext::new())
            .await
            .map_err(|e| CoreError::Internal(format!("Reflection failed: {e}")))?;

        let should_pivot = feedback.status == sisyphus_types::FeedbackStatus::Failed
            || reflection.deviation_score > 0.7;

        let mut revised_plan = None;

        if should_pivot {
            tracing::warn!(plan_id = %feedback.plan_id, "Strategic pivot triggered by feedback");

            let augmented_goal = format!(
                "Original Goal: {}\nFailure in step {}: {}\nSuggested Refinement: {}",
                feedback.plan_id,
                feedback.step_id,
                feedback.error.as_deref().unwrap_or("Unknown error"),
                reflection
                    .suggested_improvements
                    .first()
                    .cloned()
                    .unwrap_or_default()
            );

            let ctx = sisyphus_types::ExecutionContext::new();

            let synthesis = self
                .intelligence_manager
                .synthesize_goal(
                    &augmented_goal,
                    &ctx,
                    project_id,
                    sisyphus_types::WorkMode::Standard,
                    None,
                )
                .await
                .map_err(|e| CoreError::Internal(e.to_string()))?;

            let config = sisyphus_types::SOPGraphConfig::from_graph(&synthesis.graph)
                .map_err(|e| CoreError::Internal(e.to_string()))?;

            revised_plan =
                Some(config.into_cognitive_plan(feedback.plan_id.clone(), augmented_goal.clone()));
        }

        Ok(sisyphus_types::EvolutionResponse {
            should_pivot,

            revised_plan,

            reflection_report: reflection
                .suggested_improvements
                .first()
                .cloned()
                .unwrap_or_default(),
        })
    }

    /// Capture success.
    pub async fn capture_success(
        &self,

        plan_id: &str,

        project_id: &str,

        final_plan: sisyphus_types::CognitivePlan,
    ) -> Result<(), CoreError> {
        tracing::info!(plan_id = %plan_id, "Capturing successful cognitive plan as SOP");

        let mut graph = SOPGraph::new();

        let mut id_map = HashMap::new();

        for step in &final_plan.steps {
            let tool_id = match &step.action {
                sisyphus_types::CognitiveAction::ToolCall { tool_id, .. } => tool_id.clone(),

                sisyphus_types::CognitiveAction::FileEdit { path, .. } => {
                    format!("file_edit:{}", path)
                }

                sisyphus_types::CognitiveAction::ShellExecute { .. } => "shell_exec_tool".into(),
            };

            let idx = graph.add_node(Arc::new(WasmSOPNode {
                name: step.name.clone(),

                tool_id,

                fuel_limit: 1_000_000,

                executor: self.execution_manager.tool_router.clone(),
            }));

            id_map.insert(step.id.clone(), idx);
        }

        for edge in &final_plan.edges {
            if let (Some(&src), Some(&tgt)) = (id_map.get(&edge.from), id_map.get(&edge.to)) {
                graph.add_edge(src, tgt);
            }
        }

        let sop_id = format!("exp_{}", plan_id);

        self.save_sop(&sop_id, project_id, &graph).await
    }

    /// Save sop.
    pub async fn save_sop(
        &self,
        sop_id: &str,
        project_id: &str,
        graph: &SOPGraph,
    ) -> Result<(), CoreError> {
        let config = sisyphus_types::SOPGraphConfig::from_graph(graph)
            .map_err(|e| CoreError::Internal(e.to_string()))?;

        let value =
            serde_json::to_value(&config).map_err(|e| CoreError::Internal(e.to_string()))?;

        if self.consensus.is_leader().await {
            if let Err(e) = self.consensus.propose(sop_id, value.clone()).await {
                tracing::warn!(sop_id = %sop_id, error = %e, "Consensus proposal failed, falling back to local save");
            }
        }

        self.execution_manager
            .save_sop(sop_id, project_id, graph)
            .await
    }

    /// Load sop.
    pub async fn load_sop(
        &self,
        sop_id: &str,
        project_id: &str,
    ) -> Result<Arc<SOPGraph>, CoreError> {
        self.execution_manager.load_sop(sop_id, project_id).await
    }

    /// Broadcast event.
    pub fn broadcast_event(&self, sop_id: &str, event: ExecutionEvent) {
        crate::cognitive_dashboard::CognitiveDashboard::render(&event);

        self.execution_manager.broadcast_event(sop_id, event);
    }

    /// Approve and apply proposal.
    pub async fn approve_and_apply_proposal(
        &self,

        project_id: &str,
        proposal_id: &str,

        reviewer: &str,
    ) -> Result<(), CoreError> {
        let proposal = self
            .proposal_manager
            .get_proposal(project_id, proposal_id)
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))?
            .ok_or_else(|| CoreError::Internal(format!("Proposal {} not found", proposal_id)))?;

        self.proposal_manager
            .approve_proposal(project_id, proposal_id, reviewer)
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))?;

        self.roadmap_manager
            .apply_proposal_action(&proposal.project_id, &proposal.action)
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))?;

        let _ = self.channel.publish(
            "strategic_events",
            AgentMessage {
                from: "kernel".into(),

                to: "*".into(),

                kind: "StrategicProposalApplied".into(),

                payload: serde_json::json!({

                    "proposal_id": proposal_id,

                    "project_id": proposal.project_id,

                    "action": proposal.action,

                }),

                correlation_id: format!("{}-apply-{}", proposal.project_id, proposal_id),
            },
        );

        Ok(())
    }
}
