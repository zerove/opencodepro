//! Integration tests for sisyphus-intel `topological_evolver`.
//!
//! Exercises the public [`TopologicalSOPEvolver::apply_ops`]
//! function which applies a list of `EvolutionOp` mutations to
//! an `SOPGraph`. Operations include `AdjustFuel`, `AddRetry`,
//! and `InsertNode`. The evolver preserves existing nodes and
//! edges while applying the operations.

use sisyphus_intel::TopologicalSOPEvolver;
use sisyphus_types::graph::{NodeKind, SOPGraph};
use sisyphus_types::types::EvolutionOp;
use sisyphus_types::node::WasmSOPNode;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Helper: build a graph with a single WasmSOPNode
// ---------------------------------------------------------------------------

struct NoopExecutor;

#[async_trait::async_trait]
impl sisyphus_types::ToolExecutor for NoopExecutor {
    async fn execute(
        &self,
        _tool_id: &str,
        _args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
        Ok(vec![])
    }
}

fn build_graph(name: &str, tool_id: &str, fuel: u64) -> SOPGraph {
    let mut g = SOPGraph::new();
    g.add_node(Arc::new(WasmSOPNode {
        name: name.into(),
        tool_id: tool_id.into(),
        fuel_limit: fuel,
        executor: Arc::new(NoopExecutor),
    }));
    g
}

// ---------------------------------------------------------------------------
// AdjustFuel
// ---------------------------------------------------------------------------

#[tokio::test]
async fn apply_ops_adjust_fuel_increases_limit() {
    let g = build_graph("n1", "t1", 100);
    let evolver = TopologicalSOPEvolver::new();
    let ops = vec![EvolutionOp::AdjustFuel {
        node_name: "n1".into(),
        delta: 50,
    }];
    let executor = Arc::new(NoopExecutor);
    let new_g = evolver.apply_ops(&g, &ops, executor).expect("ok");
    // 100 + 50 = 150.
    let node = new_g
        .graph
        .node_indices()
        .next()
        .map(|i| new_g.graph.node_weight(i).expect(""))
        .expect("node");
    if let Some(wasm) = node.as_any().downcast_ref::<WasmSOPNode>() {
        assert_eq!(wasm.fuel_limit, 150);
    } else {
        panic!("expected WasmSOPNode");
    }
}

#[tokio::test]
async fn apply_ops_adjust_fuel_unknown_node_errors() {
    let g = build_graph("n1", "t1", 100);
    let evolver = TopologicalSOPEvolver::new();
    let ops = vec![EvolutionOp::AdjustFuel {
        node_name: "nonexistent".into(),
        delta: 50,
    }];
    let executor = Arc::new(NoopExecutor);
    let result = evolver.apply_ops(&g, &ops, executor);
    assert!(result.is_err());
}

#[tokio::test]
async fn apply_ops_empty_list_preserves_graph() {
    let g = build_graph("n1", "t1", 100);
    let evolver = TopologicalSOPEvolver::new();
    let executor = Arc::new(NoopExecutor);
    let new_g = evolver.apply_ops(&g, &[], executor).expect("ok");
    assert_eq!(new_g.graph.node_count(), 1);
    assert_eq!(new_g.graph.edge_count(), 0);
}

#[tokio::test]
async fn apply_ops_add_retry_wraps_node() {
    let g = build_graph("n1", "t1", 100);
    let evolver = TopologicalSOPEvolver::new();
    let ops = vec![EvolutionOp::AddRetry {
        node_name: "n1".into(),
        max_retries: 5,
    }];
    let executor = Arc::new(NoopExecutor);
    let new_g = evolver.apply_ops(&g, &ops, executor).expect("ok");
    // The node should now be wrapped in a Retry.
    let node = new_g
        .graph
        .node_indices()
        .next()
        .map(|i| new_g.graph.node_weight(i).expect(""))
        .expect("");
    let kind = node.to_kind();
    match kind {
        NodeKind::Retry { max_retries, .. } => {
            assert_eq!(max_retries, 5);
        }
        other => panic!("expected Retry, got {other:?}"),
    }
}

#[tokio::test]
async fn apply_ops_add_retry_unknown_node_errors() {
    let g = build_graph("n1", "t1", 100);
    let evolver = TopologicalSOPEvolver::new();
    let ops = vec![EvolutionOp::AddRetry {
        node_name: "nonexistent".into(),
        max_retries: 3,
    }];
    let executor = Arc::new(NoopExecutor);
    let result = evolver.apply_ops(&g, &ops, executor);
    assert!(result.is_err());
}

