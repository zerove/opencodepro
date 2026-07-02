use async_trait::async_trait;
use sisyphus_core::SisyphusKernel;
use sisyphus_intel::{SisyphusIntel, StandardEvolver, StandardReflector};
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
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        Ok(b"Success".to_vec())
    }
}

#[tokio::test]
async fn test_dynamic_synthesis_execution() {
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

    let kernel = SisyphusKernel::new(state_store, tool_executor.clone(), intel, Arc::new(sisyphus_tools_llm::metis::DummyLlmClient), synthesizer);

    // Test a "fetch, process, store" goal which should synthesize a 3-node DAG
    let (result, report) = kernel
        .execute_goal("fetch process store task", "dyn_sop_1", None)
        .await
        .unwrap();

    assert!(result.to_string().contains("Success"));
    assert!(report.contains("Linear Pipeline"));
    assert!(report.contains("fetch"));
    assert!(report.contains("process"));
    assert!(report.contains("sink"));
    assert_eq!(tool_executor.call_count.load(Ordering::SeqCst), 3);
}
