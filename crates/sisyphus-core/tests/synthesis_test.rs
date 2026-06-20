use std::sync::Arc;
use sisyphus_core::SisyphusKernel;
use sisyphus_types::ToolExecutor;
use sisyphus_mem::MfsStore;
use sisyphus_intel::{SisyphusIntel, StandardReflector, StandardEvolver};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};

struct MockExecutor {
    call_count: AtomicUsize,
}

#[async_trait]
impl ToolExecutor for MockExecutor {
    async fn execute(&self, _tool_id: &str, _args: Vec<u8>, _fuel_limit: u64) -> Result<Vec<u8>, String> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        Ok(b"Success".to_vec())
    }
}

#[tokio::test]
async fn test_dynamic_synthesis_execution() {
    let state_store = Arc::new(MfsStore::new("sqlite::memory:").await.unwrap());
    let tool_executor = Arc::new(MockExecutor { call_count: AtomicUsize::new(0) });
    
    let simulator = Arc::new(sisyphus_intel::ExperienceSimulator::new(state_store.clone()));
    let reflector = Arc::new(StandardReflector);
    let evolver = Arc::new(StandardEvolver::new());
    let intel = Arc::new(SisyphusIntel::new(simulator, reflector, evolver));
    let synthesizer = Arc::new(sisyphus_intel::TemplateSynthesizer::new(tool_executor.clone()));
    
    let kernel = SisyphusKernel::new(state_store, tool_executor.clone(), intel, synthesizer);
    
    // Test a "complex" goal which should synthesize a 3-node DAG
    let (result, report) = kernel.execute_goal("complex task", "dyn_sop_1").await.unwrap();
    
    assert!(result.to_string().contains("Success"));
    assert!(report.contains("Linear Pipeline: Fetch -> Process -> Sink"));
    assert_eq!(tool_executor.call_count.load(Ordering::SeqCst), 3);
}
