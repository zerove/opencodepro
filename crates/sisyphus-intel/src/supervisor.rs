use crate::reflect::Reflector;
use crate::simulation::SimulationResult;
use async_trait::async_trait;
use sisyphus_types::SOPGraph;
use sisyphus_types::{ExecutionContext, NodeOutput, RuntimeSupervisor, SupervisorDecision};
use std::sync::Arc;

/// `RuntimeSupervisor` 的具体实现，使用 `Reflector`
/// 检测与预测执行路径的运行时偏差。
pub struct SisyphusSupervisor {
    reflector: Arc<dyn Reflector>,
    prediction: SimulationResult,
    _graph: Arc<SOPGraph>,
    threshold: f64,
    experience_bank: Arc<dyn sisyphus_types::experience::ExperienceBank>,
    goal_text: String,
}

impl SisyphusSupervisor {
    pub fn new(
        reflector: Arc<dyn Reflector>,
        prediction: SimulationResult,
        graph: Arc<SOPGraph>,
        threshold: f64,
        experience_bank: Arc<dyn sisyphus_types::experience::ExperienceBank>,
        goal_text: String,
    ) -> Self {
        Self {
            reflector,
            prediction,
            _graph: graph,
            threshold,
            experience_bank,
            goal_text,
        }
    }

    /// 从当前执行上下文构造轨迹的辅助函数。
    fn build_trace(&self, ctx: &ExecutionContext) -> String {
        let trace_points = ctx.trace.read().map(|t| t.clone()).unwrap_or_default();
        let mut lines = Vec::new();

        for point in trace_points {
            let status = if point.output_summary.contains("Error")
                || point.output_summary.contains("ResourceExhausted")
            {
                "error"
            } else {
                "ok"
            };

            lines.push(format!(
                r#"{{"node": "{}", "status": "{}", "output": "{}"}}"#,
                point.node_name, status, point.output_summary
            ));
        }
        lines.join("\n")
    }
}

#[async_trait]
impl RuntimeSupervisor for SisyphusSupervisor {
    async fn evaluate(
        &self,
        node_name: &str,
        output: &NodeOutput,
        ctx: Arc<ExecutionContext>,
    ) -> Result<SupervisorDecision, String> {
        tracing::debug!(node = %node_name, "Runtime supervisor evaluating node output");

        // 1. 快速启发式反思（第 1 层）
        let output_str = output.0.as_str().unwrap_or("");
        let is_obvious_error = output_str.contains("Error")
            || output_str.contains("Failed")
            || output_str.contains("Exception")
            || output_str.contains("ResourceExhausted");

        if !is_obvious_error && !output_str.is_empty() && self.prediction.success_probability >= 0.5
        {
            tracing::info!(node = %node_name, "Fast-track: node output appears healthy, skipping LLM reflection");
            return Ok(SupervisorDecision::Continue { deviation: 0.05 });
        }

        // 2. 构建执行过程中的当前轨迹
        let trace = self.build_trace(&ctx);

        // 检索当前目标与节点的语义经验
        let exp_query = sisyphus_types::experience::ExperienceQuery {
            query_text: self.goal_text.clone(),
            context_hash: format!("{:x}", md5::compute(&self.goal_text)),
            project_id: None,
            action_id: Some(node_name.to_string()),
            visual_embedding: None,
            limit: 5,
        };
        let experiences = self.experience_bank.query_similar(exp_query).await.ok();

        // 2. 对比当前状态与预测进行反思
        let reflection = self
            .reflector
            .reflect(&trace, &self.prediction, experiences.as_deref())
            .await
            .map_err(|e| format!("Reflection error during runtime supervision: {}", e))?;

        tracing::info!(
            deviation = reflection.deviation_score,
            threshold = self.threshold,
            "Runtime deviation check"
        );

        // 3. 决定是否转向
        // 基于仿真成功概率调整阈值：
        // 较低的成功概率（较高风险）使监督者对偏差更敏感。
        let effective_threshold = self.threshold * self.prediction.success_probability.max(0.1);

        if reflection.deviation_score > effective_threshold {
            tracing::warn!(
                deviation = reflection.deviation_score,
                threshold = effective_threshold,
                node = %node_name,
                "Runtime deviation threshold exceeded! Triggering Strategic Pivot."
            );

            return Ok(SupervisorDecision::Pivot {
                reason: format!(
                    "Node {} produced output with deviation score {:.2} (effective threshold {:.2}, base {:.2})",
                    node_name, reflection.deviation_score, effective_threshold, self.threshold
                ),
                suggested_correction: None, // Local re-synthesis will handle the correction
            });
        }

        Ok(SupervisorDecision::Continue {
            deviation: reflection.deviation_score,
        })
    }
}
