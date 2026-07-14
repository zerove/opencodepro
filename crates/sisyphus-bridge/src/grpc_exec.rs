use std::sync::Arc;
use std::time::Instant;

use sisyphus_core::{SOPGraph, WasmSOPNode};
use sisyphus_metrics::record_sop_execution;
use tokio::sync::mpsc;
use tonic::Status;
use tracing::info;

use super::MySisyphusBridge;

impl MySisyphusBridge {
    pub(crate) async fn run_execution(
        &self,
        sop_id: String,
        event_tx: Option<mpsc::UnboundedSender<sisyphus_types::ExecutionEvent>>,
    ) -> Result<(String, String), Status> {
        let start = Instant::now();
        let result = if let Some(goal) = sop_id.strip_prefix("goal:") {
            let synthesized_sop_id = format!("synth_{}", goal);
            match self
                .kernel
                .execute_goal(goal, &synthesized_sop_id, event_tx, None)
                .await
            {
                Ok((result, report)) => {
                    let duration = start.elapsed();
                    record_sop_execution(&synthesized_sop_id, true, duration.as_secs_f64());
                    Ok((
                        format!("Result: {} | Cognition: {:?}", result, report),
                        synthesized_sop_id,
                    ))
                }
                Err(e) => {
                    let duration = start.elapsed();
                    record_sop_execution(&synthesized_sop_id, false, duration.as_secs_f64());
                    Err(Status::internal(e))
                }
            }
        } else {
            let graph = match self.kernel.load_sop(&sop_id).await {
                Ok(g) => g,
                Err(_) => {
                    let mut g = SOPGraph::new();
                    let tool_id = sop_id.clone();
                    let fuel = self.default_fuel;
                    let node = Arc::new(WasmSOPNode {
                        name: format!("node_{}", tool_id),
                        tool_id,
                        executor: self.kernel.execution_manager.tool_router.clone(),
                        fuel_limit: fuel,
                    });
                    g.add_node(node);
                    Arc::new(g)
                }
            };

            let start_node = graph
                .graph
                .node_indices()
                .next()
                .ok_or_else(|| Status::internal("Graph empty"))?;

            match self
                .kernel
                .execute_with_cognition(graph, &sop_id, None, event_tx, None)
                .await
            {
                Ok((result, report, evolved_graph)) => {
                    let duration = start.elapsed();
                    record_sop_execution(&sop_id, true, duration.as_secs_f64());
                    if evolved_graph.is_some() {
                        info!("SOP evolved for sop_id={}", sop_id);
                    }
                    Ok((
                        format!("Result: {} | Cognition: {:?}", result, report),
                        sop_id,
                    ))
                }
                Err(e) => {
                    let duration = start.elapsed();
                    record_sop_execution(&sop_id, false, duration.as_secs_f64());
                    Err(Status::internal(e))
                }
            }
        };
        result
    }
}
