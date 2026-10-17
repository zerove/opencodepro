mod common;

use async_trait::async_trait;
use sisyphus_core::{
    CompositeGraph, CompositeOutput, DataMapping, GraphDependency, SOPGraph, SOPGraphConfig,
    SisyphusKernel, WasmSOPNode,
};
use sisyphus_types::ToolExecutor;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct CompositeMockExecutor {
    call_count: AtomicUsize,
}

#[async_trait]
impl ToolExecutor for CompositeMockExecutor {
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

async fn make_kernel() -> (Arc<SisyphusKernel>, Arc<CompositeMockExecutor>) {
    let state_store = Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .unwrap(),
    );
    let tool_executor = Arc::new(CompositeMockExecutor {
        call_count: AtomicUsize::new(0),
    });

    let simulator = Arc::new(sisyphus_intel::MonteCarloSimulator::with_state_store(
        100,
        state_store.clone(),
    ));
    let intel = Arc::new(sisyphus_intel::SisyphusIntel::new(
        simulator,
        Arc::new(common::MockLogicalSimulator),
        Arc::new(common::MockReflector),
        Arc::new(common::MockEvolver),
    ));

    let synthesizer = Arc::new(common::MockSynthesizer);
    let experience_bank = Arc::new(common::MockExperienceBank);

    let llm_client = Arc::new(sisyphus_tools_llm::metis::DummyLlmClient);
    let fast_llm_client = Arc::new(sisyphus_tools_llm::metis::DummyLlmClient);

    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    let sop_store = Arc::new(sisyphus_mem::SopSqlStore::new(pool.clone()).await.unwrap());
    let vector_store = Arc::new(sisyphus_mem::VectorMemoryStore::new(pool.clone()).await.unwrap());
    let roadmap_store = Arc::new(sisyphus_mem::RoadmapSqlStore::new(pool.clone()).await.unwrap());

    let kernel = Arc::new(SisyphusKernel::new(
        state_store,
        sop_store,
        vector_store,
        experience_bank,
        tool_executor.clone(),
        Arc::new(sisyphus_tools_executor::executor::test_cmds::TestExecutor::new()),
        intel,
        llm_client,
        fast_llm_client,
        Arc::new(sisyphus_tools_llm_tools::expert_reviewer::DefaultExpertReviewer),
        synthesizer,
        Arc::new(parking_lot::Mutex::new(
            sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(".omo/test_meta"),
        )),
        Arc::new(common::MockConsensus),
        roadmap_store,
        Arc::new(sisyphus_tools_llm::DummyEmbeddingClient),
    ));

    (kernel, tool_executor)
}

fn make_single_node_config(name: &str, tool_id: &str) -> SOPGraphConfig {
    let mut graph = SOPGraph::new();
    graph.add_node(Arc::new(WasmSOPNode {
        name: name.to_string(),
        tool_id: tool_id.to_string(),
        fuel_limit: 1_000_000,
        executor: Arc::new(CompositeMockExecutor {
            call_count: AtomicUsize::new(0),
        }),
        args: Vec::new(),
    }));
    SOPGraphConfig::from_graph(&graph).unwrap()
}

#[tokio::test]
async fn test_dependency_resolver_empty() {
    let composite = CompositeGraph::new();
    let order = sisyphus_core::DependencyResolver::resolve(&composite).unwrap();
    assert!(order.is_empty());
}

#[tokio::test]
async fn test_dependency_resolver_linear() {
    let mut composite = CompositeGraph::new();
    composite.add_graph("graph_a", make_single_node_config("a", "tool_a"));
    composite.add_graph("graph_b", make_single_node_config("b", "tool_b"));
    composite.add_dependency(GraphDependency {
        source_sop_id: "graph_a".into(),
        target_sop_id: "graph_b".into(),
        mappings: vec![DataMapping {
            source_node: "graph_a".into(),
            target_placeholder: "dep_output".into(),
        }],
    });

    let order = sisyphus_core::DependencyResolver::resolve(&composite).unwrap();
    assert_eq!(order.len(), 2);
    // graph_a must come before graph_b
    let pos_a = order.iter().position(|id| id == "graph_a").unwrap();
    let pos_b = order.iter().position(|id| id == "graph_b").unwrap();
    assert!(pos_a < pos_b, "graph_a should execute before graph_b");
}

