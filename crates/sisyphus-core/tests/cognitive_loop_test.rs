use sisyphus_core::{SisyphusKernel, SOPGraph, WasmSOPNode};
use sisyphus_types::ToolExecutor;
use sisyphus_mem::MfsStore;
use sisyphus_intel::{SisyphusIntel, MonteCarloSimulator, StandardReflector, StandardEvolver};
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct MockExecutor {
    call_count: AtomicUsize,
    fail_until: usize,
    always_fail: bool,
}

#[async_trait]
impl ToolExecutor for MockExecutor {
    async fn execute(&self, _tool_id: &str, _args: Vec<u8>, fuel_limit: u64) -> Result<Vec<u8>, String> {
        let count = self.call_count.fetch_add(1, Ordering::SeqCst);
        if self.always_fail || (count < self.fail_until && fuel_limit <= 1_000_000) {
            return Err("ResourceExhausted: fuel limit too low".to_string());
        }
        Ok(b"Success".to_vec())
    }
}

#[tokio::test]
async fn test_recursive_cognitive_loop_converges() {
    let state_store = Arc::new(MfsStore::new("sqlite::memory:").await.unwrap());
    let tool_executor = Arc::new(MockExecutor { call_count: AtomicUsize::new(0), fail_until: 2, always_fail: false });
    
    let simulator = Arc::new(MonteCarloSimulator::with_state_store(100, state_store.clone()));
    let reflector = Arc::new(StandardReflector);
    let evolver = Arc::new(StandardEvolver::new());
    let intel = Arc::new(SisyphusIntel::new(simulator, reflector, evolver));
    
    let synthesizer = Arc::new(sisyphus_intel::GoalDrivenSynthesizer::new(tool_executor.clone()));
    let kernel = SisyphusKernel::new(state_store, tool_executor.clone(), intel, synthesizer);
    
    let sop_id = "cognitive_loop_test";
    let mut graph = SOPGraph::new();
    let node = Arc::new(WasmSOPNode {
        name: "node_1".into(),
        tool_id: "tool_1".into(),
        fuel_limit: 1_000_000,
        executor: tool_executor.clone(),
    });
    let start_node = graph.add_node(node);
    
    // First call: fails twice, evolves fuel, succeeds on 3rd iteration
    let (result, report, evolved) = kernel.execute_with_cognition(&graph, start_node, sop_id).await.unwrap();
    
    assert!(result.to_string().contains("Success"));
    assert!(report.contains("converged=true"));
    assert!(report.contains("iter[0]"));
    assert!(report.contains("deviation="));
    assert!(evolved.is_some(), "Should have evolved graph after convergence");
}

#[tokio::test]
async fn test_execute_goal_with_recursive_cognition() {
    let state_store = Arc::new(MfsStore::new("sqlite::memory:").await.unwrap());
    let tool_executor = Arc::new(MockExecutor { call_count: AtomicUsize::new(0), fail_until: 1, always_fail: false });
    
    let simulator = Arc::new(MonteCarloSimulator::with_state_store(100, state_store.clone()));
    let reflector = Arc::new(StandardReflector);
    let evolver = Arc::new(StandardEvolver::new());
    let intel = Arc::new(SisyphusIntel::new(simulator, reflector, evolver));
    
    let synthesizer = Arc::new(sisyphus_intel::GoalDrivenSynthesizer::new(tool_executor.clone()));
    let kernel = SisyphusKernel::new(state_store, tool_executor.clone(), intel, synthesizer);
    
    let sop_id = "goal_test";
    
    // Execute goal with multi-node pipeline
    let (result, report) = kernel.execute_goal("fetch process store", sop_id).await.unwrap();
    
    assert!(result.to_string().contains("Success"));
    assert!(report.contains("Linear Pipeline"));
    assert!(report.contains("fetch"));
    assert!(report.contains("process"));
    assert!(report.contains("sink"));
    assert!(report.contains("CognitiveLoop"));
    assert!(report.contains("converged="));
}

#[tokio::test]
async fn test_recursive_loop_max_iterations() {
    let state_store = Arc::new(MfsStore::new("sqlite::memory:").await.unwrap());
    // This executor always fails to test max iteration limit
    let tool_executor = Arc::new(MockExecutor { call_count: AtomicUsize::new(0), fail_until: 0, always_fail: true });
    
    let simulator = Arc::new(MonteCarloSimulator::with_state_store(100, state_store.clone()));
    let reflector = Arc::new(StandardReflector);
    let evolver = Arc::new(StandardEvolver::new());
    let intel = Arc::new(SisyphusIntel::new(simulator, reflector, evolver));
    
    let synthesizer = Arc::new(sisyphus_intel::GoalDrivenSynthesizer::new(tool_executor.clone()));
    let kernel = SisyphusKernel::new(state_store, tool_executor.clone(), intel, synthesizer);
    
    let sop_id = "max_iter_test";
    let mut graph = SOPGraph::new();
    let node = Arc::new(WasmSOPNode {
        name: "node_1".into(),
        tool_id: "tool_1".into(),
        fuel_limit: 1_000_000,
        executor: tool_executor.clone(),
    });
    let start_node = graph.add_node(node);
    
    // Should hit max iterations (3) and still return a result
    let (result, report, _evolved) = kernel.execute_with_cognition(&graph, start_node, sop_id).await.unwrap();
    
    // Debug: print the report
    eprintln!("REPORT: {}", report);
    
    // Result is still "Success" string but contains error
    assert!(report.contains("iterations"));
    assert!(report.contains("converged=false"));
    assert!(report.contains("Resource exhaustion"));
}

#[tokio::test]
async fn test_parallel_goal_synthesis_and_execution() {
    let state_store = Arc::new(MfsStore::new("sqlite::memory:").await.unwrap());
    let tool_executor = Arc::new(MockExecutor { call_count: AtomicUsize::new(0), fail_until: 0, always_fail: false });
    
    let simulator = Arc::new(MonteCarloSimulator::with_state_store(100, state_store.clone()));
    let reflector = Arc::new(StandardReflector);
    let evolver = Arc::new(StandardEvolver::new());
    let intel = Arc::new(SisyphusIntel::new(simulator, reflector, evolver));
    
    let synthesizer = Arc::new(sisyphus_intel::GoalDrivenSynthesizer::new(tool_executor.clone()));
    let kernel = SisyphusKernel::new(state_store, tool_executor.clone(), intel, synthesizer);
    
    let sop_id = "parallel_test";
    
    // Test parallel goal - fan-out then merge
    let (result, report) = kernel.execute_goal("fetch and process parallel", sop_id).await.unwrap();
    
    assert!(result.to_string().contains("Success"));
    assert!(report.contains("Fan-out"));
    assert!(report.contains("Merge"));
    assert!(report.contains("CognitiveLoop"));
    assert!(report.contains("converged=true"));
}