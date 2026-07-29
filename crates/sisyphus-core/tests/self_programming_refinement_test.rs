#![cfg(feature = "broken_tests")]
use async_trait::async_trait;
use sisyphus_core::{ExecutionManager, IntelligenceManager};
use sisyphus_intel::{GapAnalyzer, SimulationResult, ToolGap, ToolVerifier};
use sisyphus_tools_llm::LlmClient;
use sisyphus_types::tools::ToolSpec;
use std::sync::Arc;

// Mock LlmClient that simulates an initial failure
struct FailingLlmClient {
    attempt: std::sync::Mutex<usize>,
}

#[async_trait]
impl LlmClient for FailingLlmClient {
    async fn chat(&self, _system: &str, prompt: &str, _stream: bool) -> Result<String, String> {
        let mut attempt = self.attempt.lock().unwrap();
        *attempt += 1;

        if *attempt == 1 {
            // First attempt: return broken code
            Ok("```rust\nfn main() { println!(\"Broken\"); }\n```".to_string())
        } else {
            // Second attempt: return working code
            Ok("```rust\nuse sisyphus_wasm_sdk::prelude::*;\nfn main() { register_tool!(\"test\", || Ok(\"ok\".to_string())); }\n```".to_string())
        }
    }
    async fn generate(&self, _prompt: &str) -> Result<String, String> {
        Ok("".into())
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

#[tokio::test]
async fn test_self_programming_refinement_loop() {
    // 1. Setup Mock Components
    let llm = Arc::new(FailingLlmClient {
        attempt: std::sync::Mutex::new(0),
    });
    let state_store = Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:")
            .await
            .unwrap(),
    );
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    let sop_store = Arc::new(sisyphus_mem::SopSqlStore::new(pool.clone()).await.unwrap());

    // 2. Setup SelfProgrammingManager
    let exec_manager = Arc::new(ExecutionManager::new(
        state_store,
        sop_store,
        Arc::new(sisyphus_sandbox::WasmToolExecutor::new().unwrap()),
        Arc::new(sisyphus_tools_executor::executor::test_cmds::TestExecutor::new()),
        Arc::new(sisyphus_core::intervention_manager::InterventionManager::new()),
        Arc::new(
            sisyphus_core::internalization_manager::InternalizationManager::new(
                llm.clone(),
                Arc::new(sisyphus_knowledge::AssetManager::new(
                    ".sisyphus/knowledge".into(),
                )),
            ),
        ),
        None,
    ));

    let sp_manager = sisyphus_core::self_programming_manager::SelfProgrammingManager::new(
        llm.clone(),
        exec_manager,
        Arc::new(sisyphus_knowledge::AssetManager::new(
            ".sisyphus/knowledge".into(),
        )),
        None,
    );

    // 3. Define a gap
    let gap = ToolGap {
        gap_id: "test_gap".into(),
        required_capability: "test".into(),
        failing_node: "test_node".into(),
        suggested_tool_spec: ToolSpec {
            tool_id: "fixed_tool".into(),
            required_capability: "test".into(),
            expected_input: serde_json::json!({}),
            expected_output: serde_json::json!({}),
            success_criteria: "returns ok".into(),
            test_command: None,
            workdir: None,
        },
    };

    // 4. Resolve
    let result = sp_manager.resolve_gap(gap).await;

    // 5. Assertions
    assert!(result.is_ok());
    let attempt_count = *llm.attempt.lock().unwrap();
    assert_eq!(
        attempt_count, 2,
        "Should have attempted twice due to initial failure"
    );
}
