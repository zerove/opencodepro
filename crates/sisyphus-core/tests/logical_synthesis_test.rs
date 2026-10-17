#![cfg(feature = "broken_tests")]
use sisyphus_core::SisyphusKernel;
use sisyphus_intel::{
    GoalDrivenSynthesizer, MonteCarloSimulator, SisyphusIntel, StandardEvolver, StandardReflector,
};
use sisyphus_mem::MfsStore;
use sisyphus_tools_llm_tools::meta_learner::MetaLearner;
use sisyphus_types::{NodeOutput, TypesError};
use std::sync::Arc;

mod common;
use common::{MockConsensus, MockEvolver, MockExperienceBank, MockLogicalSimulator, MockReflector};

use async_trait::async_trait;

struct MockExecutor {
    responses: std::collections::HashMap<String, String>,
}

#[async_trait::async_trait]
impl sisyphus_types::ToolExecutor for MockExecutor {
    async fn execute(
        &self,
        tool_id: &str,
        _args: Vec<u8>,
        _fuel: u64,
    ) -> Result<Vec<u8>, TypesError> {
        let res = self.responses.get(tool_id).cloned().unwrap_or_else(|| "default".into());
        Ok(res.into_bytes())
    }
}

/// Smart mock LLM that returns different JSON based on prompt content.
struct SmartMockLlm {
    call_count: std::sync::atomic::AtomicU32,
}

impl SmartMockLlm {
    fn new() -> Self {
        Self {
            call_count: std::sync::atomic::AtomicU32::new(0),
        }
    }
}

#[async_trait]
impl sisyphus_tools_llm::LlmClient for SmartMockLlm {
    async fn generate(&self, prompt: &str) -> Result<String, String> {
        if prompt.contains("critical_findings") || prompt.contains("Task: Identify") {
            return Ok(r#"{"critical_findings": [], "risk_score": 0.1, "suggested_refinement": "none", "should_proceed": true}"#.to_string());
        }
        Ok(r#"{"nodes": [{"name": "check", "kind": {"Wasm": {"tool_id": "check", "fuel_limit": 1000000}}}], "edges": []}"#.to_string())
    }

    async fn chat(
        &self,
        _system_prompt: &str,
        user_message: &str,
        _json_mode: bool,
    ) -> Result<String, String> {
        if user_message.contains("Please review this proposal")
            || user_message.contains("Return a JSON object with fields 'approved'")
        {
            return Ok(r#"{"approved": true, "feedback": "Looks good"}"#.to_string());
        }
        // All other calls (proposal, refine, fallback) return a valid SOPGraphConfig
        Ok(r#"{"nodes": [{"name": "check", "kind": {"Wasm": {"tool_id": "check", "fuel_limit": 1000000}}}], "edges": []}"#.to_string())
    }

    fn get_params_for(&self, _agent_id: &str) -> (String, String, String) {
        (
            "mock-model".into(),
            "mock-key".into(),
            "https://mock.api".into(),
        )
    }

    fn breaker(&self) -> sisyphus_tools_llm::circuit_breaker::CircuitBreaker {
        sisyphus_tools_llm::circuit_breaker::CircuitBreaker::default()
    }
}

#[tokio::test]
async fn test_logical_synthesis_branching_execution() {
    // 1. Setup Smart Mock LLM
    let llm = Arc::new(SmartMockLlm::new());

    // 2. Setup Executor
    let mut responses = std::collections::HashMap::new();
    responses.insert("check".to_string(), "success".to_string());
    let tool_executor = Arc::new(MockExecutor { responses });

    // 3. Use GoalDrivenSynthesizer
    let synthesizer = Arc::new(GoalDrivenSynthesizer::new(
        tool_executor.clone() as Arc<dyn sisyphus_types::ToolExecutor>
    ));

    // 4. Setup Kernel
    let store = Arc::new(
        MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default()).await.unwrap(),
    );
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    let intel = Arc::new(SisyphusIntel::new(
        Arc::new(MonteCarloSimulator::new(100)),
        Arc::new(MockLogicalSimulator),
        Arc::new(MockReflector),
        Arc::new(MockEvolver),
    ));

    let kernel = SisyphusKernel::new(
        store,
        Arc::new(sisyphus_mem::SopSqlStore::new(pool.clone()).await.unwrap()),
        Arc::new(sisyphus_mem::VectorMemoryStore::new(pool.clone()).await.unwrap()),
        Arc::new(MockExperienceBank),
        tool_executor.clone() as Arc<dyn sisyphus_types::ToolExecutor>,
        Arc::new(sisyphus_tools_executor::executor::test_cmds::TestExecutor::new()),
        intel,
        llm.clone() as Arc<dyn sisyphus_tools_llm::LlmClient>,
        llm as Arc<dyn sisyphus_tools_llm::LlmClient>,
        Arc::new(sisyphus_tools_llm_tools::expert_reviewer::DefaultExpertReviewer),
        synthesizer as Arc<dyn sisyphus_intel::FlowSynthesizer>,
        Arc::new(parking_lot::Mutex::new(MetaLearner::new(".omo/test_meta"))),
        Arc::new(MockConsensus),
        Arc::new(sisyphus_mem::RoadmapSqlStore::new(pool).await.unwrap()),
        Arc::new(sisyphus_tools_llm::DummyEmbeddingClient),
    );

    // 5. Execute Goal
    let (output, report) =
        kernel.execute_goal("test branching", "sop-branch", "default", None, None).await.unwrap();

    // 6. Verify
    assert!(!output.to_string().is_empty());
    assert!(report.contains("Plan:"));
}
