use std::sync::Arc;
use async_trait::async_trait;
use sisyphus_core::{SisyphusKernel};
use sisyphus_mem::{SisyphusStore};
use sisyphus_types::{SOPGraph, ToolExecutor, TypesError};
use sisyphus_tools_core::ToolError;
use sisyphus_tools_llm::{DummyLlmClient, DummyEmbeddingClient};
use sisyphus_intel::{SisyphusIntel, MonteCarloSimulator, simulation::LlmLogicalSimulator};
use sisyphus_intel::StrategicSopSynthesizer;
use sisyphus_core::Consensus;
use parking_lot::Mutex;

// --- Mocks ---

struct MockConsensus;
#[async_trait::async_trait]
impl Consensus for MockConsensus {
    async fn is_leader(&self) -> bool { true }
    async fn get_leader(&self) -> String { "node-1".into() }
    async fn propose(&self, _k: &str, _v: serde_json::Value) -> Result<(), String> { Ok(()) }
}

struct MockToolExecutor;
#[async_trait::async_trait]
impl ToolExecutor for MockToolExecutor {
    async fn execute(&self, _tool_id: &str, _input: Vec<u8>, _fuel: u64) -> Result<Vec<u8>, TypesError> {
        Ok(vec![])
    }
}

// --- Test ---

#[tokio::test]
async fn test_project_isolation() {
    // 1. Setup Stores
    let store = SisyphusStore::new("sqlite::memory:").await.expect("Store init failed");
    let state_store = store.state.clone();
    let sop_store = store.sop.clone();
    let roadmap_store = store.roadmap.clone();
    let vector_store = Arc::new(sisyphus_mem::VectorMemoryStore::new(store.pool()).await.expect("Vector store failed"));

    // 2. Setup Components
    let llm_client = Arc::new(DummyLlmClient);
    let embedding_client = Arc::new(DummyEmbeddingClient);
    let tool_executor = Arc::new(MockToolExecutor);
    
    let simulator = Arc::new(MonteCarloSimulator::new(100));
    let logical_simulator = Arc::new(LlmLogicalSimulator::new(llm_client.clone()));
    let reflector = Arc::new(sisyphus_intel::StandardReflector);
    let evolver = Arc::new(sisyphus_intel::LlmEvolver::new(
        llm_client.clone(), 
        Arc::new(sisyphus_intel::TopologicalSOPEvolver::new()), 
        tool_executor.clone()
    ));
    
    let intel = Arc::new(SisyphusIntel::new(
        simulator, 
        logical_simulator, 
        reflector, 
        evolver
    ));
    
    let synthesizer = Arc::new(StrategicSopSynthesizer::new(
        Arc::new(sisyphus_intel::GoalDrivenSynthesizer::new(tool_executor.clone())),
        Arc::new(MonteCarloSimulator::new(100)),
    ));

    let meta_learner = Arc::new(Mutex::new(sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(".omo/meta_test")));

    // Setup Experience Bank
    let exp_bank = Arc::new(sisyphus_mem::experience_bank::VectorExperienceBank::new(
        state_store.clone(), 
        vector_store.clone(), 
        embedding_client.clone()
    ).await.expect("Exp bank init failed"));

    // 3. Instantiate Kernel (Strict 13 Arguments)
    let kernel = SisyphusKernel::new(
        state_store,           // 1. state_store
        sop_store,             // 2. sop_store
        vector_store,          // 3. vector_store
        exp_bank,              // 4. experience_bank
        tool_executor,         // 5. tool_executor
        intel,                 // 6. intel
        llm_client.clone(),    // 7. llm_client
        llm_client.clone(),    // 8. fast_llm_client
        synthesizer,           // 9. synthesizer
        meta_learner,          // 10. meta_learner
        Arc::new(MockConsensus), // 11. consensus
        roadmap_store,         // 12. sql_store
        embedding_client,      // 13. embedding_client
    );

    let project_alpha = "proj_alpha";
    let project_beta = "proj_beta";
    let sop_id_alpha = "sop_alpha_1";
    let sop_id_beta = "sop_beta_1";

    // 4. Save unique SOPs for each project
    let graph_alpha = Arc::new(SOPGraph::new());
    let graph_beta = Arc::new(SOPGraph::new());

    kernel.save_sop(sop_id_alpha, project_alpha, &graph_alpha).await.expect("Save alpha failed");
    kernel.save_sop(sop_id_beta, project_beta, &graph_beta).await.expect("Save beta failed");

    // 5. CROSS-ACCESS VERIFICATION
    let load_attempt_1 = kernel.load_sop(sop_id_alpha, project_beta).await;
    assert!(load_attempt_1.is_err(), "Isolation Breach: project_beta should not be able to load project_alpha's SOP");

    let load_attempt_2 = kernel.load_sop(sop_id_beta, project_alpha).await;
    assert!(load_attempt_2.is_err(), "Isolation Breach: project_alpha should not be able to load project_beta's SOP");

    // 6. VALID ACCESS VERIFICATION
    let load_alpha = kernel.load_sop(sop_id_alpha, project_alpha).await;
    match &load_alpha {
        Ok(_) => println!("Successfully loaded own SOP!"),
        Err(e) => println!("Failed to load own SOP: {:?}", e),
    }
    assert!(load_alpha.is_ok(), "Should be able to load own SOP");

    println!("Project Isolation E2E verified successfully!");
}