#[tokio::test]
async fn test_dependency_resolver_cycle() {
    let mut composite = CompositeGraph::new();
    composite.add_graph("graph_a", make_single_node_config("a", "tool_a"));
    composite.add_graph("graph_b", make_single_node_config("b", "tool_b"));
    composite.add_dependency(GraphDependency {
        source_sop_id: "graph_a".into(),
        target_sop_id: "graph_b".into(),
        mappings: vec![],
    });
    composite.add_dependency(GraphDependency {
        source_sop_id: "graph_b".into(),
        target_sop_id: "graph_a".into(),
        mappings: vec![],
    });

    let result = sisyphus_core::DependencyResolver::resolve(&composite);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cycle"));
}

#[tokio::test]
async fn test_execute_composite_linear() {
    let (kernel, executor) = make_kernel().await;

    let mut composite = CompositeGraph::new();
    composite.add_graph("step1", make_single_node_config("fetch", "tool_fetch"));
    composite.add_graph("step2", make_single_node_config("process", "tool_process"));
    composite.add_dependency(GraphDependency {
        source_sop_id: "step1".into(),
        target_sop_id: "step2".into(),
        mappings: vec![],
    });

    let output: CompositeOutput =
        kernel.execute_composite(&composite, None, "test-project").await.unwrap();

    assert_eq!(output.execution_order.len(), 2);
    assert_eq!(output.execution_order[0], "step1");
    assert_eq!(output.execution_order[1], "step2");
    assert!(output.results.contains_key("step1"));
    assert!(output.results.contains_key("step2"));

    // Both graphs executed
    let call_count = executor.call_count.load(Ordering::SeqCst);
    assert_eq!(call_count, 2, "Both graphs should have executed");
}

#[tokio::test]
async fn test_execute_composite_single_graph() {
    let (kernel, _executor) = make_kernel().await;

    let mut composite = CompositeGraph::new();
    composite.add_graph("solo", make_single_node_config("only", "tool_only"));

    let output = kernel.execute_composite(&composite, None, "test-project").await.unwrap();

    assert_eq!(output.execution_order.len(), 1);
    assert!(output.results.contains_key("solo"));
}

#[tokio::test]
async fn test_execute_composite_with_predefined_order() {
    let (kernel, _executor) = make_kernel().await;

    let mut composite = CompositeGraph::new();
    composite.add_graph("beta", make_single_node_config("b", "tool_b"));
    composite.add_graph("alpha", make_single_node_config("a", "tool_a"));
    // Override order: alpha first despite being added second
    composite.execution_order = vec!["alpha".into(), "beta".into()];

    let output = kernel.execute_composite(&composite, None, "test-project").await.unwrap();

    assert_eq!(output.execution_order.len(), 2);
    assert_eq!(output.execution_order[0], "alpha");
    assert_eq!(output.execution_order[1], "beta");
}

#[tokio::test]
async fn test_execute_composite_missing_graph() {
    let kernel = make_kernel().await.0;
    let mut composite = CompositeGraph::new();
    composite.execution_order = vec!["nonexistent".into()];

    let result = kernel.execute_composite(&composite, None, "test-project").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_execute_goals_multi_goal() {
    let (kernel, _executor) = make_kernel().await;

    let output = kernel
        .execute_goals(
            &["fetch data", "process data", "store results"],
            "test-project",
        )
        .await
        .unwrap();

    assert_eq!(
        output.execution_order.len(),
        3,
        "Three goals should produce three sub-graphs"
    );
    // Each result should have a CognitiveLoop report (as a string)
    for sop_id in &output.execution_order {
        let (_result, report) = output.results.get(sop_id).unwrap();
        assert!(
            !report.is_empty(),
            "Each sub-graph should execute through cognitive loop: {}",
            sop_id
        );
    }
}

#[tokio::test]
async fn test_execute_goals_single_goal() {
    let (kernel, _executor) = make_kernel().await;

    let output = kernel.execute_goals(&["fetch only"], "test-project").await.unwrap();

    assert_eq!(output.execution_order.len(), 1);
    assert!(output.results.contains_key("goal_0"));
}

#[tokio::test]
async fn test_synthesize_composite_independent_graphs() {
    let (kernel, _executor) = make_kernel().await;
    let ctx = sisyphus_types::ExecutionContext::new();

    let goals = ["read config", "process data", "write output"];
    let goal_refs: Vec<&str> = goals.to_vec();
    let composite = sisyphus_intel::synthesize_composite(
        &goal_refs,
        &*kernel.intelligence_manager.synthesizer,
        &ctx,
        "test-project",
    )
    .await
    .unwrap();

    assert_eq!(
        composite.graphs.len(),
        3,
        "Each goal becomes a separate sub-graph"
    );
    assert!(
        composite.dependencies.is_empty(),
        "Independent goals have no cross-dependencies"
    );
}
