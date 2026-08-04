//! 认知执行 — 迭代的 预测 → 执行 → 反思 → 演进 循环。
//!
//! 本模块现在主要作为 SisyphusKernel 与
//! StrategicManager 之间的桥梁。

use std::sync::Arc;
use tokio::sync::mpsc;

use crate::{CoreError, ExecutionContext, ExecutionEvent, NodeOutput, SOPGraph, SisyphusKernel};

// ---------------------------------------------------------------------------
// SisyphusKernel — 认知循环委托
// ---------------------------------------------------------------------------

impl SisyphusKernel {
    /// 以递归认知反馈执行 SOP 图谱。
    /// 委托给 StrategicManager 执行。
    pub async fn execute_with_cognition(
        &self,
        sop_graph: Arc<SOPGraph>,
        sop_id: &str,
        project_id: Option<&str>,
        initial_ctx: Option<Arc<ExecutionContext>>,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
        stream_tx: Option<mpsc::UnboundedSender<sisyphus_types::CognitiveEvent>>,
    ) -> Result<
        (
            NodeOutput,
            crate::strategic_manager::CognitiveReport,
            Option<Arc<SOPGraph>>,
        ),
        CoreError,
    > {
        let goal_text = self
            .roadmap_manager
            .get_goal_description(project_id.unwrap_or("default"), sop_id)
            .await
            .ok()
            .flatten();

        self.strategic_manager
            .run_cognitive_loop(
                sop_graph,
                sop_id,
                project_id,
                goal_text.as_deref(),
                initial_ctx,
                event_tx,
                |sid, event| self.broadcast_event(sid, event),
                stream_tx,
                self.micro_reflector.clone(),
            )
            .await
    }
}
