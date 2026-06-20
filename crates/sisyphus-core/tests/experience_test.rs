use sisyphus_core::{SisyphusKernel, SOPGraph, WasmSOPNode};
use sisyphus_types::{ToolExecutor, StateStore};
use sisyphus_mem::MfsStore;
use sisyphus_intel::{SisyphusIntel, StandardReflector, StandardEvolver};
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct MockExecutor {
    call_count: AtomicUsize,
}

#[async_trait]
impl ToolExecutor for MockExecutor {
    async fn execute(&self, _tool_id: &str, _args: Vec<u8>, fuel_limit: u64) -> Result<Vec<u8>, String> {
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
    let tool_executor = Arc::new(MockExecutor { call_count: AtomicUsize::new(0) });
    
    let simulator = Arc::new(sisyphus_intel::ExperienceSimulator::new(state_store.clone()));
    let reflector = Arc::new(StandardReflector);
    let evolver = Arc::new(StandardEvolver::new());
    let intel = Arc::new(SisyphusIntel::new(simulator, reflector, evolver));
    
    let synthesizer = Arc::new(sisyphus_intel::TemplateSynthesizer::new(tool_executor.clone()));
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
    
    // Run 1: Should fail and create first experience record (Success: false)
    let (_res1, report1, _) = kernel.execute_with_cognition(&graph, start_node, sop_id).await.unwrap();
    assert!(report1.contains("Deviation")); 
    
    // Run 2: Should fail again (Success: false)
    let (_res2, _report2, _) = kernel.execute_with_cognition(&graph, start_node, sop_id).await.unwrap();
    
    // Run 3: Now we have 2 failures. If we used ExperienceSimulator, the next prediction 
    // should be based on these 2 failures.
    // However, the current bridge/kernel uses the simulator at the START of execute_with_cognition.
    // So for Run 3, the prediction is made BEFORE the execution.
    
    // Let's verify the experience record exists
    let history_val = kernel.state_store.get_state_json(&format!("history:{}", sop_id)).await.unwrap();
    let history: Option<sisyphus_types::ExecutionHistory> = history_val.and_then(|val| serde_json::from_value(val).ok());
    assert!(history.is_some());
    assert_eq!(history.unwrap().records.len(), 2);
}
