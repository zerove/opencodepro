#![cfg(feature = "broken_tests")]
use async_trait::async_trait;
use parking_lot::Mutex;
use sisyphus_core::Consensus;
use sisyphus_core::SisyphusKernel;
use sisyphus_intel::{
    GoalDrivenSynthesizer, MetaLearningSynthesizer, MonteCarloSimulator, SisyphusIntel,
    StandardEvolver, StandardReflector, StrategicSopSynthesizer,
};
use sisyphus_mem::MfsStore;
use sisyphus_tools_llm::metis::LlmClient;
use sisyphus_tools_llm_tools::meta_learner::MetaLearner;
use sisyphus_types::ToolExecutor;
use std::sync::Arc;

mod common;
use common::{MockExperienceBank, MockLogicalSimulator};

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
        // Review requests: return AuditReview
        if user.contains("Please review this proposal")
            || user.contains("Return a JSON object with fields 'approved'")
        {
            return Ok(r#"{"approved": true, "feedback": "Looks good"}"#.to_string());
        }
        // All other chat calls: return valid SOPGraphConfig
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

struct MockConsensus;
#[async_trait::async_trait]
impl Consensus for MockConsensus {
    async fn is_leader(&self) -> bool {
        true
    }
    async fn get_leader(&self) -> String {
        "node-1".into()
    }
    async fn propose(
        &self,
        _k: &str,
        _v: serde_json::Value,
    ) -> Result<(), sisyphus_types::TypesError> {
        Ok(())
    }
}

async fn setup_kernel(
    llm: Arc<dyn LlmClient>,
    synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer>,
    meta_learner: Arc<Mutex<MetaLearner>>,
) -> SisyphusKernel {
    let store = Arc::new(
        MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .unwrap(),
    );
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
        store,
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
        meta_learner,
        Arc::new(MockConsensus),
        Arc::new(sisyphus_mem::RoadmapSqlStore::new(pool).await.unwrap()),
        Arc::new(sisyphus_tools_llm::DummyEmbeddingClient),
    )
}

#[tokio::test]
async fn test_meta_learning_closed_loop() {
    let llm = Arc::new(MockLlmClient);
    let tool_exec = Arc::new(MockExecutor);
    let temp_dir = tempfile::tempdir().unwrap();
    let ml_path = temp_dir.path().join("ml_dir");
    let meta_learner = Arc::new(Mutex::new(MetaLearner::new(ml_path.to_str().unwrap())));
    let inner_syn = Arc::new(GoalDrivenSynthesizer::new(
        tool_exec.clone() as Arc<dyn ToolExecutor>
    ));
    let syn = Arc::new(MetaLearningSynthesizer::new(
        inner_syn,
        meta_learner.clone(),
    ));

    let kernel = setup_kernel(
        llm as Arc<dyn LlmClient>,
        syn as Arc<dyn sisyphus_intel::FlowSynthesizer>,
        meta_learner.clone(),
    )
    .await;

    let goal = "Fetch AI news";
    let sop_id = "test_meta_goal";

    // 1. First execution: Standard SOP
    let (res1, report1) = kernel
        .execute_goal(goal, sop_id, "default", None, None)
        .await
        .unwrap();
    assert!(!res1.to_string().is_empty());
    assert!(report1.contains("Plan:"));

    // 2. Manually record a failure for this goal with a specific strategy
    {
        let mut ml = meta_learner.lock();
        for _ in 0..3 {
            ml.record_outcome(sisyphus_types::MetaLearningSample {
                context: serde_json::json!({ "goal": goal }),
                strategy: "Robustness".into(),
                params: serde_json::json!({}),
                success: true,
                duration_secs: 1.0,
                timestamp: 0,
            })
            .unwrap();
        }
    }

    // 3. Second execution: Should now use the "Robustness" hint
    let (res2, report2) = kernel
        .execute_goal(goal, sop_id, "default", None, None)
        .await
        .unwrap();
    assert!(!res2.to_string().is_empty());
    // The MetaLearningSynthesizer will inject the hint
    assert!(report2.contains("Plan:"));
}

#[tokio::test]
async fn test_strategic_simulator_selection() {
    let llm = Arc::new(MockLlmClient);
    let tool_exec = Arc::new(MockExecutor);

    let inner_syn = Arc::new(GoalDrivenSynthesizer::new(
        tool_exec.clone() as Arc<dyn ToolExecutor>
    ));
    let strat_syn = Arc::new(StrategicSopSynthesizer::new(
        inner_syn,
        Arc::new(MonteCarloSimulator::new(100)),
    ));

    let temp_dir = tempfile::tempdir().unwrap();
    let ml_path = temp_dir.path().join("ml_dir");
    let meta_learner = Arc::new(Mutex::new(MetaLearner::new(ml_path.to_str().unwrap())));

    let kernel = setup_kernel(
        llm as Arc<dyn LlmClient>,
        strat_syn as Arc<dyn sisyphus_intel::FlowSynthesizer>,
        meta_learner,
    )
    .await;

    let goal = "Fetch AI news";
    let sop_id = "test_strat_goal";

    let (res, report) = kernel
        .execute_goal(goal, sop_id, "default", None, None)
        .await
        .unwrap();
    assert!(!res.to_string().is_empty());
    assert!(report.contains("Plan:"));
}
