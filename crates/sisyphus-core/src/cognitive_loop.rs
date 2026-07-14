//! Cognitive execution — iterative predict → execute → reflect → evolve loop.
//!
//! This module now primarily serves as the bridge between the SisyphusKernel
//! and the StrategicManager.

use std::sync::Arc;
use tokio::sync::mpsc;

use crate::{CoreError, ExecutionContext, ExecutionEvent, NodeOutput, SisyphusKernel, SOPGraph};

// ---------------------------------------------------------------------------
// SisyphusKernel — cognitive loop delegation
// ---------------------------------------------------------------------------

impl SisyphusKernel {
    /// Execute a SOP graph with recursive cognitive feedback.
    /// Delegated to StrategicManager.
    pub async fn execute_with_cognition(
        &self,
        sop_graph: Arc<SOPGraph>,
        sop_id: &str,
        project_id: Option<&str>,
        initial_ctx: Option<Arc<ExecutionContext>>,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
        stream_tx: Option<mpsc::UnboundedSender<sisyphus_types::CognitiveEvent>>,
    ) -> Result<(NodeOutput, crate::strategic_manager::CognitiveReport, Option<Arc<SOPGraph>>), CoreError> {
        let goal_text = self.roadmap_manager.get_goal_description(project_id.unwrap_or("default"), sop_id).await
            .ok()
            .flatten();
        
        self.strategic_manager.run_cognitive_loop(
            sop_graph,
            sop_id,
            project_id,
            goal_text.as_deref(),
            initial_ctx,
            event_tx,
            |sid, event| self.broadcast_event(sid, event),
            stream_tx,
            self.micro_reflector.clone(),
        ).await
    }
}
