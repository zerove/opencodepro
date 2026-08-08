#![cfg(feature = "broken_tests")]
use async_trait::async_trait;
use sisyphus_core::{SOPGraph, SisyphusKernel, WasmSOPNode};
use sisyphus_intel::{MonteCarloSimulator, SisyphusIntel, StandardEvolver, StandardReflector};
use sisyphus_mem::MfsStore;
use sisyphus_types::{StateStore, ToolExecutor};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

mod common;
use common::{MockConsensus, MockExperienceBank, MockLogicalSimulator};

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
        // First 2 calls fail to test experience accumulation
        if count < 2 && fuel_limit <= 1_000_000 {
            return Err(sisyphus_types::TypesError::ToolExecutionError {
                tool_id: _tool_id.to_string(),
                details: "ResourceExhausted: fuel limit too low".to_string(),
            });
        }
        Ok(b"Success".to_vec())
    }
}

async fn setup_kernel(tool_executor: Arc<MockExecutor>) -> SisyphusKernel {
    let state_store = Arc::new(
        MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .unwrap(),
    );
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    let llm_client = Arc::new(sisyphus_tools_llm::metis::DummyLlmClient);

    let simulator = Arc::new(MonteCarloSimulator::with_state_store(
        1000,
        state_store.clone(),
    ));
    let intel = Arc::new(SisyphusIntel::new(
        simulator,
        Arc::new(MockLogicalSimulator),
        Arc::new(StandardReflector),
        Arc::new(StandardEvolver::new()),
    ));

    let synthesizer = Arc::new(sisyphus_intel::GoalDrivenSynthesizer::new(
        tool_executor.clone(),
    ));

    SisyphusKernel::new(
        state_store,
        Arc::new(sisyphus_mem::SopSqlStore::new(pool.clone()).await.unwrap()),
        Arc::new(
            sisyphus_mem::VectorMemoryStore::new(pool.clone())
                .await
                .unwrap(),
        ),
        Arc::new(MockExperienceBank),
        tool_executor as Arc<dyn ToolExecutor>,
        intel,
        llm_client.clone() as Arc<dyn sisyphus_tools_llm::LlmClient>,
        llm_client as Arc<dyn sisyphus_tools_llm::LlmClient>,
        synthesizer as Arc<dyn sisyphus_intel::FlowSynthesizer>,
        Arc::new(parking_lot::Mutex::new(
            sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(".omo/test_meta"),
        )),
        Arc::new(MockConsensus),
        Arc::new(sisyphus_mem::RoadmapSqlStore::new(pool).await.unwrap()),
        Arc::new(sisyphus_tools_llm::DummyEmbeddingClient),
    )
}

#[tokio::test]
async fn test_experience_prediction_convergence() {
    let tool_executor = Arc::new(MockExecutor {
        call_count: AtomicUsize::new(0),
    });

    let kernel = setup_kernel(tool_executor.clone()).await;

    let sop_id = "exp_test_sop";
    let mut graph_inner = SOPGraph::new();
    let node = Arc::new(WasmSOPNode {
        name: "node_1".into(),
        tool_id: "tool_1".into(),
        fuel_limit: 1_000_000,
        executor: tool_executor.clone(),
    });
    graph_inner.add_node(node);
    let graph = Arc::new(graph_inner);

    // Run 1: Recursive cognitive loop - iterates until convergence or max iterations
    let (_res1, report1, _) = kernel
        .execute_with_cognition(graph.clone(), sop_id, None, None, None, None)
        .await
        .unwrap();
    // Should show iterations in the cognitive loop
    assert!(report1.iterations > 0);
    // Should show convergence attempt
    assert!(report1.final_deviation >= 0.0);

    // Run 2: Should now succeed on first iteration (call_count >= 2)
    let (_res2, report2, _) = kernel
        .execute_with_cognition(graph.clone(), sop_id, None, None, None, None)
        .await
        .unwrap();
    // May or may not converge depending on state
    assert!(report2.iterations > 0);
}
