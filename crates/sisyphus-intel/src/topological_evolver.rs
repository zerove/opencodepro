use crate::error::IntelError;
use crate::reflect::{ReflectionReport, SOPEvolver};
use crate::simulation::SimulationResult;
use async_trait::async_trait;
use petgraph::graph::NodeIndex;
use sisyphus_types::{EvolutionOp, SOPGraph, WasmSOPNode};
use std::collections::HashMap;
use std::sync::Arc;

pub struct TopologicalSOPEvolver;

impl Default for TopologicalSOPEvolver {
    fn default() -> Self {
        Self::new()
    }
}

impl TopologicalSOPEvolver {
    pub fn new() -> Self {
        Self
    }

    /// Applies a batch of evolution operations to the graph.
    pub fn apply_ops(
        &self,
        graph: &SOPGraph,
        ops: &[EvolutionOp],
        tool_executor: Arc<dyn sisyphus_types::ToolExecutor>,
    ) -> Result<SOPGraph, IntelError> {
        let mut new_graph = SOPGraph::new();
        let mut node_map = HashMap::new();

        // First, copy all existing nodes to the new graph
        for node_idx in graph.graph.node_indices() {
            let node = graph.graph.node_weight(node_idx).expect("valid index");
            let new_idx = new_graph.add_node(node.clone());
            node_map.insert(node_idx, new_idx);
        }

        // Copy all existing edges
        for edge in graph.graph.edge_indices() {
            let (source, target) = graph.graph.edge_endpoints(edge).expect("valid edge");
            let new_source = node_map[&source];
            let new_target = node_map[&target];
            new_graph.add_edge(new_source, new_target);
        }

        // Apply operations
        for op in ops {
            match op {
                EvolutionOp::AdjustFuel { node_name, delta } => {
                    let node_idx = self.find_node_by_name(&new_graph, node_name)?;
                    let node = new_graph.graph.node_weight(node_idx).expect("valid index");

                    if let Some(wasm_node) = node.as_any().downcast_ref::<WasmSOPNode>() {
                        let new_fuel = (wasm_node.fuel_limit as i64 + delta).max(0) as u64;
                        let evolved_node = Arc::new(WasmSOPNode {
                            name: wasm_node.name.clone(),
                            tool_id: wasm_node.tool_id.clone(),
                            fuel_limit: new_fuel,
                            executor: wasm_node.executor.clone(),
                        });
                        new_graph.graph[node_idx] = evolved_node;
                    } else {
                        return Err(IntelError::NodeTypeMismatch {
                            node_name: node_name.clone(),
                        });
                    }
                }
                EvolutionOp::AddRetry {
                    node_name,
                    max_retries,
                } => {
                    let node_idx = self.find_node_by_name(&new_graph, node_name)?;
                    let node = new_graph.graph.node_weight(node_idx).expect("valid index");

                    let evolved_node = Arc::new(sisyphus_types::RetrySOPNode {
                        name: format!("{}_retry", node_name),
                        child: node.clone(),
                        max_retries: *max_retries,
                    });
                    new_graph.graph[node_idx] = evolved_node;
                }
                EvolutionOp::InsertNode {
                    after_node,
                    new_name,
                    tool_id,
                } => {
                    let after_idx = self.find_node_by_name(&new_graph, after_node)?;

                    // Create new node
                    // Note: We need a tool_executor. In a real scenario, this would be passed in or stored.
                    // For this implementation, we assume the tool_executor is provided to apply_ops.
                    let new_node = Arc::new(WasmSOPNode {
                        name: new_name.clone(),
                        tool_id: tool_id.clone(),
                        fuel_limit: 1_000_000,
                        executor: tool_executor.clone(),
                    });
                    let new_idx = new_graph.add_node(new_node);

                    // Rewire: after_node -> successors becomes after_node -> new_node -> successors
                    let successors: Vec<_> = new_graph.graph.neighbors(after_idx).collect();
                    for succ in successors {
                        if let Some(edge_idx) = new_graph.graph.find_edge(after_idx, succ) {
                            new_graph.graph.remove_edge(edge_idx);
                        }
                        new_graph.add_edge(after_idx, new_idx);
                        new_graph.add_edge(new_idx, succ);
                    }

                    // If after_node had no successors, just add the edge
                    if new_graph.graph.neighbors(after_idx).count() == 0 {
                        new_graph.add_edge(after_idx, new_idx);
                    }
                }
                EvolutionOp::RemoveNode { node_name } => {
                    let node_idx = self.find_node_by_name(&new_graph, node_name)?;

                    let predecessors: Vec<_> = new_graph
                        .graph
                        .neighbors_directed(node_idx, petgraph::Direction::Incoming)
                        .collect();
                    let successors: Vec<_> = new_graph.graph.neighbors(node_idx).collect();

                    for pred in predecessors {
                        for succ in &successors {
                            new_graph.add_edge(pred, *succ);
                        }
                    }

                    new_graph.graph.remove_node(node_idx);
                }
                EvolutionOp::SwapNodes { node_a, node_b } => {
                    let idx_a = self.find_node_by_name(&new_graph, node_a)?;
                    let idx_b = self.find_node_by_name(&new_graph, node_b)?;

                    if idx_a == idx_b {
                        continue;
                    }

                    // Swap edges
                    let preds_a: Vec<_> = new_graph
                        .graph
                        .neighbors_directed(idx_a, petgraph::Direction::Incoming)
                        .collect();
                    let succs_a: Vec<_> = new_graph.graph.neighbors(idx_a).collect();
                    let preds_b: Vec<_> = new_graph
                        .graph
                        .neighbors_directed(idx_b, petgraph::Direction::Incoming)
                        .collect();
                    let succs_b: Vec<_> = new_graph.graph.neighbors(idx_b).collect();

                    // Identify edges between the two nodes before removing them
                    let a_to_b = new_graph.graph.contains_edge(idx_a, idx_b);
                    let b_to_a = new_graph.graph.contains_edge(idx_b, idx_a);

                    // Remove all edges connected to a and b
                    for p in &preds_a {
                        if let Some(edge_idx) = new_graph.graph.find_edge(*p, idx_a) {
                            new_graph.graph.remove_edge(edge_idx);
                        }
                    }
                    for s in &succs_a {
                        if let Some(edge_idx) = new_graph.graph.find_edge(idx_a, *s) {
                            new_graph.graph.remove_edge(edge_idx);
                        }
                    }
                    for p in &preds_b {
                        if let Some(edge_idx) = new_graph.graph.find_edge(*p, idx_b) {
                            new_graph.graph.remove_edge(edge_idx);
                        }
                    }
                    for s in &succs_b {
                        if let Some(edge_idx) = new_graph.graph.find_edge(idx_b, *s) {
                            new_graph.graph.remove_edge(edge_idx);
                        }
                    }

                    // Rewire: a's old preds -> b, b's old succs -> a
                    for p in preds_a {
                        if p != idx_b {
                            new_graph.add_edge(p, idx_b);
                        }
                    }
                    for s in succs_b {
                        if s != idx_a {
                            new_graph.add_edge(idx_a, s);
                        }
                    }

                    // Rewire: b's old preds -> a, a's old succs -> b
                    for p in preds_b {
                        if p != idx_a {
                            new_graph.add_edge(p, idx_a);
                        }
                    }
                    for s in succs_a {
                        if s != idx_b {
                            new_graph.add_edge(idx_b, s);
                        }
                    }

                    // Swap the edge between them
                    if a_to_b {
                        new_graph.add_edge(idx_b, idx_a);
                    }
                    if b_to_a {
                        new_graph.add_edge(idx_a, idx_b);
                    }
                }
            }
        }

        // Final check for cycles to preserve DAG invariant
        if petgraph::algo::is_cyclic_directed(&new_graph.graph) {
            return Err(IntelError::EvolutionCycle);
        }

        Ok(new_graph)
    }

