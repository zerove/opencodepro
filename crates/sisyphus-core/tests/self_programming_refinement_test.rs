#![cfg(feature = "broken_tests")]
use async_trait::async_trait;
use sisyphus_core::{ExecutionManager, IntelligenceManager, SisyphusKernel};
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
    async fn generate(&self, _prompt: &str, _stream: bool) -> Result<String, String> {
        Ok("".into())
    }
}

#[tokio::test]
async fn test_self_programming_refinement_loop() {
    // 1. Setup Mock Components
    let llm = Arc::new(FailingLlmClient {
        attempt: std::sync::Mutex::new(0),
    });
    let state_store = Arc::new(sisyphus_mem::MfsStore::new(".sisyphus/test_mfs"));
    let sop_store = Arc::new(sisyphus_mem::SopSqlStore::new_in_memory().await.unwrap());

    // 2. Setup SelfProgrammingManager
    // 简化初始化，使用最小依赖
    let exec_manager = Arc::new(ExecutionManager::new(
        state_store,
        sop_store,
        Arc::new(sisyphus_sandbox::WasmToolExecutor::new().unwrap()),
        Arc::new(sisyphus_tools_executor::executor::test_cmds::TestExecutor::new()),
        Arc::new(crate::intervention_manager::InterventionManager::new()),
        Arc::new(crate::internalization_manager::InternalizationManager::new(
            llm.clone(),
            Arc::new(sisyphus_knowledge::AssetManager::new(
                ".sisyphus/knowledge".into(),
            )),
        )),
        None,
    ));

    let sp_manager = crate::self_programming_manager::SelfProgrammingManager::new(
        llm.clone(),
        exec_manager,
        Arc::new(sisyphus_knowledge::AssetManager::new(
            ".sisyphus/knowledge".into(),
        )),
    );

    // 3. Define a gap
    let gap = ToolGap {
        gap_id: "test_gap".into(),
        required_capability: "test".into(),
        suggested_tool_spec: ToolSpec {
            tool_id: "fixed_tool".into(),
            required_capability: "test".into(),
            expected_input: serde_json::json!({}),
            expected_output: serde_json::json!({}),
            success_criteria: "returns ok".into(),
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
