//! Shared test helpers for sisyphus-core integration tests.
//!
//! Provides mock implementations and a builder for constructing a test
//! `SisyphusKernel` without duplicating boilerplate across test files.

use async_trait::async_trait;
use sisyphus_core::{Consensus, SOPGraph, SisyphusKernel, WasmSOPNode};
use sisyphus_intel::simulation::LogicalSimulator;
use sisyphus_intel::{
    LogicalSimulationResult, MonteCarloSimulator, ReflectionReport, SimulationResult,
    SisyphusIntel,
};
use sisyphus_mem::MfsStore;
use sisyphus_tools_llm::circuit_breaker::CircuitBreaker;
use sisyphus_tools_llm::LlmClient;
use sisyphus_types::{ExecutionContext, ToolExecutor, WorkMode, experience::ExperienceTuple};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Mock Consensus
// ---------------------------------------------------------------------------

pub struct MockConsensus;

#[async_trait]
impl Consensus for MockConsensus {
    async fn is_leader(&self) -> bool {
        true
    }
    async fn get_leader(&self) -> String {
        "mock-leader".into()
    }
    async fn propose(&self, _key: &str, _value: serde_json::Value) -> Result<(), String> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Mock LlmClient (trivial)
// ---------------------------------------------------------------------------

pub struct MockLlmClient;

#[async_trait]
impl LlmClient for MockLlmClient {
    async fn generate(&self, _prompt: &str) -> Result<String, String> {
        Ok("Mock response".into())
    }
    async fn chat(&self, _sys: &str, _ctx: &str, _stream: bool) -> Result<String, String> {
        Ok("Mock chat".into())
    }
    fn get_params_for(&self, _agent_id: &str) -> (String, String, String) {
        ("test_key".into(), "http://test:8080".into(), "test-model".into())
    }
    fn breaker(&self) -> CircuitBreaker {
        CircuitBreaker::default()
    }
}

// ---------------------------------------------------------------------------
// Mock LogicalSimulator
// ---------------------------------------------------------------------------

pub struct MockLogicalSimulator;

#[async_trait]
impl LogicalSimulator for MockLogicalSimulator {
    async fn simulate_logic(
        &self,
        _graph: Arc<SOPGraph>,
        _context: &ExecutionContext,
        _sop_id: &str,
        _goal_text: Option<&str>,
    ) -> Result<LogicalSimulationResult, String> {
        Ok(LogicalSimulationResult {
            is_valid: true,
            flaws: vec![],
            predicted_path: vec!["mock".to_string()],
            confidence: 1.0,
        })
    }
}

// ---------------------------------------------------------------------------
// Mock Reflector
// ---------------------------------------------------------------------------

pub struct MockReflector;

#[async_trait]
impl sisyphus_intel::reflect::Reflector for MockReflector {
    async fn reflect(
        &self,
        _trace: &str,
        _predicted: &SimulationResult,
        _experiences: Option<&[ExperienceTuple]>,
    ) -> Result<ReflectionReport, String> {
        Ok(ReflectionReport {
            original_sop_id: "mock".into(),
            deviation_score: 0.0,
            suggested_improvements: vec![],
            evolved_sop_structure: None,
            records: vec![],
        })
    }
}

// ---------------------------------------------------------------------------
// Mock SOPEvolver
// ---------------------------------------------------------------------------

pub struct MockEvolver;

#[async_trait]
impl sisyphus_intel::reflect::SOPEvolver for MockEvolver {
    async fn evolve(
        &self,
        graph: Arc<SOPGraph>,
        _report: &ReflectionReport,
        _prediction: &SimulationResult,
        _insights: Option<&str>,
    ) -> Result<Arc<SOPGraph>, String> {
        Ok(graph)
    }
}

// ---------------------------------------------------------------------------
// Mock ExperienceBank
// ---------------------------------------------------------------------------

pub struct MockExperienceBank;

#[async_trait]
impl sisyphus_types::experience::ExperienceBank for MockExperienceBank {
    async fn record(&self, _exp: ExperienceTuple) -> Result<(), String> {
        Ok(())
    }
    async fn query_similar(
        &self,
        _query: sisyphus_types::experience::ExperienceQuery,
    ) -> Result<Vec<ExperienceTuple>, String> {
        Ok(vec![])
    }
}

// ---------------------------------------------------------------------------
// Mock FlowSynthesizer — returns a single-node graph
// ---------------------------------------------------------------------------

pub struct MockSynthesizer;

#[async_trait]
impl sisyphus_intel::FlowSynthesizer for MockSynthesizer {
    async fn synthesize(
        &self,
        _goal: &str,
        _context: &ExecutionContext,
        _project_id: &str,
        _mode: WorkMode,
        _insights: Option<&str>,
    ) -> Result<sisyphus_intel::SynthesisResult, String> {
        let mut graph = SOPGraph::new();
        let start_node = graph.add_node(Arc::new(WasmSOPNode {
            name: "mock_node".into(),
            tool_id: "mock_tool".into(),
            fuel_limit: 1_000_000,
            executor: Arc::new(MockExecutor::new()),
        }));
        Ok(sisyphus_intel::SynthesisResult {
            graph: Arc::new(graph),
            start_node,
            plan_description: "mock plan".into(),
        })
    }
}

// ---------------------------------------------------------------------------
// Mock ToolExecutor (counting)
// ---------------------------------------------------------------------------

pub struct MockExecutor {
    pub call_count: AtomicUsize,
}

impl MockExecutor {
    pub fn new() -> Self {
        Self {
            call_count: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl ToolExecutor for MockExecutor {
    async fn execute(
        &self,
        tool_id: &str,
        _args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        Ok(format!("executed:{}", tool_id).into_bytes())
    }
}

/// Create a test SisyphusKernel with all-mock dependencies.
pub async fn make_test_kernel() -> (Arc<SisyphusKernel>, Arc<MockExecutor>) {
    let state_store = Arc::new(MfsStore::new("sqlite::memory:").await.unwrap());
    let executor = Arc::new(MockExecutor::new());

    let simulator = Arc::new(MonteCarloSimulator::with_state_store(100, state_store.clone()));
    let intel = Arc::new(SisyphusIntel::new(
        simulator,
        Arc::new(MockLogicalSimulator),
        Arc::new(MockReflector),
        Arc::new(MockEvolver),
    ));

    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .unwrap();

    let kernel = Arc::new(SisyphusKernel::new(
        state_store,
        Arc::new(sisyphus_mem::SopSqlStore::new(pool.clone()).await.unwrap()),
        Arc::new(sisyphus_mem::VectorMemoryStore::new(pool.clone()).await.unwrap()),
        Arc::new(MockExperienceBank),
        executor.clone() as Arc<dyn ToolExecutor>,
        intel,
        Arc::new(MockLlmClient) as Arc<dyn LlmClient>,
        Arc::new(MockLlmClient) as Arc<dyn LlmClient>,
        Arc::new(MockSynthesizer) as Arc<dyn sisyphus_intel::FlowSynthesizer>,
        Arc::new(parking_lot::Mutex::new(
            sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(".omo/test_meta"),
        )),
        Arc::new(MockConsensus),
        Arc::new(sisyphus_mem::RoadmapSqlStore::new(pool).await.unwrap()),
        Arc::new(sisyphus_tools_llm::DummyEmbeddingClient),
    ));

    (kernel, executor)
}
