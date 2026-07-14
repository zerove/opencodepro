use std::sync::Arc;
use sisyphus_core::{SisyphusKernel, SOPGraph, CoreError};
use sisyphus_types::{ToolExecutor, TypesError, WasmSOPNode, NodeOutput};
use sisyphus_intel::{LlmEvolver, HybridReflector, StandardReflector, LlmReflector};
use sisyphus_tools_llm::LlmClient;
use async_trait::async_trait;
use serde_json::Value;

struct MockLlmClient;
#[async_trait]
impl LlmClient for MockLlmClient {
    async fn generate(&self, prompt: &str) -> Result<String, String> {
        if prompt.contains("EvolutionOps") {
            return Ok(r#"[{"AdjustFuel": {"node_name": "fuel_node", "delta": 100}}]"#.to_string());
        }
        Ok(r#"{"deviation": 0.8, "suggestions": ["Increase fuel for fuel_node"], "analysis": "Low fuel"}"#.to_string())
    }
    async fn chat(&self, _sys: &str, _ctx: &str, _stream: bool) -> Result<String, String> {
        Ok("Mock chat".into())
    }
}

struct MockJcodeExecutor;
#[async_trait]
impl ToolExecutor for MockJcodeExecutor {
    async fn execute(&self, tool_id: &str, _args: Vec<u8>, _fuel: u64) -> Result<Vec<u8>, TypesError> {
        if tool_id == "verify_evolution" {
            return Ok(serde_json::json!({
                "decision": "ACCEPT",
                "reason": "Verified",
                "improvement_score": 0.9
            }).to_string().into_bytes());
        }
        Err(TypesError::NodeExecutionError("Unknown tool".into()))
    }
}

#[tokio::test]
async fn test_jcode_evolution_loop() {
    // 1. Setup Graph
    let mut graph = SOPGraph::new();
    let node = Arc::new(WasmSOPNode {
        name: "fuel_node".into(),
        tool_id: "test_tool".into(),
        fuel_limit: 10,
        executor: Arc::new(MockJcodeExecutor), // Simplified for test
    });
    graph.add_node(node);
    let graph = Arc::new(graph);

    // 2. Setup Kernel Components
    let llm_client = Arc::new(MockLlmClient);
    let tool_executor = Arc::new(sisyphus_types::ExecutorRouter::new(Arc::new(MockJcodeExecutor)));
    
    // Manually register jcode prefix for the test
    // Note: In real code this is done in Bridge::new
    // (We can't mutate the router after creation unless we use a different pattern, 
    // so we'll just use a custom ToolExecutor that handles both)
    
    // For this test, we'll use a simple wrapper that mocks the behavior
    struct TestExecutor;
    #[async_trait]
    impl ToolExecutor for TestExecutor {
        async fn execute(&self, tool_id: &str, args: Vec<u8>, fuel: u64) -> Result<Vec<u8>, TypesError> {
            if tool_id == "verify_evolution" {
                return Ok(serde_json::json!({"decision": "ACCEPT", "reason": "OK"}).to_string().into_bytes());
            }
            if tool_id == "test_tool" && fuel < 50 {
                return Err(TypesError::NodeExecutionError("ResourceExhausted".into()));
            }
            Ok(b"Success".to_vec())
        }
    }
    let tool_executor = Arc::new(TestExecutor);

    let evolver = LlmEvolver::new(
        llm_client.clone(),
        Arc::new(sisyphus_intel::topological_evolver::TopologicalSOPEvolver::new()),
        tool_executor.clone(),
    );

    // 3. Manually trigger a simplified evolve loop to verify logic
    let report = sisyphus_intel::reflect::ReflectionReport {
        original_sop_id: "test".into(),
        deviation_score: 0.8,
        suggested_improvements: vec!["Increase fuel".into()],
        evolved_sop_structure: None,
        records: vec![],
    };

    let evolved_graph = evolver.evolve(graph.clone(), &report, &sisyphus_intel::simulation::SimulationResult::default(), None)
        .await
        .expect("Evolution should succeed");

    // 4. Verify the evolution
    let node_idx = evolved_graph.graph.node_indices().next().unwrap();
    let node = evolved_graph.graph.node_weight(node_idx).unwrap();
    let wasm_node = node.as_any().downcast_ref::<WasmSOPNode>().expect("Should be WasmNode");
    
    assert!(wasm_node.fuel_limit >= 100, "Fuel should have been increased by evolver");
    println!("Evolution verified: Fuel increased to {}", wasm_node.fuel_limit);
}
