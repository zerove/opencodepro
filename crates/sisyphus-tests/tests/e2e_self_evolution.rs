use std::sync::Arc;
use tokio::sync::mpsc;
use async_trait::async_trait;
use sisyphus_core::{SelfProgrammingManager, CoreError};
use sisyphus_intel::{ToolGap};
use sisyphus_types::{CognitiveEvent, tools::ToolSpec, node::{ToolExecutor, TestExecutor}};
use sisyphus_tools_llm::{LlmClient, circuit_breaker::CircuitBreaker};
use sisyphus_core::execution_manager::ExecutionManager;
use sisyphus_knowledge::AssetManager;
use sisyphus_mem::{SisyphusStore, MfsStore};
use sisyphus_types::errors::TypesError;
use sisyphus_types::TestResult;

/// A simple mock executor that implements both Tool and Test traits.
struct MockExecutor;

#[async_trait]
impl ToolExecutor for MockExecutor {
    async fn execute(&self, _tool_id: &str, _args: Vec<u8>, _fuel: u64) -> Result<Vec<u8>, TypesError> {
        Ok(b"mock success".to_vec())
    }
}

#[async_trait]
impl TestExecutor for MockExecutor {
    async fn run_test(&self, _command: &str, _workdir: Option<&str>) -> Result<TestResult, TypesError> {
        Ok(TestResult {
            success: true,
            stdout: "Test passed".into(),
            stderr: "".into(),
            exit_code: 0,
            duration_ms: 10,
            error_summary: None,
        })
    }
}

/// A programmable LLM client that returns pre-defined responses based on the prompt.
struct ProgrammableLlmClient {
    pub responses: std::collections::HashMap<String, String>,
}

#[async_trait]
impl LlmClient for ProgrammableLlmClient {
    async fn chat(&self, _sys: &str, prompt: &str, _stream: bool) -> Result<String, String> {
        for (key, val) in &self.responses {
            if prompt.contains(key) {
                return Ok(val.clone());
            }
        }
        Ok("I don't know how to do that.".into())
    }

    async fn generate(&self, _prompt: &str) -> Result<String, String> {
        Ok("Generic generation".into())
    }

    fn get_params_for(&self, _agent_id: &str) -> (String, String, String) {
        ("test_key".into(), "http://test:8080".into(), "test-model".into())
    }

    fn breaker(&self) -> CircuitBreaker {
        CircuitBreaker::default()
    }
}

#[tokio::test]
async fn test_self_evolution_e2e() {
    // 1. Setup LLM with a specific Rust tool implementation for the gap
    let mut responses = std::collections::HashMap::new();
    
    let rust_code = r#"
        use sisyphus_wasm_sdk::{SisyphusTool, ToolContext, ToolOutput};
        use serde_json::json;

        struct SymmetryAnalyzer;

        impl SisyphusTool for SymmetryAnalyzer {
            fn execute(&self, ctx: &ToolContext) -> Result<ToolOutput, String> {
                Ok(ToolOutput::json(json!({
                    "status": "success",
                    "symmetry_score": 0.95,
                    "analysis": "High symmetry detected"
                })))
            }
        }

        fn main() {
            sisyphus_wasm_sdk::register_tool(SymmetryAnalyzer);
        }
    "#;
    
    responses.insert("SymmetryAnalysis".to_string(), format!("```rust\n{}\n```", rust_code));
    let llm_client = Arc::new(ProgrammableLlmClient { responses });

    // 2. Setup persistence
    let store = SisyphusStore::new("sqlite::memory:").await.expect("Store init failed");
    
    // 3. Setup core components
    let execution_manager = Arc::new(ExecutionManager::new(
        store.state.clone(),
        store.sop.clone(),
        Arc::new(MockExecutor),
        Arc::new(MockExecutor),
        Arc::new(sisyphus_core::intervention_manager::InterventionManager::new()),
        Arc::new(sisyphus_core::internalization_manager::InternalizationManager::new(
            llm_client.clone(), 
            Arc::new(AssetManager::new(std::path::PathBuf::from("test_assets")))
        )),
        Some(Arc::new(sisyphus_visual::VisualPerceptionManager::new(
            Arc::new(sisyphus_visual::SisyphusVisualProvider::new(llm_client.clone())),
            llm_client.clone(),
        ))),
    ));

    let asset_manager = Arc::new(AssetManager::new(
        std::path::PathBuf::from("test_assets"),
    ));

    let sp_manager = SelfProgrammingManager::new(
        llm_client.clone(),
        execution_manager.clone(),
        asset_manager.clone(),
    );

    // 4. Define a tool gap
    let gap = ToolGap {
        gap_id: "gap_123".into(),
        failing_node: "node_123".into(),
        required_capability: "SymmetryAnalysis".into(),
        suggested_tool_spec: ToolSpec {
            tool_id: "symmetry_analyzer".into(),
            required_capability: "SymmetryAnalysis".into(),
            expected_input: "Symmetry Analysis of object X".into(),
            expected_output: "Symmetry score and analysis".into(),
            success_criteria: "Score between 0 and 1".into(),
        },
    };

    // 5. Monitor the event stream
    let (tx, mut rx) = mpsc::unbounded_channel::<CognitiveEvent>();
    sp_manager.set_event_sender(tx);

    // 6. Execute the self-programming loop
    let resolve_handle = tokio::spawn(async move {
        sp_manager.resolve_gap(gap).await
    });

    // 7. Assert the event sequence
    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event.clone());
        if let CognitiveEvent::ToolDeployed { .. } = event {
            break;
        }
    }

    let result = resolve_handle.await.expect("Join error").expect("Self-programming failed");

    // Verify the events were emitted in order
    assert!(events.iter().any(|e| matches!(e, CognitiveEvent::ToolGenerationStarted { .. })), "Should start generation");
    assert!(events.iter().any(|e| matches!(e, CognitiveEvent::ToolVerified { .. })), "Should verify tool");
    assert!(events.iter().any(|e| matches!(e, CognitiveEvent::ToolDeployed { .. })), "Should deploy tool");

    println!("E2E Self-Evolution verified successfully!");
}
