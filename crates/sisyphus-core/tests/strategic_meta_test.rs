use sisyphus_core::Consensus;
use sisyphus_core::{CoreError, SOPGraph, SisyphusKernel};
use sisyphus_intel::{
    GoalDrivenSynthesizer, MetaLearningSynthesizer, MonteCarloSimulator, SimulationResult,
    Simulator, SisyphusIntel, StandardEvolver, StandardReflector, StrategicSopSynthesizer,
};
use sisyphus_mem::MfsStore;
use sisyphus_tools_llm::metis::LlmClient;
use sisyphus_tools_llm_tools::meta_learner::MetaLearner;
use sisyphus_types::{ExecutionContext, ToolExecutor, WorkMode};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

// --- Mocks ---

struct MockLlmClient;
#[async_trait::async_trait]
impl LlmClient for MockLlmClient {
    async fn generate(&self, prompt: &str) -> Result<String, String> {
        if prompt.contains("Robustness") {
            Ok(r#"{"nodes": [{"id": "n1", "name": "robust_node", "tool_id": "fast_tool", "args": {}}], "edges": [], "start_node": "n1"}"#.to_string())
        } else if prompt.contains("Efficiency") {
            Ok(r#"{"nodes": [{"id": "n1", "name": "efficient_node", "tool_id": "fast_tool", "args": {}}], "edges": [], "start_node": "n1"}"#.to_string())
        } else {
            Ok(r#"{"nodes": [{"id": "n1", "name": "standard_node", "tool_id": "fast_tool", "args": {}}], "edges": [], "start_node": "n1"}"#.to_string())
        }
    }

    async fn chat(&self, _system: &str, _user: &str, _stream: bool) -> Result<String, String> {
        Ok("Mock chat response".into())
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

#[tokio::test]
async fn test_meta_learning_closed_loop() {
    let store = Arc::new(MfsStore::new("sqlite::memory:").await.unwrap());
    let tool_exec = Arc::new(MockExecutor);
    let llm = Arc::new(MockLlmClient);
    let simulator = Arc::new(MonteCarloSimulator::new(10));
    let intel = Arc::new(SisyphusIntel::new(
        simulator.clone(),
        Arc::new(StandardReflector),
        Arc::new(StandardEvolver::new()),
    ));

    let temp_dir = tempfile::tempdir().unwrap();
    let ml_path = temp_dir.path().join("ml_dir");
    let meta_learner = Arc::new(Mutex::new(MetaLearner::new(ml_path.to_str().unwrap())));
    let inner_syn = Arc::new(GoalDrivenSynthesizer::new(tool_exec.clone()));
    let syn = Arc::new(MetaLearningSynthesizer::new(
        inner_syn,
        meta_learner.clone(),
    ));

    let kernel = SisyphusKernel::new(
        store,
        tool_exec,
        intel,
        llm.clone(),
        syn,
        meta_learner.clone(),
        Arc::new(MockConsensus),
    );

    let goal = "Fetch AI news";
    let sop_id = "test_meta_goal";

    // 1. First execution: Standard SOP
    let (res1, report1) = kernel.execute_goal(goal, sop_id, None).await.unwrap();
    assert!(report1.contains("Standard"));

    // 2. Manually record a failure for this goal with a specific strategy
    {
        let mut ml = meta_learner.lock().unwrap();
        ml.record_outcome(sisyphus_types::MetaLearningSample {
            context: serde_json::json!({ "goal": goal }),
            strategy: "Robustness".into(),
            success: true, // We want to steer it towards Robustness
            duration_secs: 1.0,
            timestamp: 0,
        })
        .unwrap();
        // Add a few more to meet the min_samples = 2 requirement in MetaLearner
        for _ in 0..2 {
            ml.record_outcome(sisyphus_types::MetaLearningSample {
                context: serde_json::json!({ "goal": goal }),
                strategy: "Robustness".into(),
                success: true,
                duration_secs: 1.0,
                timestamp: 0,
            })
            .unwrap();
        }
    }

    // 3. Second execution: Should now use the "Robustness" hint
    let (res2, report2) = kernel.execute_goal(goal, sop_id, None).await.unwrap();
    // The MetaLearningSynthesizer will inject the hint, and MockLlmClient will return the robust SOP
    assert!(report2.contains("Robustness") || res2.to_string().contains("robust"));
}

#[tokio::test]
async fn test_strategic_simulator_selection() {
    let store = Arc::new(MfsStore::new("sqlite::memory:").await.unwrap());
    let tool_exec = Arc::new(MockExecutor);
    let llm = Arc::new(MockLlmClient);
    let simulator = Arc::new(MonteCarloSimulator::new(100));
    let intel = Arc::new(SisyphusIntel::new(
        simulator.clone(),
        Arc::new(StandardReflector),
        Arc::new(StandardEvolver::new()),
    ));

    let inner_syn = Arc::new(GoalDrivenSynthesizer::new(tool_exec.clone()));
    let strat_syn = Arc::new(StrategicSopSynthesizer::new(inner_syn, simulator.clone()));

    let temp_dir = tempfile::tempdir().unwrap();
    let ml_path = temp_dir.path().join("ml_dir");
    let meta_learner = Arc::new(Mutex::new(MetaLearner::new(ml_path.to_str().unwrap())));

    let kernel = SisyphusKernel::new(
        store,
        tool_exec,
        intel,
        llm,
        strat_syn,
        meta_learner,
        Arc::new(MockConsensus),
    );

    let goal = "Fetch AI news";
    let sop_id = "test_strat_goal";

    let (res, report) = kernel.execute_goal(goal, sop_id, None).await.unwrap();
    assert!(report.contains("Strategic selection"));
}
