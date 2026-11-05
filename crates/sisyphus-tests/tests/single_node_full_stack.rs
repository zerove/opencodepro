//! 单机全功能端到端集成测试（single-node full stack）— Phase 1 of the
//! single-node full-functionality follow-up plan.
//!
//! 目标：在**单进程、无集群**条件下，驱动 `SisyphusKernel` 跑完「目标合成 →
//! 认知循环执行 → 经验沉淀/演进」全主链路，证明单机模式下核心全功能不依赖
//! 集群协同层而退化。
//!
//! 与 cluster 侧 `chaos_*` / `anti_entropy_*` / `cluster_integration` 用例不同，
//! 本用例不起 gRPC server、不建 peer；它直接用真实 `SisyphusKernel` + 真实内存
//! store + mock LLM/tool/intel 跑一次完整的单机认知循环。这正是生产路径上
//! 「bridge 接 core、core 接 memory/intel/tools」在单机装配下的语义。

use async_trait::async_trait;
use sisyphus_core::{Consensus, SOPGraph, SisyphusKernel};
use sisyphus_intel::{
    FlowSynthesizer, MonteCarloSimulator, SisyphusIntel, StandardEvolver, StandardReflector,
};
use sisyphus_mem::MfsStore;
use sisyphus_tools_llm::circuit_breaker::CircuitBreaker;
use sisyphus_tools_llm::{DummyEmbeddingClient, LlmClient};
use sisyphus_tools_llm_tools::expert_reviewer::DefaultExpertReviewer;
use sisyphus_tools_llm_tools::meta_learner::MetaLearner;
use sisyphus_types::experience::{
    ExperienceBank, ExperienceDigest, ExperienceQuery, ExperienceTuple,
};
use sisyphus_types::{ExecutionContext, ToolExecutor, WorkMode};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Mock：LLM
// ---------------------------------------------------------------------------

struct MockLlmClient;

#[async_trait]
impl LlmClient for MockLlmClient {
    async fn generate(&self, prompt: &str) -> Result<String, sisyphus_types::TypesError> {
        // 认知循环内 critic（architecture critic）要求返回结构化 JSON。
        if prompt.contains("rigorous AI Architecture Critic") {
            return Ok(
                r#"{"critical_findings":[],"risk_score":0.0,"suggested_refinement":"No critical findings; proceed.","should_proceed":true}"#.to_string(),
            );
        }
        Ok(format!("Mock response to: {}", prompt))
    }
    async fn chat(
        &self,
        _system: &str,
        user: &str,
        _stream: bool,
    ) -> Result<String, sisyphus_types::TypesError> {
        // Swarm audit：审计员一审即通过，跳过 refine 循环。
        if user.contains("review this proposal") {
            return Ok(r#"{"approved":true,"feedback":"Mock: proposal approved"}"#.to_string());
        }
        // Swarm audit：架构师给出合法的单节点 SOP 提案 JSON。
        if user.contains("propose an optimized SOPGraphConfig") {
            return Ok(
                r#"{"nodes":[{"name":"mock_start","kind":{"Wasm":{"tool_id":"mock","fuel_limit":1000000,"args":[]}}}],"edges":[]}"#.to_string(),
            );
        }
        Ok("Mock chat response".into())
    }
    fn get_params_for(&self, _agent_id: &str) -> (String, String, String) {
        (
            "test_key".into(),
            "http://test:8080".into(),
            "test-model".into(),
        )
    }
    fn breaker(&self) -> CircuitBreaker {
        CircuitBreaker::default()
    }
}

// ---------------------------------------------------------------------------
// Mock：工具执行
// ---------------------------------------------------------------------------

struct MockToolExecutor;

#[async_trait]
impl ToolExecutor for MockToolExecutor {
    async fn execute(
        &self,
        _tool_id: &str,
        _args: Vec<u8>,
        _fuel: u64,
    ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
        Ok(b"mock".to_vec())
    }
}

// ---------------------------------------------------------------------------
// Mock：合成
// ---------------------------------------------------------------------------

struct MockSynthesizer;

#[async_trait]
impl FlowSynthesizer for MockSynthesizer {
    async fn synthesize(
        &self,
        goal: &str,
        _ctx: &ExecutionContext,
        _project_id: &str,
        _mode: WorkMode,
        _insights: Option<&str>,
    ) -> Result<sisyphus_intel::SynthesisResult, sisyphus_intel::IntelError> {
        // 构造一条真实可执行的 DAG：start -> mock node，保证 start node 可定位。
        let mut graph = SOPGraph::new();
        let start_node = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
            name: "mock_start".into(),
            tool_id: "mock".into(),
            fuel_limit: 1_000_000,
            executor: Arc::new(MockToolExecutor) as Arc<dyn ToolExecutor>,
            args: b"{}".to_vec(),
        }));
        let _second_node = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
            name: "mock_step".into(),
            tool_id: "mock".into(),
            fuel_limit: 1_000_000,
            executor: Arc::new(MockToolExecutor) as Arc<dyn ToolExecutor>,
            args: b"{}".to_vec(),
        }));
        graph.add_edge(start_node, _second_node);

        Ok(sisyphus_intel::SynthesisResult {
            graph: Arc::new(graph),
            start_node,
            plan_description: format!("Mock plan for: {}", goal),
        })
    }
}

