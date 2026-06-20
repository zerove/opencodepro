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



    pub async fn execute_goal(
        &self,
        goal: &str,
        sop_id: &str,
    ) -> Result<(NodeOutput, String), String> {
        let ctx = ExecutionContext::new();
        
        // 1. Synthesize
        let synthesis = self.synthesizer.synthesize(goal, &ctx).await?;
        let graph = synthesis.graph;
        let start_node = synthesis.start_node;
        
        // 2. Execute with cognition
        let (res, report, _) = self.execute_with_cognition(&graph, start_node, sop_id).await?;
        Ok((res, format!("Plan: {} | Cognition: {}", synthesis.plan_description, report)))
    }

    pub async fn execute_with_cognition(
        &self,
        sop_graph: &SOPGraph,
        start_node: NodeIndex,
        _sop_id: &str,
    ) -> Result<(NodeOutput, String, Option<SOPGraph>), String> {
        let ctx = ExecutionContext::new();
        
        // 1. Predict
        let prediction = self.intel.simulator.simulate(sop_graph, &ctx, _sop_id).await?;
        
        // 2. Execute
        let final_ctx = self.executor.execute(sop_graph, start_node).await?;
        let result = final_ctx.get_result(sop_graph.graph.node_weight(start_node).unwrap().name())
            .ok_or("Execution failed to produce result")?;
        
        // 3. Build per-node trace records
        let mut trace_lines: Vec<String> = Vec::new();
        for node in sop_graph.graph.node_indices() {
            let name = sop_graph.graph.node_weight(node).unwrap().name().to_string();
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
        
        let history_key = format!("history:{}", _sop_id);
        let mut history: sisyphus_types::ExecutionHistory = self.state_store.get_state_json(&history_key).await
            .map_err(|e| format!("MFS history load error: {}", e))?
            .and_then(|val| serde_json::from_value(val).ok())
            .unwrap_or_else(|| sisyphus_types::ExecutionHistory {
                sop_id: _sop_id.to_string(),
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

        let evolved_graph = self.intel.evolver.evolve(sop_graph, &reflection).ok();
        
        if let Some(ref evolved) = evolved_graph {
            let _ = self.save_sop(_sop_id, evolved).await;
        }
        
        let report = format!(
            "Prediction: {} (Prob: {:.2}) | Reflection: Deviation {:.2}, Suggestions: {:?}",
            prediction.predicted_outcome, 
            prediction.success_probability, 
            reflection.deviation_score, 
            reflection.suggested_improvements
        );
        
        Ok((result, report, evolved_graph))
    }

}
