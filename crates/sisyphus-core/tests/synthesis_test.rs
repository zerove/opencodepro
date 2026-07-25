#![cfg(feature = "broken_tests")]
use async_trait::async_trait;
use sisyphus_core::SisyphusKernel;
use sisyphus_intel::{SisyphusIntel, StandardEvolver, StandardReflector};
use sisyphus_mem::MfsStore;
use sisyphus_types::ToolExecutor;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct MockConsensus;
#[async_trait]
impl sisyphus_core::Consensus for MockConsensus {
    async fn is_leader(&self) -> bool {
        true
    }
    async fn get_leader(&self) -> String {
        "mock-leader".into()
    }
    async fn propose(&self, _key: &str, _value: serde_json::Value) -> Result<(), String> {
        Ok(())
    }
}

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

    let kernel = SisyphusKernel::new(
        state_store,
        tool_executor.clone(),
        intel,
        Arc::new(sisyphus_tools_llm::metis::DummyLlmClient),
        synthesizer,
        Arc::new(parking_lot::Mutex::new(
            sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(".omo/test_meta"),
        )),
        Arc::new(MockConsensus),
    );

    // Test a "fetch, process, store" goal which should synthesize a 3-node DAG
    let (result, report) = kernel
        .execute_goal("fetch process store task", "dyn_sop_1", None)
        .await
        .unwrap();

    assert!(result.to_string().contains("Success"));
    assert!(report.iterations > 0);
    // Result matches goal intent: it produced a 3-node execution
    assert_eq!(tool_executor.call_count.load(Ordering::SeqCst), 3);
}
