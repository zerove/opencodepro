use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::algo::toposort;
use std::collections::HashMap;
use std::sync::Arc;
use serde_json::Value;
use tokio::task::JoinSet;

pub use sisyphus_types::{ExecutionContext, SOPGraph, SOPNode, ToolExecutor, NodeOutput, StateStore, WasmSOPNode};
use sisyphus_mem::MfsStore;
use sisyphus_intel::SisyphusIntel;



pub struct Executor;

impl Default for Executor {
    fn default() -> Self { Self::new() }
}

impl Executor {
    pub fn new() -> Self { Self }

    pub async fn execute(&self, sop_graph: &SOPGraph, _start_node: NodeIndex) -> Result<Arc<ExecutionContext>, String> {
        if toposort(&sop_graph.graph, None).is_err() {
            return Err("SOPGraph contains cycles".to_string());
        }

        let ctx = Arc::new(ExecutionContext::new());
        let mut join_set = JoinSet::new();
        let mut in_degree = HashMap::new();
        let nodes = sop_graph.graph.node_indices();
        for node in nodes {
            let degree = sop_graph.graph.edges_directed(node, petgraph::Direction::Incoming).count();
            in_degree.insert(node, degree);
        }

        for (&node, &degree) in in_degree.iter() {
            if degree == 0 {
                let node_ref = Arc::clone(sop_graph.graph.node_weight(node).unwrap());
                let ctx_ref = Arc::clone(&ctx);
                join_set.spawn(async move {
                    let name = node_ref.name().to_string();
                    let result = node_ref.execute(&ctx_ref).await;
                    (node, name, result)
                });
            }
        }

        while let Some(res) = join_set.join_next().await {
            let (node, name, result) = res.map_err(|e| e.to_string())?;
            let output = match result {
                Ok(out) => out,
                Err(e) => Value::String(e),
            };
            ctx.set_result(name, output);
            for edge in sop_graph.graph.edges(node) {
                let target = edge.target();
                if let Some(degree) = in_degree.get_mut(&target) {
                    *degree -= 1;
                    if *degree == 0 {
                        let node_ref = Arc::clone(sop_graph.graph.node_weight(target).unwrap());
                        let ctx_ref = Arc::clone(&ctx);
                        join_set.spawn(async move {
                            let name = node_ref.name().to_string();
                            let result = node_ref.execute(&ctx_ref).await;
                            (target, name, result)
                        });
                    }
                }
            }
        }
        Ok(ctx)
    }
}

// ---------------------------------------------------------------------------
// Cognitive execution — iterative predict → execute → reflect → evolve
// ---------------------------------------------------------------------------

const MAX_COGNITIVE_ITERATIONS: usize = 3;
const CONVERGENCE_THRESHOLD: f64 = 0.3;

/// A single iteration in a cognitive execution cycle.
pub struct CognitiveIteration {
    pub iteration: usize,
    pub prediction: sisyphus_intel::SimulationResult,
    pub deviation_score: f64,
    pub suggestions: Vec<String>,
    pub evolved: bool,
}

/// Full report from a recursive cognitive execution.
pub struct CognitiveReport {
    pub iterations: usize,
    pub converged: bool,
    pub final_deviation: f64,
    pub history: Vec<CognitiveIteration>,
}

pub struct SisyphusKernel {
    pub executor: Executor,
    pub state_store: Arc<MfsStore>,
    pub tool_executor: Arc<dyn ToolExecutor>,
    pub intel: Arc<SisyphusIntel>,
    pub synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer>,
}

impl SisyphusKernel {
    pub fn new(
        state_store: Arc<MfsStore>,
        tool_executor: Arc<dyn ToolExecutor>,
        intel: Arc<SisyphusIntel>,
        synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer>,
    ) -> Self {
        Self {
            executor: Executor::new(),
            state_store,
            tool_executor,
            intel,
            synthesizer,
        }
    }

    pub async fn save_sop(&self, sop_id: &str, graph: &SOPGraph) -> Result<(), String> {
        let config = sisyphus_types::SOPGraphConfig::from_graph(graph);
        let data = serde_json::to_string(&config).map_err(|e| e.to_string())?;
        self.state_store.set_state_json(&format!("sop:{}", sop_id), serde_json::Value::String(data)).await
            .map_err(|e| format!("MFS save error: {}", e))
    }

    pub async fn load_sop(&self, sop_id: &str) -> Result<SOPGraph, String> {
        let data_val = self.state_store.get_state_json(&format!("sop:{}", sop_id)).await
            .map_err(|e| format!("MFS load error: {}", e))?;
        
        let data = data_val.and_then(|v| v.as_str().map(|s| s.to_string()))
            .ok_or_else(|| format!("SOP {} not found", sop_id))?;
        let config: sisyphus_types::SOPGraphConfig = serde_json::from_str(&data).map_err(|e| e.to_string())?;
        Ok(SOPGraph::from_config(&config, self.tool_executor.clone()))
    }