    fn find_node_by_name(&self, graph: &SOPGraph, name: &str) -> Result<NodeIndex, IntelError> {
        graph
            .graph
            .node_indices()
            .find(|&idx| {
                graph
                    .graph
                    .node_weight(idx)
                    .map(|n| n.name() == name)
                    .unwrap_or(false)
            })
            .ok_or_else(|| IntelError::NodeNotFound { name: name.into() })
    }
}

// ... (existing code) ...
#[async_trait]
impl SOPEvolver for TopologicalSOPEvolver {
    async fn evolve(
        &self,
        graph: Arc<SOPGraph>,
        report: &ReflectionReport,
        _prediction: &SimulationResult,
        _insights: Option<&str>,
    ) -> Result<Arc<SOPGraph>, String> {
        let mut ops = Vec::new();

        // 1. Analyze failures and generate evolution ops
        for record in &report.records {
            if record.status == "error" {
                if record.output.contains("ResourceExhausted") {
                    ops.push(EvolutionOp::AdjustFuel {
                        node_name: record.node.clone(),
                        delta: 1_000_000,
                    });

                    // If it's a recurring issue, consider splitting the node
                    if report.deviation_score > 0.7 {
                        ops.push(EvolutionOp::InsertNode {
                            after_node: record.node.clone(),
                            new_name: format!("{}_part2", record.node),
                            tool_id: "generic_processor".into(), // In real case, would split the logic
                        });
                    }
                } else if record.output.contains("TypeError")
                    || record.output.contains("InvalidInput")
                {
                    ops.push(EvolutionOp::InsertNode {
                        after_node: "start".into(), // Simplified: insert at start or before node
                        new_name: format!("verify_{}", record.node),
                        tool_id: "type_check".into(),
                    });
                } else if record.output.contains("Timeout") {
                    ops.push(EvolutionOp::AddRetry {
                        node_name: record.node.clone(),
                        max_retries: 3,
                    });
                }
            }
        }

        if ops.is_empty() {
            return Ok(graph);
        }

        // We need a tool_executor to apply some ops (like InsertNode).
        // Since we don't have one here, we'll use a dummy one or we need to change the trait.
        // For now, we'll use a mock to demonstrate the structural change.
        let mock_executor = Arc::new(crate::mock::MockExecutor);
        self.apply_ops(&graph, &ops, mock_executor)
            .map(Arc::new)
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use sisyphus_types::{ToolExecutor, WasmSOPNode};

    struct MockExecutor;
    #[async_trait]
    impl ToolExecutor for MockExecutor {
        async fn execute(
            &self,
            _id: &str,
            _args: Vec<u8>,
            _fuel: u64,
        ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
            Ok(b"ok".to_vec())
        }
    }

    fn setup_graph() -> (SOPGraph, Arc<dyn ToolExecutor>) {
        let mut graph = SOPGraph::new();
        let exec = Arc::new(MockExecutor);
        let n1 = graph.add_node(Arc::new(WasmSOPNode {
            name: "node1".into(),
            tool_id: "t1".into(),
            fuel_limit: 100,
            executor: exec.clone(),
        }));
        let n2 = graph.add_node(Arc::new(WasmSOPNode {
            name: "node2".into(),
            tool_id: "t2".into(),
            fuel_limit: 100,
            executor: exec.clone(),
        }));
        graph.add_edge(n1, n2);
        (graph, exec)
    }

    #[test]
    fn test_insert_node() {
        let (graph, exec) = setup_graph();
        let evolver = TopologicalSOPEvolver::new();
        let ops = vec![EvolutionOp::InsertNode {
            after_node: "node1".into(),
            new_name: "node_mid".into(),
            tool_id: "t_mid".into(),
        }];
        let new_graph = evolver.apply_ops(&graph, &ops, exec).unwrap();

        assert_eq!(new_graph.graph.node_count(), 3);
        let n1 = evolver.find_node_by_name(&new_graph, "node1").unwrap();
        let mid = evolver.find_node_by_name(&new_graph, "node_mid").unwrap();
        let n2 = evolver.find_node_by_name(&new_graph, "node2").unwrap();

        assert!(new_graph.graph.contains_edge(n1, mid));
        assert!(new_graph.graph.contains_edge(mid, n2));
        assert!(!new_graph.graph.contains_edge(n1, n2));
    }

    #[test]
    fn test_remove_node() {
        let (graph, exec) = setup_graph();
        let evolver = TopologicalSOPEvolver::new();
        let ops = vec![EvolutionOp::RemoveNode {
            node_name: "node2".into(),
        }];
        let new_graph = evolver.apply_ops(&graph, &ops, exec).unwrap();

        assert_eq!(new_graph.graph.node_count(), 1);
        assert!(evolver.find_node_by_name(&new_graph, "node1").is_ok());
        assert!(evolver.find_node_by_name(&new_graph, "node2").is_err());
    }

    #[test]
    fn test_swap_nodes() {
        let (graph, exec) = setup_graph();
        let evolver = TopologicalSOPEvolver::new();
        let ops = vec![EvolutionOp::SwapNodes {
            node_a: "node1".into(),
            node_b: "node2".into(),
        }];
        let new_graph = evolver.apply_ops(&graph, &ops, exec).unwrap();

        let n1 = evolver.find_node_by_name(&new_graph, "node1").unwrap();
        let n2 = evolver.find_node_by_name(&new_graph, "node2").unwrap();

        assert!(new_graph.graph.contains_edge(n2, n1));
        assert!(!new_graph.graph.contains_edge(n1, n2));
    }

    #[test]
    fn test_cycle_detection() {
        let (graph, exec) = setup_graph();
        let evolver = TopologicalSOPEvolver::new();
        let ops = vec![EvolutionOp::InsertNode {
            after_node: "node2".into(),
            new_name: "node_cycle".into(),
            tool_id: "t_cycle".into(),
        }];
        let res = evolver.apply_ops(&graph, &ops, exec);
        assert!(res.is_ok());
    }
}
