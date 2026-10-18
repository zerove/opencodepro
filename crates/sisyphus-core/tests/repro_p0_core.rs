//! P0 缺陷复现/修复验证 ②③
//!
//! ② `execution_order` 绕过校验（kernel/execution.rs:500-505）：
//!    [已修复] 环检测现在始终执行；本文件断言环 + 非空 order → Err。
//!    修复前的缺陷行为（保留此说明用于对照）：非空 `execution_order` 时
//!    跳过 `DependencyResolver::resolve`，循环依赖不报错、不执行任何
//!    SOP、仍返回 `Ok`（结果静默丢失）。
//!
//! ③ 子图信号量死锁（executor.rs:39/187/432/552-564）：
//!    [已修复] 子图父节点在等待子图执行前释放并发许可（两个 spawn 点），
//!    子节点可从同一信号量取得许可。原缺陷：父节点持许可（`_permit` 覆盖
//!    整个节点执行期）等待子节点，≥16 个并发子图父节点占满许可 →
//!    子节点永远拿不到许可，死锁（实测 started=16 / completed=0）。
//!    本用例断言全部 20 个子图节点在超时内完成。
//!
//! 运行：
//! ```bash
//! cargo test -p sisyphus-core --test repro_p0_core -- --nocapture
//! ```

mod common;

use async_trait::async_trait;
use sisyphus_core::{
    CompositeGraph, ExecutionEvent, Executor, GraphDependency, SOPGraph, SOPGraphConfig,
    SisyphusKernel, WasmSOPNode,
};
use sisyphus_types::{SubGraphSOPNode, ToolExecutor};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

// ===========================================================================
// 复现 ② 的测试设施（与 tests/composite_test.rs 相同的既有 harness 模式）
// ===========================================================================

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

// ===========================================================================
// 复现 ②：execution_order 绕过环检测
// ===========================================================================

#[tokio::test]
async fn repro_p0_2_execution_order_bypasses_cycle_check() {
    let (kernel, _tool) = make_kernel().await;

    // 与 sisyphus-core/src/dag/tests.rs::test_dependency_resolver_cycle 同构的环
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

    // 基线：空 execution_order → resolver 正确拒绝
    let baseline = sisyphus_core::DependencyResolver::resolve(&composite);
    assert!(
        baseline.is_err(),
        "baseline sanity: resolver must reject the cycle"
    );

    // 修复验证：非空 execution_order 也必须经过环检测（kernel/execution.rs:500-505）
    composite.execution_order = vec!["graph_a".into(), "graph_b".into()];
    let result = kernel.execute_composite(&composite, None, "test-project").await;

    match result {
        Err(e) => {
            let msg = e.to_string();
            eprintln!("FIX VERIFIED (P0 ②): cycle rejected despite execution_order -> Err({msg})");
            assert!(msg.contains("cycle"), "expected cycle error, got: {msg}");
        },
        Ok(output) => panic!(
            "FIX MISSING: cycle silently accepted; Ok returned with results={:?}",
            output.results.keys().collect::<Vec<_>>()
        ),
    }
}

// ===========================================================================
// 复现 ③：子图信号量死锁
// ===========================================================================

struct MockToolExecutor;

#[async_trait]
impl ToolExecutor for MockToolExecutor {
    async fn execute(
        &self,
        _tool_id: &str,
        _args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
        Ok(b"mock".to_vec())
    }
}

fn make_executor() -> Executor {
    Executor::new(
        Arc::new(sisyphus_tools_executor::executor::test_cmds::TestExecutor::new()),
        Arc::new(sisyphus_core::intervention_manager::InterventionManager::new()),
        Arc::new(
            sisyphus_core::internalization_manager::InternalizationManager::new(
                Arc::new(sisyphus_tools_llm::metis::DummyLlmClient),
                Arc::new(sisyphus_knowledge::AssetManager::new(
                    ".sisyphus/knowledge".into(),
                )),
            ),
        ),
        None,
    )
}

#[tokio::test]
async fn repro_p0_3_subgraph_nodes_deadlock_on_shared_semaphore() {
    let executor = make_executor();

    // 内层子图：单节点
    let mut inner = SOPGraph::new();
    inner.add_node(Arc::new(WasmSOPNode {
        name: "inner-node".to_string(),
        tool_id: "tool_inner".to_string(),
        fuel_limit: 1_000_000,
        executor: Arc::new(MockToolExecutor),
        args: Vec::new(),
    }));
    let inner_cfg = SOPGraphConfig::from_graph(&inner).expect("inner config builds");

    // 外层图：20 个互不依赖的子图节点（信号量只有 16 个许可）
    const N: usize = 20;
    let mut outer = SOPGraph::new();
    for i in 0..N {
        outer.add_node(Arc::new(SubGraphSOPNode {
            name: format!("subgraph-{i}"),
            config: inner_cfg.clone(),
        }));
    }

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let run = executor.execute_with_context(
        Arc::new(outer),
        petgraph::graph::NodeIndex::new(0),
        None,
        Some(tx),
        Arc::new(MockToolExecutor),
        None,
        None,
        None,
    );

    match tokio::time::timeout(Duration::from_secs(5), run).await {
        Ok(Ok(_ctx)) => {
            let (mut parent_started, mut parent_completed, mut inner_completed) =
                (0usize, 0usize, 0usize);
            while let Ok(ev) = rx.try_recv() {
                match ev {
                    ExecutionEvent::NodeStarted { node_name } => {
                        if node_name.starts_with("subgraph-") {
                            parent_started += 1;
                        }
                    },
                    ExecutionEvent::NodeCompleted { node_name, .. } => {
                        if node_name.starts_with("subgraph-") {
                            parent_completed += 1;
                        } else if node_name == "inner-node" {
                            inner_completed += 1;
                        }
                    },
                    _ => {},
                }
            }
            eprintln!(
                "FIX VERIFIED (P0 ③): completed within 5s — parent_started={parent_started}, \
                 parent_completed={parent_completed}, inner_completed={inner_completed}"
            );
            assert_eq!(parent_started, N, "every subgraph parent must start");
            assert_eq!(parent_completed, N, "every subgraph parent must complete");
            assert_eq!(
                inner_completed, N,
                "every subgraph's inner node must complete"
            );
        },
        Ok(Err(e)) => panic!("FIX INVALID: execution errored instead of completing: {e}"),
        Err(_elapsed) => panic!("FIX MISSING: still deadlocked — no completion within 5s"),
    }
}
