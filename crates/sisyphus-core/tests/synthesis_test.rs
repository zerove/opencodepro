#![cfg(feature = "broken_tests")]
use async_trait::async_trait;
use sisyphus_core::SisyphusKernel;
use sisyphus_intel::{MonteCarloSimulator, SisyphusIntel, StandardEvolver, StandardReflector};
use sisyphus_mem::MfsStore;
use sisyphus_tools_llm::metis::LlmClient;
use sisyphus_types::ToolExecutor;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

mod common;
use common::{MockConsensus, MockExperienceBank, MockLogicalSimulator};

// --- Mocks ---

struct MockLlmClient;
#[async_trait::async_trait]
impl LlmClient for MockLlmClient {
    async fn generate(&self, prompt: &str) -> Result<String, String> {
        if prompt.contains("critical_findings") || prompt.contains("Task: Identify") {
            return Ok(r#"{"critical_findings": [], "risk_score": 0.1, "suggested_refinement": "none", "should_proceed": true}"#.to_string());
        }
        Ok(r#"{"nodes": [{"name": "standard_node", "kind": {"Wasm": {"tool_id": "fast_tool", "fuel_limit": 1000000}}}], "edges": []}"#.to_string())
    }

    async fn chat(&self, _system: &str, user: &str, _stream: bool) -> Result<String, String> {
        if user.contains("Please review this proposal")
            || user.contains("Return a JSON object with fields 'approved'")
        {
            return Ok(r#"{"approved": true, "feedback": "Looks good"}"#.to_string());
        }
        Ok(r#"{"nodes": [{"name": "standard_node", "kind": {"Wasm": {"tool_id": "fast_tool", "fuel_limit": 1000000}}}], "edges": []}"#.to_string())
    }

    fn get_params_for(&self, _agent_id: &str) -> (String, String, String) {
        (
            "test_key".into(),
            "http://test:8080".into(),
            "test-model".into(),
        )
    }

    fn breaker(&self) -> sisyphus_tools_llm::circuit_breaker::CircuitBreaker {
        sisyphus_tools_llm::circuit_breaker::CircuitBreaker::default()
    }
}

struct MockExecutor;
#[async_trait::async_trait]
impl ToolExecutor for MockExecutor {
    async fn execute(
        &self,
        _id: &str,
        _args: Vec<u8>,
        _fuel: u64,
    ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
        Ok(b"success".to_vec())
    }
}

async fn setup_kernel(
    llm: Arc<dyn LlmClient>,
    synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer>,
) -> SisyphusKernel {
    let state_store = Arc::new(MfsStore::new("sqlite::memory:").await.unwrap());
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    let tool_exec = Arc::new(MockExecutor);

    let simulator = Arc::new(MonteCarloSimulator::new(10));
    let intel = Arc::new(SisyphusIntel::new(
        simulator,
        Arc::new(MockLogicalSimulator),
        Arc::new(StandardReflector),
        Arc::new(StandardEvolver::new()),
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
        tool_exec as Arc<dyn ToolExecutor>,
        intel,
        llm.clone() as Arc<dyn LlmClient>,
        llm as Arc<dyn LlmClient>,
        synthesizer,
        Arc::new(parking_lot::Mutex::new(
            sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(".omo/test_meta"),
        )),
        Arc::new(MockConsensus),
        Arc::new(sisyphus_mem::RoadmapSqlStore::new(pool).await.unwrap()),
        Arc::new(sisyphus_tools_llm::DummyEmbeddingClient),
    )
}

#[tokio::test]
async fn test_dynamic_synthesis_execution() {
    let llm = Arc::new(MockLlmClient);
    let tool_exec = Arc::new(MockExecutor);
    let synthesizer = Arc::new(sisyphus_intel::GoalDrivenSynthesizer::new(
        tool_exec.clone() as Arc<dyn ToolExecutor>,
    ));

    let kernel = setup_kernel(
        llm as Arc<dyn LlmClient>,
        synthesizer as Arc<dyn sisyphus_intel::FlowSynthesizer>,
    )
    .await;

    // Test a "fetch, process, store" goal which should synthesize a multi-node DAG
    let (result, report) = kernel
        .execute_goal(
            "fetch process store task",
            "dyn_sop_1",
            "default",
            None,
            None,
        )
        .await
        .unwrap();

    assert!(!result.to_string().is_empty());
    assert!(report.contains("Plan:"));
}
