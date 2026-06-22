use async_trait::async_trait;
use sisyphus_core::{SOPGraph, SisyphusKernel, WasmSOPNode};
use sisyphus_intel::{SisyphusIntel, StandardEvolver, StandardReflector};
use sisyphus_mem::MfsStore;
use sisyphus_types::{StateStore, ToolExecutor};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct MockExecutor {
    call_count: AtomicUsize,
}

#[async_trait]
impl ToolExecutor for MockExecutor {
    async fn execute(
        &self,
        _tool_id: &str,
        _args: Vec<u8>,
        fuel_limit: u64,
    ) -> Result<Vec<u8>, String> {
        let count = self.call_count.fetch_add(1, Ordering::SeqCst);
        // First 2 calls fail to test experience accumulation
        if count < 2 && fuel_limit <= 1_000_000 {
            return Err("ResourceExhausted: fuel limit too low".to_string());
        }
        Ok(b"Success".to_vec())
    }
}

#[tokio::test]
async fn test_experience_prediction_convergence() {
    let state_store = Arc::new(MfsStore::new("sqlite::memory:").await.unwrap());
    let tool_executor = Arc::new(MockExecutor {
        call_count: AtomicUsize::new(0),
    });

    let simulator = Arc::new(sisyphus_intel::MonteCarloSimulator::with_state_store(
        1000,
        state_store.clone(),
    ));
    let reflector = Arc::new(StandardReflector);
    let evolver = Arc::new(StandardEvolver::new());
    let intel = Arc::new(SisyphusIntel::new(simulator, reflector, evolver));

    let synthesizer = Arc::new(sisyphus_intel::GoalDrivenSynthesizer::new(
        tool_executor.clone(),
    ));
    let kernel = SisyphusKernel::new(state_store, tool_executor.clone(), intel, synthesizer);

    let sop_id = "exp_test_sop";
    let mut graph = SOPGraph::new();
    let node = Arc::new(WasmSOPNode {
        name: "node_1".into(),
        tool_id: "tool_1".into(),
        fuel_limit: 1_000_000,
        executor: tool_executor.clone(),
    });
    let start_node = graph.add_node(node);

    // Run 1: Recursive cognitive loop - iterates until convergence or max iterations
    // First iteration fails (count=0), evolves fuel, second iteration succeeds
    let (_res1, report1, _) = kernel
        .execute_with_cognition(&graph, start_node, sop_id, None, None)
        .await
        .unwrap();
    // Should contain deviation info from the iterations
    assert!(report1.contains("deviation="));
    // Should show iterations in the cognitive loop
    assert!(report1.contains("iterations"));
    // Should show convergence
    assert!(report1.contains("converged="));

    // Run 2: Should now succeed on first iteration (call_count >= 2)
    let (_res2, report2, _) = kernel
        .execute_with_cognition(&graph, start_node, sop_id, None, None)
        .await
        .unwrap();
    assert!(report2.contains("converged=true"));

    // Verify experience accumulated: multiple records from recursive iterations
    let history_val = kernel
        .state_store
        .get_state_json(&format!("history:{}", sop_id))
        .await
        .unwrap();
    let history: Option<sisyphus_types::ExecutionHistory> =
        history_val.and_then(|val| serde_json::from_value(val).ok());
    assert!(history.is_some());
    // First call produced 3 iterations, second call converged in 1 iteration = 4 total records
    let records = history.unwrap().records;
    assert!(
        records.len() >= 3,
        "Should have at least 3 experience records from recursive iterations"
    );

    // Verify the last record shows success
    let last = records.last().unwrap();
    assert!(last.success, "Last iteration should have succeeded");
}
