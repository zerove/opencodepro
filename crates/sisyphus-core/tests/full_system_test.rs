#![cfg(feature = "broken_tests")]
use async_trait::async_trait;
use parking_lot::Mutex;
use sisyphus_core::{Consensus, SOPGraph, SisyphusKernel};
use sisyphus_intel::{
    GoalDrivenSynthesizer, MonteCarloSimulator, SisyphusIntel, StandardEvolver, StandardReflector,
    StrategicSopSynthesizer,
};
use sisyphus_mem::MfsStore;
use sisyphus_tools_llm::metis::LlmClient;
use sisyphus_tools_llm_tools::meta_learner::MetaLearner;
use sisyphus_types::{GoalRef, Milestone, MilestoneStatus, Phase, ToolExecutor};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

mod common;
use common::{MockExperienceBank, MockLogicalSimulator};

// --- Mocks ---

struct MockLlmClient {
    call_count: AtomicUsize,
}
#[async_trait::async_trait]
impl LlmClient for MockLlmClient {
    async fn generate(&self, prompt: &str) -> Result<String, String> {
        if prompt.contains("Robustness") {
            Ok(r#"{"nodes": [{"id": "n1", "name": "robust_node", "tool_id": "fast_tool", "args": {}}], "edges": [], "start_node": "n1"}"#.to_string())
        } else if prompt.contains("Efficiency") {
            Ok(r#"{"nodes": [{"id": "n1", "name": "efficient_node", "tool_id": "fast_tool", "args": {}}], "edges": [], "start_node": "n1"}"#.to_string())
        } else if prompt.contains("critical_findings") || prompt.contains("Critique") {
            // Return valid critique JSON
            Ok(r#"{"critical_findings": [], "risk_score": 0.1, "suggested_refinement": "none", "should_proceed": true}"#.to_string())
        } else {
            Ok(r#"{"nodes": [{"id": "n1", "name": "standard_node", "tool_id": "fast_tool", "args": {}}], "edges": [], "start_node": "n1"}"#.to_string())
        }
    }

    async fn chat(&self, system: &str, _user: &str, _stream: bool) -> Result<String, String> {
        if system.contains("senior engineering director") {
            Ok(r#"{
                "health_assessment": "The project is at critical risk due to repeated failures in core infrastructure access.",
                "trajectory": "Stagnating",
                "risk_factors": ["Critical resource unavailable", "Blocking structural failure"],
                "recommendations": [
                    {
                        "action": "Replace Secure-Vault tool",
                        "rationale": "The current vault access tool is consistently failing with ResourceUnavailable",
                        "priority": "high",
                        "target_milestone": "milestone-1"
                    }
                ],
                "confidence": 0.95
            }"#.to_string())
        } else {
            let count = self.call_count.fetch_add(1, Ordering::SeqCst);
            if count % 2 == 0 {
                // Even calls: proposal (SOPGraphConfig) - must have at least one node
                Ok(r#"{"nodes": [{"name": "mock_node", "kind": {"Wasm": {"tool_id": "mock_tool", "fuel_limit": 1000000}}}], "edges": []}"#.to_string())
            } else {
                // Odd calls: review (AuditReview)
                Ok(r#"{"approved": true, "feedback": "Looks good"}"#.to_string())
            }
        }
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
    async fn propose(&self, _k: &str, _v: serde_json::Value) -> Result<(), String> {
        Ok(())
    }
}

async fn setup_kernel(llm: Arc<dyn LlmClient>, tool_exec: Arc<dyn ToolExecutor>) -> SisyphusKernel {
    let store = Arc::new(MfsStore::new("sqlite::memory:").await.unwrap());
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();

    let simulator = Arc::new(MonteCarloSimulator::new(10));
    let intel = Arc::new(SisyphusIntel::new(
        simulator.clone(),
        Arc::new(MockLogicalSimulator),
        Arc::new(StandardReflector),
        Arc::new(StandardEvolver::new()),
    ));

    let temp_dir = tempfile::tempdir().unwrap();
    let ml_path = temp_dir.path().join("ml_dir");
    let meta_learner = Arc::new(Mutex::new(MetaLearner::new(ml_path.to_str().unwrap())));

    let inner_syn = Arc::new(GoalDrivenSynthesizer::new(tool_exec.clone()));
    let syn = Arc::new(StrategicSopSynthesizer::new(inner_syn, simulator));

    let roadmap_store = Arc::new(
        sisyphus_mem::RoadmapSqlStore::new(pool.clone())
            .await
            .unwrap(),
    );
    roadmap_store.init_schema().await.unwrap();

    SisyphusKernel::new(
        store,
        Arc::new(sisyphus_mem::SopSqlStore::new(pool.clone()).await.unwrap()),
        Arc::new(
            sisyphus_mem::VectorMemoryStore::new(pool.clone())
                .await
                .unwrap(),
        ),
        Arc::new(MockExperienceBank),
        tool_exec,
        intel,
        llm.clone() as Arc<dyn LlmClient>,
        llm.clone() as Arc<dyn LlmClient>,
        syn as Arc<dyn sisyphus_intel::FlowSynthesizer>,
        meta_learner,
        Arc::new(MockConsensus),
        roadmap_store,
        Arc::new(sisyphus_tools_llm::DummyEmbeddingClient),
    )
    .with_strategic_reflector(llm)
}

#[tokio::test]
async fn test_full_system_integration_flow() {
    println!("🚀 Starting Full-System Integration Test...");

    let llm = Arc::new(MockLlmClient {
        call_count: AtomicUsize::new(0),
    });
    let tool_exec = Arc::new(MockExecutor);

    let kernel = setup_kernel(
        llm.clone() as Arc<dyn LlmClient>,
        tool_exec as Arc<dyn ToolExecutor>,
    )
    .await;

    let project_id = "project-ai-agent";
    let milestone_id = "milestone-infra";
    let phase_id = "phase-1";
    let goal_id = "goal-llm-connector";
    let goal_text = "Implement LLM Connector";

    kernel
        .roadmap_manager
        .create_project(project_id, "Build Simple AI Agent", "Infrastructure Setup")
        .await
        .unwrap();
    let milestone = Milestone {
        id: milestone_id.to_string(),
        name: "Core Infrastructure".into(),
        phases: vec![Phase {
            id: phase_id.to_string(),
            name: "Initialization".into(),
            goals: vec![GoalRef {
                goal_id: goal_id.to_string(),
                description: goal_text.to_string(),
                status: MilestoneStatus::Pending,
            }],
            depends_on: vec![],
        }],
        status: MilestoneStatus::Pending,
    };
    kernel
        .roadmap_manager
        .add_milestone(project_id, milestone)
        .await
        .unwrap();

    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let (res, _report, _health) = kernel
        .execute_and_record(
            goal_text,
            "test_sop_id",
            project_id,
            milestone_id,
            phase_id,
            goal_id,
            Some(event_tx),
        )
        .await
        .unwrap();

    assert!(!res.to_string().is_empty());

    println!("✅ Full-System Integration Test Passed!");
}

#[tokio::test]
async fn test_complex_strategic_goal() {
    println!("🚀 Starting Complex Strategic Goal Test...");

    let llm = Arc::new(MockLlmClient {
        call_count: AtomicUsize::new(0),
    });
    let tool_exec = Arc::new(MockExecutor);

    let kernel = setup_kernel(
        llm.clone() as Arc<dyn LlmClient>,
        tool_exec as Arc<dyn ToolExecutor>,
    )
    .await;

    let complex_goal =
        "Fetch API specs, process rate limiting rules, validate security, and save configuration";
    let (_event_tx, _event_rx) = mpsc::unbounded_channel::<sisyphus_types::ExecutionEvent>();
    let (res, report) = kernel
        .execute_goal(complex_goal, "complex_sop_id", "default", None, None)
        .await
        .unwrap();

    assert!(!res.to_string().is_empty());
    assert!(report.contains("Plan:"));

    println!("✅ Complex Strategic Goal Test Passed!");
}
