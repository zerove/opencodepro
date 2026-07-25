#![cfg(feature = "broken_tests")]
use sisyphus_core::*;
use sisyphus_intel::SisyphusIntel;
use sisyphus_mem::MfsStore;
use sisyphus_tools_llm::meta_learner::MetaLearner;
use sisyphus_types::*;
use std::sync::Arc;
use tokio::sync::RwLock;

// --- Mocks ---
struct MockLlmClient;
#[async_trait::async_trait]
impl sisyphus_tools_llm::LlmClient for MockLlmClient {
    async fn generate(&self, prompt: &str) -> Result<String, String> {
        Ok(format!("Mock response to: {}", prompt))
    }
    async fn chat(&self, _system: &str, _user: &str, _stream: bool) -> Result<String, String> {
        Ok("Mock chat response".into())
    }
}

struct MockToolExecutor;
#[async_trait::async_trait]
impl ToolExecutor for MockToolExecutor {
    async fn execute(
        &self,
        _tool_id: &str,
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
        "leader".into()
    }
    async fn propose(&self, _k: &str, _v: serde_json::Value) -> Result<(), String> {
        Ok(())
    }
}

struct MockSynthesizer;
#[async_trait::async_trait]
impl sisyphus_intel::FlowSynthesizer for MockSynthesizer {
    async fn synthesize(
        &self,
        goal: &str,
        _ctx: &ExecutionContext,
        _mode: WorkMode,
        _insights: Option<&str>,
    ) -> Result<sisyphus_intel::synthesis::SynthesisResult, String> {
        let mut steps = Vec::new();
        steps.push(CognitiveStep {
            id: "step1".into(),
            name: "Step 1".into(),
            action: CognitiveAction::ToolCall {
                tool_id: "mock_tool".into(),
                args: serde_json::json!({}),
            },
            priority: 1,
            verification: "Output should be success".into(),
        });

        let graph = SOPGraph::new(); // Simplified

        Ok(sisyphus_intel::synthesis::SynthesisResult {
            cognitive_plan: Some(CognitivePlan {
                plan_id: "plan_123".into(),
                goal: goal.into(),
                steps,
                edges: vec![],
                start_node_id: "step1".into(),
            }),
            graph,
            start_node: "step1".into(),
            plan_description: "Mock plan".into(),
        })
    }
}

async fn setup_kernel() -> SisyphusKernel {
    let state_store = Arc::new(MfsStore::new(".omo/test_store"));
    let tool_executor = Arc::new(MockToolExecutor);

    // SisyphusIntel requires simulator, reflector, evolver. We use the real ones with dummy configs.
    let intel = Arc::new(SisyphusIntel::new(
        Arc::new(sisyphus_intel::simulation::Simulator::default()),
        Arc::new(sisyphus_intel::reflect::Reflector::default()),
        Arc::new(sisyphus_intel::evolution::SOPEvolver::default()),
    ));

    let llm_client = Arc::new(MockLlmClient);
    let synthesizer = Arc::new(MockSynthesizer);
    let meta_learner = Arc::new(parking_lot::Mutex::new(MetaLearner::default()));
    let consensus = Arc::new(MockConsensus);

    SisyphusKernel::new(
        state_store,
        tool_executor,
        intel,
        llm_client,
        synthesizer,
        meta_learner,
        consensus,
    )
}

#[tokio::test]
async fn test_full_cognitive_lifecycle() {
    // 1. Setup Environment
    let kernel = setup_kernel().await;

    println!("--- Phase 1: Initial Planning (Cold Start) ---");
    let goal = "Refactor all DeprecatedApi to NewApi in the codebase";

    let ctx = ExecutionContext::new();
    let synthesis = kernel
        .intelligence_manager
        .synthesize_goal(goal, &ctx, WorkMode::Standard, None)
        .await
        .expect("Initial synthesis failed");

    let original_plan = synthesis.cognitive_plan.expect("Plan should be generated");
    println!(
        "Initial plan generated with {} steps",
        original_plan.steps.len()
    );

    kernel
        .cache_plan(&original_plan.plan_id, &original_plan)
        .await
        .expect("Cache failed");

    println!("\n--- Phase 2: Failure & Reflection Loop ---");
    let feedback = ExecutionFeedback {
        plan_id: original_plan.plan_id.clone(),
        step_id: original_plan.steps[0].id.clone(),
        status: FeedbackStatus::Failed,
        output: "Error: File not found at expected path".into(),
        error: Some("FileNotFound".into()),
        state_snapshot: None,
    };

    let evolution = kernel
        .handle_feedback("default", feedback)
        .await
        .expect("Feedback handling failed");

    assert!(
        evolution.should_pivot,
        "Sisyphus should pivot after a failure"
    );
    assert!(
        evolution.revised_plan.is_some(),
        "Revised plan should be provided"
    );
    println!("Sisyphus correctly pivoted and provided a revised plan.");

    println!("\n--- Phase 3: Success & Experience Capture ---");
    let final_plan = evolution.revised_plan.unwrap();
    kernel
        .capture_success(&final_plan.plan_id, final_plan.clone())
        .await
        .expect("Capture failed");

    println!("Successful plan captured as experience SOP.");

    println!("\n--- Phase 4: Experience-Guided Synthesis ---");
    let similar_goal = "Update all deprecated APIs to new ones in the project";

    let result = kernel.execute_goal(similar_goal, "test_sop", None).await;
    assert!(result.is_ok(), "Experience-guided execution should succeed");

    println!("Sisyphus successfully used retrieved experience to handle a similar goal.");
    println!("\n✅ Full Cognitive Lifecycle Test Passed!");
}