// ---------------------------------------------------------------------------
// Mock：共识
// ---------------------------------------------------------------------------

struct MockConsensus;

#[async_trait]
impl Consensus for MockConsensus {
    async fn is_leader(&self) -> bool {
        true
    }
    async fn get_leader(&self) -> String {
        "mock_leader".into()
    }
    async fn propose(
        &self,
        _key: &str,
        _value: serde_json::Value,
    ) -> Result<(), sisyphus_types::TypesError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Mock：经验库
// ---------------------------------------------------------------------------

struct MockExperienceBank;

#[async_trait]
impl ExperienceBank for MockExperienceBank {
    async fn record(&self, _exp: ExperienceTuple) -> Result<(), sisyphus_types::TypesError> {
        Ok(())
    }
    async fn query_similar(
        &self,
        _query: ExperienceQuery,
    ) -> Result<Vec<ExperienceTuple>, sisyphus_types::TypesError> {
        Ok(Vec::new())
    }
    async fn export_version_vector(&self) -> Result<ExperienceDigest, sisyphus_types::TypesError> {
        Ok(Default::default())
    }
}

// ---------------------------------------------------------------------------
// Mock：逻辑模拟
// ---------------------------------------------------------------------------

struct MockLogicalSimulator;

#[async_trait]
impl sisyphus_intel::simulation::LogicalSimulator for MockLogicalSimulator {
    async fn simulate_logic(
        &self,
        _graph: Arc<SOPGraph>,
        _context: &ExecutionContext,
        _sop_id: &str,
        _goal_text: Option<&str>,
    ) -> Result<sisyphus_intel::simulation::LogicalSimulationResult, sisyphus_intel::IntelError>
    {
        Ok(sisyphus_intel::simulation::LogicalSimulationResult {
            is_valid: true,
            flaws: Vec::new(),
            predicted_path: Vec::new(),
            confidence: 0.5,
        })
    }
}

// ---------------------------------------------------------------------------
// Kernel 装配（单机：全依赖来自本地 store / mock）
// ---------------------------------------------------------------------------

async fn setup_kernel() -> SisyphusKernel {
    let state_store = Arc::new(
        MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default()).await.unwrap(),
    );
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    let tool_executor = Arc::new(MockToolExecutor);

    let simulator = Arc::new(MonteCarloSimulator::with_state_store(
        100,
        state_store.clone(),
    ));
    let intel = Arc::new(SisyphusIntel::new(
        simulator,
        Arc::new(MockLogicalSimulator),
        Arc::new(StandardReflector),
        Arc::new(StandardEvolver::new()),
    ));

    let llm_client = Arc::new(MockLlmClient);
    let synthesizer = Arc::new(MockSynthesizer);
    let meta_learner = Arc::new(parking_lot::Mutex::new(MetaLearner::new(".omo/test_meta")));

    SisyphusKernel::new(
        state_store,
        Arc::new(sisyphus_mem::SopSqlStore::new(pool.clone()).await.unwrap()),
        Arc::new(sisyphus_mem::VectorMemoryStore::new(pool.clone()).await.unwrap()),
        Arc::new(MockExperienceBank),
        tool_executor as Arc<dyn ToolExecutor>,
        Arc::new(sisyphus_tools_executor::executor::test_cmds::TestExecutor::new()),
        intel,
        llm_client.clone() as Arc<dyn sisyphus_types::LlmClient>,
        llm_client as Arc<dyn sisyphus_types::LlmClient>,
        Arc::new(DefaultExpertReviewer),
        synthesizer as Arc<dyn sisyphus_intel::FlowSynthesizer>,
        meta_learner,
        Arc::new(MockConsensus),
        Arc::new(sisyphus_mem::RoadmapSqlStore::new(pool).await.unwrap()),
        Arc::new(DummyEmbeddingClient),
    )
}

// ---------------------------------------------------------------------------
// 用例
// ---------------------------------------------------------------------------

#[tokio::test]
async fn single_node_full_stack_end_to_end() {
    let kernel = setup_kernel().await;

    // 1. 目标合成
    let ctx = ExecutionContext::new();
    let synthesis = kernel
        .intelligence_manager
        .synthesize_goal(
            "Refactor all DeprecatedApi to NewApi in the codebase",
            &ctx,
            "default",
            WorkMode::Standard,
            None,
        )
        .await
        .expect("initial synthesis should succeed");

    let plan_desc = synthesis.plan_description.clone();
    assert!(
        !plan_desc.is_empty(),
        "plan description should not be empty"
    );

    // 2. 认知循环执行
    let result = kernel
        .execute_with_cognition(
            synthesis.graph,
            "test_sop",
            Some("default"),
            None,
            None,
            None,
        )
        .await;

    let (output, report, _evolved) = result.expect("cognitive loop should complete");
    println!(
        "single-node full stack: loop finished, iterations={}, converged={}",
        report.iterations, report.converged
    );
    assert!(!output.to_string().is_empty());

    // 3. 经验驱动的再次执行（experience-guided）
    let result = kernel
        .execute_goal(
            "Update all deprecated APIs to new ones in the project",
            "test_sop",
            "default",
            None,
            None,
        )
        .await;
    let (_output, _report) = result.expect("experience-guided execution should complete");
}