    /// Execute a natural-language goal through the full cognitive pipeline.
    ///
    /// 1. Synthesize a SOP graph from the goal string
    /// 2. Execute with recursive cognition (auto-retry on high deviation)
    /// 3. Return the result plus a detailed cognitive report
    pub async fn execute_goal(
        &self,
        goal: &str,
        sop_id: &str,
    ) -> Result<(NodeOutput, String), String> {
        let ctx = ExecutionContext::new();
        
        let synthesis = self.synthesizer.synthesize(goal, &ctx).await?;
        let graph = synthesis.graph;
        let start_node = synthesis.start_node;
        let plan_desc = synthesis.plan_description;
        
        let (res, report, _) = self.execute_with_cognition(&graph, start_node, sop_id).await?;
        Ok((res, format!("Plan: {} | {}", plan_desc, report)))
    }

    /// Execute a SOP graph with recursive cognitive feedback.
    ///
    /// Loop: predict → execute → reflect → evolve
    ///   - If deviation < CONVERGENCE_THRESHOLD (0.3): stop, converged.
    ///   - If deviation >= threshold AND evolved graph available: re-execute.
    ///   - Max MAX_COGNITIVE_ITERATIONS (3) iterations.
    /// Returns (result, report_string, opt_evolved_graph).
    #[allow(unused_assignments)]
    pub async fn execute_with_cognition(
        &self,
        sop_graph: &SOPGraph,
        start_node: NodeIndex,
        sop_id: &str,
    ) -> Result<(NodeOutput, String, Option<SOPGraph>), String> {
        let mut own_graph: Option<SOPGraph> = None;
        let mut current_ref: &SOPGraph = sop_graph;
        let mut current_start = start_node;
        let mut iterations: Vec<CognitiveIteration> = Vec::new();
        let mut final_result: NodeOutput = serde_json::Value::Null;
        let mut final_evolved: Option<SOPGraph> = None;

        for iteration in 0..MAX_COGNITIVE_ITERATIONS {
            let ctx = ExecutionContext::new();
            
            // 1. Predict
            let prediction = self.intel.simulator.simulate(current_ref, &ctx, sop_id).await?;
            
            // 2. Execute
            let final_ctx = self.executor.execute(current_ref, current_start).await?;
            let result = final_ctx.get_result(
                current_ref.graph.node_weight(current_start).unwrap().name()
            ).ok_or("Execution failed to produce result")?;
            final_result = result.clone();
            
            // 3. Build per-node trace records
            let mut trace_lines: Vec<String> = Vec::new();
            for node in current_ref.graph.node_indices() {
                let name = current_ref.graph.node_weight(node).unwrap().name().to_string();
                let output = final_ctx
                    .get_result(&name)
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "no_result".into());
                let status = if output.contains("ResourceExhausted") {
                    "error"
                } else {
                    "ok"
                };
                let record = serde_json::json!({
                    "node": name,
                    "status": status,
                    "output": output,
                });
                trace_lines.push(record.to_string());
            }
            let trace = trace_lines.join("\n");
            
            // 4. Reflect
            let reflection = self.intel.reflector.reflect(&trace, &prediction).await?;
            
            // Persist history
            let history_key = format!("history:{}", sop_id);
            let mut history: sisyphus_types::ExecutionHistory = self.state_store.get_state_json(&history_key).await
                .map_err(|e| format!("MFS history load error: {}", e))?
                .and_then(|val| serde_json::from_value(val).ok())
                .unwrap_or_else(|| sisyphus_types::ExecutionHistory {
                    sop_id: sop_id.to_string(),
                    records: Vec::new(),
                });

            history.records.push(sisyphus_types::HistoryRecord {
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                trace: trace.clone(),
                deviation_score: reflection.deviation_score,
                success: !trace.contains("ResourceExhausted"),
                cost: 10 * (trace.lines().count() as u64),
            });
            let _ = self.state_store.set_state_json(&history_key, serde_json::to_value(history).unwrap()).await;
            
            // 5. Evolve
            let evolved = self.intel.evolver.evolve(current_ref, &reflection, &prediction).ok();
            let has_evolved = evolved.is_some();
            
            if let Some(ref eg) = evolved {
                let _ = self.save_sop(sop_id, eg).await;
            }
            
            iterations.push(CognitiveIteration {
                iteration,
                prediction,
                deviation_score: reflection.deviation_score,
                suggestions: reflection.suggested_improvements,
                evolved: has_evolved,
            });
            
            // Check convergence
            if reflection.deviation_score < CONVERGENCE_THRESHOLD || !has_evolved {
                final_evolved = evolved;
                break;
            }
            
            // Re-execute with evolved graph
            if let Some(eg) = evolved {
                current_start = eg.graph.node_indices().next().unwrap_or(start_node);
                own_graph = Some(eg);
                current_ref = own_graph.as_ref().unwrap();
            } else {
                break;
            }
        }

        let converged = iterations.last()
            .map(|i| i.deviation_score < CONVERGENCE_THRESHOLD)
            .unwrap_or(false);

        let iteration_details: Vec<String> = iterations.iter().map(|i| {
            format!(
                "iter[{}]: deviation={:.2}, evolved={}, suggestions={:?}",
                i.iteration, i.deviation_score, i.evolved, i.suggestions
            )
        }).collect();

        let report = format!(
            "CognitiveLoop: {} iterations, converged={} | {}",
            iterations.len(),
            converged,
            iteration_details.join(" -> ")
        );
        
        Ok((final_result, report, final_evolved))
    }

}
