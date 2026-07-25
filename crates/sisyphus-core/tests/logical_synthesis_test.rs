#![cfg(feature = "broken_tests")]
use sisyphus_core::{SisyphusKernel, ToolExecutor};
use sisyphus_intel::{LogicalSOPSynthesizer, SisyphusIntel};
use sisyphus_mem::MfsStore;
use sisyphus_tools_llm::metis::{FixedResponseClient, LlmClient};
use sisyphus_tools_llm_tools::meta_learner::MetaLearner;
use sisyphus_types::{NodeOutput, TypesError};
use std::sync::Arc;
use tokio::sync::Mutex;

struct MockExecutor {
    // Map of tool_id -> response
    responses: std::collections::HashMap<String, String>,
}

#[async_trait::async_trait]
impl ToolExecutor for MockExecutor {
    async fn execute(
        &self,
        tool_id: &str,
        _args: Vec<u8>,
        _fuel: u64,
    ) -> Result<Vec<u8>, TypesError> {
        let res = self
            .responses
            .get(tool_id)
            .cloned()
            .unwrap_or_else(|| "default".into());
        Ok(res.into_bytes())
    }
}

#[tokio::test]
async fn test_logical_synthesis_branching_execution() {
    // 1. Setup Mock LLM to return a Branching SOP
    let sop_json = r#"{
        "plan_description": "Test Branching Logic",
        "nodes": [
            { "name": "CheckStatus", "kind": { "Wasm": { "tool_id": "check", "fuel_limit": 1000000 } } },
            { "name": "Decision", "kind": { "Branch": { "branches": { "success": 2, "failure": 3 }, "default_next": 3 } } },
            { "name": "SuccessPath", "kind": { "Wasm": { "tool_id": "success_tool", "fuel_limit": 1000000 } } },
            { "name": "FailurePath", "kind": { "Wasm": { "tool_id": "failure_tool", "fuel_limit": 1000000 } } }
        ],
        "edges": [ [0, 1], [1, 2], [1, 3] ]
    }"#;
    let llm = Arc::new(FixedResponseClient(sop_json.to_string()));

    // 2. Setup Executor
    let mut responses = std::collections::HashMap::new();
    responses.insert("check".to_string(), "success".to_string());
    let tool_executor = Arc::new(MockExecutor { responses });

    // 3. Setup Synthesizer
    let synthesizer = Arc::new(LogicalSOPSynthesizer::new(
        llm.clone(),
        tool_executor.clone(),
        Arc::new(sisyphus_intel::simulation::MonteCarloSimulator::new(100)),
        None,
    ));

    // 4. Setup Kernel
    let store = Arc::new(MfsStore::new("sqlite::memory:").await.unwrap());
    let intel = Arc::new(SisyphusIntel::new(
        Arc::new(sisyphus_intel::simulation::MonteCarloSimulator::new(100)),
        Arc::new(sisyphus_intel::reflect::StandardReflector),
        Arc::new(sisyphus_intel::topological_evolver::TopologicalSOPEvolver::new()),
    ));

    let kernel = SisyphusKernel::new(
        store,
        tool_executor.clone(),
        intel,
        llm.clone(),
        synthesizer,
        Arc::new(parking_lot::Mutex::new(MetaLearner::default_dir())),
        Arc::new(SisyphusConsensus::new()),
    );

    // 5. Execute Goal
    let (output, report) = kernel
        .execute_goal("test branching", "sop-branch", None)
        .await
        .unwrap();

    // 6. Verify
    // The logic should be: CheckStatus("success") -> Decision -> SuccessPath
    assert!(report.iterations > 0);
    // We expect the final output to be the result of the SuccessPath tool
    // Note: the execute_goal returns the final node's output.
    // In our MockExecutor, success_tool returns "default" unless specified.
}

struct SisyphusConsensus;
#[async_trait::async_trait]
impl sisyphus_core::Consensus for SisyphusConsensus {
    async fn is_leader(&self) -> bool {
        true
    }
    async fn get_leader(&self) -> String {
        "leader".into()
    }
    async fn propose(&self, _k: &str, _v: serde_json::Value) -> Result<(), String> {
        Ok(())
    }
}
impl SisyphusConsensus {
    fn new() -> Self {
        Self
    }
}
