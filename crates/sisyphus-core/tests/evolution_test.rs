use async_trait::async_trait;
use sisyphus_core::{SOPGraph, SisyphusKernel, WasmSOPNode};
use sisyphus_intel::{MonteCarloSimulator, SisyphusIntel, StandardEvolver, StandardReflector};
use sisyphus_mem::MfsStore;
use sisyphus_types::ToolExecutor;
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
    ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
        let count = self.call_count.fetch_add(1, Ordering::SeqCst);
        if count == 0 && fuel_limit <= 1_000_000 {
            return Err(sisyphus_types::TypesError::ToolExecutionError { tool_id: _tool_id.to_string(), details: "ResourceExhausted: fuel limit too low".to_string() });
        }
        Ok(b"Success".to_vec())
    }
}

#[tokio::test]
async fn test_sop_evolution_fuel_increase() {
    let state_store = Arc::new(MfsStore::new("sqlite::memory:").await.unwrap());
    let tool_executor = Arc::new(MockExecutor {
        call_count: AtomicUsize::new(0),
    });

    let simulator = Arc::new(MonteCarloSimulator::new(1));
    let reflector = Arc::new(StandardReflector);
    let evolver = Arc::new(StandardEvolver::new());
    let intel = Arc::new(SisyphusIntel::new(simulator, reflector, evolver));

    let synthesizer = Arc::new(sisyphus_intel::GoalDrivenSynthesizer::new(
        tool_executor.clone(),
    ));
    let kernel = SisyphusKernel::new(state_store, tool_executor.clone(), intel, Arc::new(sisyphus_tools_llm::metis::DummyLlmClient), synthesizer);

    let sop_id = "test_evolution_sop";
    let mut graph = SOPGraph::new();
    let node = Arc::new(WasmSOPNode {
        name: "node_1".into(),
        tool_id: "tool_1".into(),
        fuel_limit: 1_000_000,
        executor: tool_executor.clone(),
    });
    let start_node = graph.add_node(node);

    // First run: should fail and evolve
    let (result, report, evolved) = kernel
        .execute_with_cognition(&graph, start_node, sop_id, None, None)
        .await
        .unwrap();
    assert!(
        result.to_string().contains("ResourceExhausted") || report.contains("ResourceExhausted")
    );
    assert!(evolved.is_some(), "SOP should have evolved");

    // Load evolved graph from MFS
    let evolved_graph = kernel.load_sop(sop_id).await.unwrap();
    let new_start_node = evolved_graph.graph.node_indices().next().unwrap();

    // Second run: should succeed with increased fuel
    let (result2, _report2, _) = kernel
        .execute_with_cognition(&evolved_graph, new_start_node, sop_id, None, None)
        .await
        .unwrap();
    assert!(result2.to_string().contains("Success"));
}
