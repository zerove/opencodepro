use async_trait::async_trait;
use sisyphus_intel::{
    reflect::{RecordedNode, ReflectionReport},
    LlmEvolver, SOPEvolver, TopologicalSOPEvolver,
};
use sisyphus_tools_llm::metis::LlmClient;
use sisyphus_types::{EvolutionOp, SOPGraph, ToolExecutor, TypesError, WasmSOPNode};
use std::sync::Arc;

struct MockExecutor;

#[async_trait]
impl ToolExecutor for MockExecutor {
    async fn execute(&self, _id: &str, _args: Vec<u8>, _fuel: u64) -> Result<Vec<u8>, TypesError> {
        Ok(b"ok".to_vec())
    }
}

struct MockLlmClient {
    response: String,
}

#[async_trait]
impl LlmClient for MockLlmClient {
    async fn generate(&self, _prompt: &str) -> Result<String, String> {
        Ok(self.response.clone())
    }

    async fn chat(&self, _system: &str, _user: &str, _stream: bool) -> Result<String, String> {
        Ok(self.response.clone())
    }
}

fn setup_basic_graph() -> (SOPGraph, Arc<dyn ToolExecutor>) {
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
fn test_topological_adjust_fuel() {
    let (graph, exec) = setup_basic_graph();
    let evolver = TopologicalSOPEvolver::new();
    let ops = vec![EvolutionOp::AdjustFuel {
        node_name: "node1".into(),
        delta: 50,
    }];
    let new_graph = evolver
        .apply_ops(&graph, &ops, exec)
        .expect("Should adjust fuel");

    let n1_idx = new_graph
        .graph
        .node_indices()
        .find(|&i| new_graph.graph.node_weight(i).unwrap().name() == "node1")
        .unwrap();
    let n1 = new_graph.graph.node_weight(n1_idx).unwrap();
    let wasm_n1 = n1
        .as_any()
        .downcast_ref::<WasmSOPNode>()
        .expect("Should be WasmSOPNode");
    assert_eq!(wasm_n1.fuel_limit, 150);
}

#[test]
fn test_topological_add_retry() {
    let (graph, exec) = setup_basic_graph();
    let evolver = TopologicalSOPEvolver::new();
    let ops = vec![EvolutionOp::AddRetry {
        node_name: "node1".into(),
        max_retries: 5,
    }];
    let new_graph = evolver
        .apply_ops(&graph, &ops, exec)
        .expect("Should add retry");

    let n1_idx = new_graph
        .graph
        .node_indices()
        .find(|&i| new_graph.graph.node_weight(i).unwrap().name() == "node1_retry")
        .unwrap();
    let n1 = new_graph.graph.node_weight(n1_idx).unwrap();
    assert!(n1.name().contains("retry"));
    // We can't easily downcast to RetrySOPNode without it being public or having a trait method,
    // but we can check the name. In a real scenario, we'd check the type.
}

#[test]
fn test_topological_insert_node() {
    let (graph, exec) = setup_basic_graph();
    let evolver = TopologicalSOPEvolver::new();
    let ops = vec![EvolutionOp::InsertNode {
        after_node: "node1".into(),
        new_name: "node_mid".into(),
        tool_id: "t_mid".into(),
    }];
    let new_graph = evolver
        .apply_ops(&graph, &ops, exec)
        .expect("Should insert node");

    assert_eq!(new_graph.graph.node_count(), 3);
    let n1_idx = new_graph
        .graph
        .node_indices()
        .find(|&i| new_graph.graph.node_weight(i).unwrap().name() == "node1")
        .unwrap();
    let mid_idx = new_graph
        .graph
        .node_indices()
        .find(|&i| new_graph.graph.node_weight(i).unwrap().name() == "node_mid")
        .unwrap();
    let n2_idx = new_graph
        .graph
        .node_indices()
        .find(|&i| new_graph.graph.node_weight(i).unwrap().name() == "node2")
        .unwrap();

    assert!(new_graph.graph.contains_edge(n1_idx, mid_idx));
    assert!(new_graph.graph.contains_edge(mid_idx, n2_idx));
    assert!(!new_graph.graph.contains_edge(n1_idx, n2_idx));
}

#[test]
fn test_topological_remove_node() {
    let (graph, exec) = setup_basic_graph();
    let evolver = TopologicalSOPEvolver::new();
    let ops = vec![EvolutionOp::RemoveNode {
        node_name: "node1".into(),
    }];
    let new_graph = evolver
        .apply_ops(&graph, &ops, exec)
        .expect("Should remove node");

    assert_eq!(new_graph.graph.node_count(), 1);
    assert!(new_graph
        .graph
        .node_indices()
        .any(|i| new_graph.graph.node_weight(i).unwrap().name() == "node2"));
}

#[test]
fn test_topological_swap_nodes() {
    let (graph, exec) = setup_basic_graph();
    let evolver = TopologicalSOPEvolver::new();
    let ops = vec![EvolutionOp::SwapNodes {
        node_a: "node1".into(),
        node_b: "node2".into(),
    }];
    let new_graph = evolver
        .apply_ops(&graph, &ops, exec)
        .expect("Should swap nodes");

    let n1_idx = new_graph
        .graph
        .node_indices()
        .find(|&i| new_graph.graph.node_weight(i).unwrap().name() == "node1")
        .unwrap();
    let n2_idx = new_graph
        .graph
        .node_indices()
        .find(|&i| new_graph.graph.node_weight(i).unwrap().name() == "node2")
        .unwrap();

    assert!(new_graph.graph.contains_edge(n2_idx, n1_idx));
    assert!(!new_graph.graph.contains_edge(n1_idx, n2_idx));
}

#[test]
fn test_topological_cycle_detection() {
    let (graph, exec) = setup_basic_graph();
    let evolver = TopologicalSOPEvolver::new();
    // Create a cycle: node2 -> node1 (already have node1 -> node2)
    let ops = vec![EvolutionOp::SwapNodes {
        node_a: "node1".into(),
        node_b: "node2".into(),
    }];
    // Wait, SwapNodes swaps edges. If we have 1->2, swap(1,2) makes it 2->1. No cycle.

    // To create a cycle, we need to add an edge from a successor back to a predecessor.
    // Let's use a manual operation or a sequence that creates a cycle.
    // Since the only way to add edges is InsertNode or SwapNodes, let's try a complex swap or multiple insertions.

    // Actually, let's just try to force a cycle if possible.
    // If I have 1->2 and I insert node_mid after 2, and then somehow make node_mid go to 1.
    // But InsertNode always goes after_node -> new_node -> successors.

    // Let's try to find a way to create a cycle with the current ops.
    // SwapNodes: a's preds -> b, b's succs -> a, b's preds -> a, a's succs -> b.

    // If we have 1->2 and 2->3. Swap(1, 3).
    // Preds 1: {}, Succs 1: {2}
    // Preds 3: {2}, Succs 3: {}
    // 1's preds -> 3: {}
    // 3's succs -> 1: {}
    // 3's preds -> 1: 2 -> 1
    // 1's succs -> 3: 2 -> 3
    // Result: 2 -> 1 and 2 -> 3. No cycle.

    // What if we have 1->2, 2->3, 3->1 (already cyclic, but graph start is DAG).
    // If we start with 1->2, 2->3.
    // We want to add 3->1.
    // With current ops, it's hard to create a cycle because InsertNode and SwapNodes are designed to be safe?
    // Wait, SwapNodes might create a cycle if there's a path from b to a.

    // Let's try: 1 -> 2 -> 3. Swap(1, 2).
    // Preds 1: {}, Succs 1: {2}
    // Preds 2: {1}, Succs 2: {3}
    // 1's preds -> 2: {}
    // 2's succs -> 1: 3 -> 1
    // 2's preds -> 1: 1 -> 1 (self loop!)
    // 1's succs -> 2: 2 -> 2 (self loop!)

    // Actually, the current SwapNodes implementation:
    // 178: for p in preds_a { if p != idx_b { new_graph.add_edge(p, idx_b); } }
    // 183: for s in succs_b { if s != idx_a { new_graph.add_edge(idx_a, s); } }
    // 190: for p in preds_b { if p != idx_a { new_graph.add_edge(p, idx_a); } }
    // 195: for s in succs_a { if s != idx_b { new_graph.add_edge(idx_b, s); } }

    // If 1 -> 2 -> 3. a=1, b=2.
    // preds_a: {}, succs_a: {2}
    // preds_b: {1}, succs_b: {3}
    // p in preds_a: none
    // s in succs_b: 3 -> 1
    // p in preds_b: none (p is 1, and p != idx_a is false)
    // s in succs_a: none (s is 2, and s != idx_b is false)
    // Result: 3 -> 1. But we still have 2 -> 3?
    // Wait, 161-175 removes all edges connected to a and b.
    // So the only edges left are those not connected to 1 or 2.
    // In 1 -> 2 -> 3, only (2, 3) is connected to 2.
    // So after removal, no edges are left.
    // Then 3 -> 1 is added. Result: 3 -> 1.

    // To get a cycle, we need a node NOT involved in the swap to be part of the path.
    // 1 -> 2, 1 -> 3, 3 -> 2. Swap(1, 2).
    // Preds 1: {}, Succs 1: {2, 3}
    // Preds 2: {1, 3}, Succs 2: {}
    // 1's preds -> 2: {}
    // 2's succs -> 1: {}
    // 2's preds -> 1: 3 -> 1
    // 1's succs -> 2: 3 -> 2
    // Result: 3 -> 1 and 3 -> 2. No cycle.

    // What if we have a self-loop? The code avoids it with `if p != idx_b`.

    // To test cycle detection, we create a graph that is already cyclic
    // or use an operation that creates one. Since apply_ops checks for cycles at the end,
    // we can just provide a graph that we've manually made cyclic.
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
    graph.add_edge(n2, n1); // Cycle!

    let evolver = TopologicalSOPEvolver::new();
    let res = evolver.apply_ops(&graph, &vec![], exec);
    assert!(res.is_err(), "Should detect cycle in already cyclic graph");
}

#[tokio::test]
async fn test_llm_evolver_integration() {
    let (graph, exec) = setup_basic_graph();
    let topo_evolver = Arc::new(TopologicalSOPEvolver::new());
    let llm_client = Arc::new(MockLlmClient {
        response: "[{\"AdjustFuel\": {\"node_name\": \"node1\", \"delta\": 100}}, {\"AddRetry\": {\"node_name\": \"node2\", \"max_retries\": 3}}]".to_string(),
    });

    let evolver = LlmEvolver::new(llm_client, topo_evolver, exec);
    let report = ReflectionReport {
        original_sop_id: "test".into(),
        deviation_score: 0.8,
        suggested_improvements: vec!["increase fuel".into(), "add retry".into()],
        evolved_sop_structure: None,
        records: vec![],
    };
    let prediction = sisyphus_intel::simulation::SimulationResult {
        predicted_outcome: "ok".into(),
        path_id: "test".into(),
        success_probability: 1.0,
        estimated_cost: 100,
    };

    let new_graph = evolver
        .evolve(Arc::new(graph), &report, &prediction, None)
        .await
        .expect("Should evolve");

    let n1_idx = new_graph
        .graph
        .node_indices()
        .find(|&i| new_graph.graph.node_weight(i).unwrap().name() == "node1")
        .unwrap();
    let n1 = new_graph.graph.node_weight(n1_idx).unwrap();
    let wasm_n1 = n1
        .as_any()
        .downcast_ref::<WasmSOPNode>()
        .expect("Should be WasmSOPNode");
    assert_eq!(wasm_n1.fuel_limit, 200);

    let n2_retry_idx = new_graph
        .graph
        .node_indices()
        .find(|&i| new_graph.graph.node_weight(i).unwrap().name() == "node2_retry")
        .unwrap();
    assert!(new_graph
        .graph
        .node_weight(n2_retry_idx)
        .unwrap()
        .name()
        .contains("retry"));
}