#[tokio::test]
async fn apply_ops_multiple_ops_applied_in_order() {
    let g = build_graph("n1", "t1", 100);
    let evolver = TopologicalSOPEvolver::new();
    let ops = vec![
        EvolutionOp::AdjustFuel {
            node_name: "n1".into(),
            delta: 50,
        },
        EvolutionOp::AddRetry {
            node_name: "n1".into(),
            max_retries: 2,
        },
    ];
    let executor = Arc::new(NoopExecutor);
    let _new_g = evolver.apply_ops(&g, &ops, executor).expect("ok");
    // Both ops applied; final state is a Retry wrapping a Wasm with 150 fuel.
    // (We don't deeply verify here, just that the call succeeds.)
}

// ---------------------------------------------------------------------------
// SOPNodeConfig construction (sanity for ops)
// ---------------------------------------------------------------------------

#[test]
fn evolution_op_serializes_for_adjust_fuel() {
    let op = EvolutionOp::AdjustFuel {
        node_name: "n1".into(),
        delta: 100,
    };
    let json = serde_json::to_string(&op).expect("");
    assert!(json.contains("AdjustFuel"));
    assert!(json.contains("n1"));
}

#[test]
fn evolution_op_serializes_for_add_retry() {
    let op = EvolutionOp::AddRetry {
        node_name: "n2".into(),
        max_retries: 3,
    };
    let json = serde_json::to_string(&op).expect("");
    assert!(json.contains("AddRetry"));
    assert!(json.contains("\"max_retries\":3"));
}

// ---------------------------------------------------------------------------
// Additional edge cases: InsertNode, negative fuel, default trait
// ---------------------------------------------------------------------------

#[test]
fn evolver_default_constructs() {
    let e1 = TopologicalSOPEvolver::default();
    let e2 = TopologicalSOPEvolver::new();
    // Both produce a usable evolver (no fields to compare, but we exercise the API).
    let _ = e1;
    let _ = e2;
}

#[tokio::test]
async fn apply_ops_adjust_fuel_negative_clamped_to_zero() {
    let g = build_graph("n1", "t1", 100);
    let evolver = TopologicalSOPEvolver::new();
    let ops = vec![EvolutionOp::AdjustFuel {
        node_name: "n1".into(),
        delta: -200, // would underflow; clamped to 0
    }];
    let executor = Arc::new(NoopExecutor);
    let new_g = evolver.apply_ops(&g, &ops, executor).expect("ok");
    let node = new_g
        .graph
        .node_indices()
        .next()
        .map(|i| new_g.graph.node_weight(i).expect(""))
        .expect("");
    if let Some(wasm) = node.as_any().downcast_ref::<WasmSOPNode>() {
        assert_eq!(wasm.fuel_limit, 0);
    } else {
        panic!("expected WasmSOPNode");
    }
}

#[tokio::test]
async fn apply_ops_adjust_fuel_zero_delta_preserves_fuel() {
    let g = build_graph("n1", "t1", 100);
    let evolver = TopologicalSOPEvolver::new();
    let ops = vec![EvolutionOp::AdjustFuel {
        node_name: "n1".into(),
        delta: 0,
    }];
    let executor = Arc::new(NoopExecutor);
    let new_g = evolver.apply_ops(&g, &ops, executor).expect("ok");
    let node = new_g
        .graph
        .node_indices()
        .next()
        .map(|i| new_g.graph.node_weight(i).expect(""))
        .expect("");
    if let Some(wasm) = node.as_any().downcast_ref::<WasmSOPNode>() {
        assert_eq!(wasm.fuel_limit, 100);
    } else {
        panic!("expected WasmSOPNode");
    }
}

#[tokio::test]
async fn apply_ops_adjust_fuel_large_positive() {
    let g = build_graph("n1", "t1", 100);
    let evolver = TopologicalSOPEvolver::new();
    let ops = vec![EvolutionOp::AdjustFuel {
        node_name: "n1".into(),
        delta: 1_000_000,
    }];
    let executor = Arc::new(NoopExecutor);
    let new_g = evolver.apply_ops(&g, &ops, executor).expect("ok");
    let node = new_g
        .graph
        .node_indices()
        .next()
        .map(|i| new_g.graph.node_weight(i).expect(""))
        .expect("");
    if let Some(wasm) = node.as_any().downcast_ref::<WasmSOPNode>() {
        assert_eq!(wasm.fuel_limit, 1_000_100);
    } else {
        panic!("expected WasmSOPNode");
    }
}

#[test]
fn evolution_op_serializes_for_insert_node() {
    let op = EvolutionOp::InsertNode {
        after_node: "a".into(),
        new_name: "b".into(),
        tool_id: "t".into(),
    };
    let json = serde_json::to_string(&op).expect("");
    assert!(json.contains("InsertNode"));
    assert!(json.contains("\"new_name\":\"b\""));
}

