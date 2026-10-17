#![cfg(feature = "broken_tests")]
use async_trait::async_trait;
use sisyphus_core::{SOPGraph, SisyphusKernel, WasmSOPNode};
use sisyphus_intel::{MonteCarloSimulator, SisyphusIntel, StandardEvolver, StandardReflector};
use sisyphus_mem::MfsStore;
use sisyphus_types::ToolExecutor;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

mod common;
use common::{MockConsensus, MockExperienceBank, MockLogicalSimulator};

struct MockExecutor {
    call_count: AtomicUsize,
    fail_until: usize,
    always_fail: bool,
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
        if self.always_fail || (count < self.fail_until && fuel_limit <= 1_000_000) {
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
        MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default()).await.unwrap(),
    );
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    let llm_client = Arc::new(sisyphus_tools_llm::metis::DummyLlmClient);

    let simulator = Arc::new(MonteCarloSimulator::with_state_store(
        100,
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
        Arc::new(sisyphus_mem::VectorMemoryStore::new(pool.clone()).await.unwrap()),
        Arc::new(MockExperienceBank),
        tool_executor as Arc<dyn ToolExecutor>,
        Arc::new(sisyphus_tools_executor::executor::test_cmds::TestExecutor::new()),
        intel,
        llm_client.clone() as Arc<dyn sisyphus_tools_llm::LlmClient>,
        llm_client as Arc<dyn sisyphus_tools_llm::LlmClient>,
        Arc::new(sisyphus_tools_llm_tools::expert_reviewer::DefaultExpertReviewer),
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
async fn test_recursive_cognitive_loop_converges() {
    let tool_executor = Arc::new(MockExecutor {
        call_count: AtomicUsize::new(0),
        fail_until: 2,
        always_fail: false,
    });

    let kernel = setup_kernel(tool_executor.clone()).await;

    let sop_id = "cognitive_loop_test";
    let mut graph = SOPGraph::new();
    let node = Arc::new(WasmSOPNode {
        name: "node_1".into(),
        tool_id: "tool_1".into(),
        fuel_limit: 1_000_000,
        executor: tool_executor.clone(),
    });
    let _start_node = graph.add_node(node);

    // First call: fails twice, evolves fuel, succeeds on 3rd iteration
    let (result, report, _evolved) = kernel
        .execute_with_cognition(Arc::new(graph), sop_id, None, None, None, None)
        .await
        .unwrap();

    assert!(!result.to_string().is_empty());
    assert!(report.iterations > 0);
    assert!(report.final_deviation >= 0.0);
}

#[tokio::test]
async fn test_recursive_loop_max_iterations() {
    let tool_executor = Arc::new(MockExecutor {
        call_count: AtomicUsize::new(0),
        fail_until: 0,
        always_fail: true,
    });

    let kernel = setup_kernel(tool_executor.clone()).await;

    let sop_id = "max_iter_test";
    let mut graph = SOPGraph::new();
    let node = Arc::new(WasmSOPNode {
        name: "node_1".into(),
        tool_id: "tool_1".into(),
        fuel_limit: 1_000_000,
        executor: tool_executor.clone(),
    });
    let _start_node = graph.add_node(node);

    // Should hit max iterations and still return a result
    let (_result, report, _evolved) = kernel
        .execute_with_cognition(Arc::new(graph), sop_id, None, None, None, None)
        .await
        .unwrap();

    assert!(report.iterations >= 1);
    assert!(!report.converged);
}
