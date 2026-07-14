use std::sync::Arc;
use async_trait::async_trait;
use sisyphus_types::{RuntimeSupervisor, SupervisorDecision, ExecutionContext, NodeOutput};
use sisyphus_types::SOPGraph;
use crate::reflect::Reflector;
use crate::simulation::SimulationResult;

/// A concrete implementation of `RuntimeSupervisor` that uses the `Reflector`
/// to detect runtime deviations from a predicted execution path.
pub struct SisyphusSupervisor {
    reflector: Arc<dyn Reflector>,
    prediction: SimulationResult,
    graph: Arc<SOPGraph>,
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
            graph,
            threshold,
            experience_bank,
            goal_text,
        }
    }

    /// Helper to construct a trace from the current execution context.
    fn build_trace(&self, ctx: &ExecutionContext) -> String {
        let trace_points = ctx.trace.read().map(|t| t.clone()).unwrap_or_default();
        let mut lines = Vec::new();
        
        for point in trace_points {
            let status = if point.output_summary.contains("Error") || point.output_summary.contains("ResourceExhausted") {
                "error"
            } else {
                "ok"
            };
            
            lines.push(format!(r#"{{"node": "{}", "status": "{}", "output": "{}"}}"#, 
                point.node_name, status, point.output_summary));
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

        // 1. Fast Heuristic Reflection (Layer 1)
        let output_str = output.0.as_str().unwrap_or("");
        let is_obvious_error = output_str.contains("Error") 
            || output_str.contains("Failed") 
            || output_str.contains("Exception")
            || output_str.contains("ResourceExhausted");

        if !is_obvious_error && output_str.len() > 0 && self.prediction.success_probability >= 0.5 {
            tracing::info!(node = %node_name, "Fast-track: node output appears healthy, skipping LLM reflection");
            return Ok(SupervisorDecision::Continue { deviation: 0.05 });
        }

        // 2. Build a current trace of the execution
        let trace = self.build_trace(&ctx);
        
        // Retrieve semantic experiences for the current goal and node
        let exp_query = sisyphus_types::experience::ExperienceQuery {
            query_text: self.goal_text.clone(),
            context_hash: format!("{:x}", md5::compute(&self.goal_text)),
            project_id: None,
            action_id: Some(node_name.to_string()),
            visual_embedding: None,
            limit: 5,
        };
        let experiences = self.experience_bank.query_similar(exp_query).await.ok();

        // 2. Reflect on the current state vs the prediction
        let reflection = self.reflector.reflect(&trace, &self.prediction, experiences.as_deref()).await
            .map_err(|e| format!("Reflection error during runtime supervision: {}", e))?;

        tracing::info!(
            deviation = reflection.deviation_score,
            threshold = self.threshold,
            "Runtime deviation check"
        );

        // 3. Decide whether to pivot
        // Adjust threshold based on simulation success probability:
        // Lower success probability (higher risk) makes the supervisor more sensitive to deviations.
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

        Ok(SupervisorDecision::Continue { deviation: reflection.deviation_score })
    }
}
