//! P1 缺陷复现/修复验证 ⑥⑦
//!
//! ⑥ composite 连坐中止与 SOP 并发上限（kernel/execution.rs）：
//!    [已修复 a] 单个 SOP 失败不再 `?` 提前返回（旧行为：JoinSet 被 drop，
//!    所有在跑的兄弟 SOP 随之中止，无报告）；现在记录失败并标记其下游为
//!    blocked，兄弟跑完后聚合为 `CoreError::CompositeExecutionFailed`。
//!    [已修复 b] 复合执行增加 SOP 级并发门控（MAX_CONCURRENT_SOPS=8）；
//!    旧行为：每个 SOP 独立创建 Executor（各 16 许可），N 个 SOP 放大为
//!    16N 并发节点与并行 LLM 管线。
//!
//! ⑦ 分支索引越界放行全部分支（executor.rs:393-403）：
//!    [已修复] 分支决策必须解析为非负整数且匹配真实出边目标；否则返回
//!    `CoreError::NodeExecutionFailed`。旧行为：任意数值静默转为
//!    NodeIndex，匹配不到出边时递减全部出边且不放行任何节点（静默停摆）。
//!
//! 运行：
//! ```bash
//! cargo test -p sisyphus-core --test repro_p1_core -- --nocapture
//! ```

mod common;

use async_trait::async_trait;
use sisyphus_core::{
    CompositeGraph, CoreError, ExecutionEvent, Executor, GraphDependency, SOPGraph, SOPGraphConfig,
    SisyphusKernel, WasmSOPNode,
};
use sisyphus_types::{BranchSOPNode, ToolExecutor, TypesError};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

// ===========================================================================
// 复现 ⑥ 的测试设施：按 tool_id 分化行为的执行器
// ===========================================================================

struct DirectiveExecutor {
    calls: parking_lot::Mutex<HashMap<String, usize>>,
    in_flight: AtomicUsize,
    peak_in_flight: AtomicUsize,
}

impl DirectiveExecutor {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: parking_lot::Mutex::new(HashMap::new()),
            in_flight: AtomicUsize::new(0),
            peak_in_flight: AtomicUsize::new(0),
        })
    }

    fn calls(&self, tool_id: &str) -> usize {
        self.calls.lock().get(tool_id).copied().unwrap_or(0)
    }

    fn enter(&self) {
        let current = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak_in_flight.fetch_max(current, Ordering::SeqCst);
    }

    fn leave(&self) {
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

#[async_trait]
impl ToolExecutor for DirectiveExecutor {
    async fn execute(
        &self,
        tool_id: &str,
        _args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, TypesError> {
        *self.calls.lock().entry(tool_id.to_string()).or_insert(0) += 1;
        match tool_id {
            "tool_fail" => Err(TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: "intentional composite failure".into(),
            }),
            // 1000ms：远大于失败 SOP 的完成耗时，保证旧语义（连坐中止）
            // 下 slow-node 必然仍在运行时被取消。
            "tool_slow" => {
                self.enter();
                tokio::time::sleep(Duration::from_millis(1000)).await;
                self.leave();
                Ok(b"slow-ok".to_vec())
            },
            // 800ms：保证 8 个门内 SOP 的工具调用窗口充分重叠，
            // 无门控时 12 个调用必然同时在场（峰值可观测）。
            "tool_gate" => {
                self.enter();
                tokio::time::sleep(Duration::from_millis(800)).await;
                self.leave();
                Ok(b"gate-ok".to_vec())
            },
            other => Ok(format!("ok:{other}").into_bytes()),
        }
    }
}

async fn make_kernel() -> (Arc<SisyphusKernel>, Arc<DirectiveExecutor>) {
    let state_store = Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .unwrap(),
    );
    let tool_executor = DirectiveExecutor::new();

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
        executor: Arc::new(AlwaysOk),
        args: Vec::new(),
    }));
    SOPGraphConfig::from_graph(&graph).unwrap()
}

// ===========================================================================
// 复现 ⑥a：单 SOP 失败连坐中止兄弟
// ===========================================================================

#[tokio::test]
async fn repro_p1_6a_failed_sop_does_not_abort_siblings() {
    let (kernel, tool) = make_kernel().await;

    let mut composite = CompositeGraph::new();
    composite.add_graph("g_fail", make_single_node_config("fail-node", "tool_fail"));
    composite.add_graph("g_slow", make_single_node_config("slow-node", "tool_slow"));
    composite.add_graph(
        "g_blocked",
        make_single_node_config("blocked-node", "tool_never"),
    );
    composite.add_dependency(GraphDependency {
        source_sop_id: "g_fail".into(),
        target_sop_id: "g_blocked".into(),
        mappings: vec![],
    });

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let result = kernel.execute_composite(&composite, Some(tx), "test-project").await;

    let err = result.err().expect("一个 SOP 失败必须在兄弟跑完后聚合为 Err");
    match err {
        CoreError::CompositeExecutionFailed {
            failed,
            blocked,
            succeeded,
            details,
        } => {
            assert_eq!(failed, 1, "恰好 g_fail 失败");
            assert_eq!(blocked, 1, "g_blocked 因上游失败被阻塞");
            assert_eq!(succeeded, 1, "g_slow 必须在失败后仍跑完");
            assert!(
                details.contains("g_fail"),
                "details 必须命名失败 SOP；实际：{details}"
            );
        },
        other => panic!("期望聚合失败错误，实际：{other:?}"),
    }

    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    let completed: Vec<String> = events
        .iter()
        .filter_map(|ev| match ev {
            ExecutionEvent::NodeCompleted { node_name, .. } => Some(node_name.clone()),
            _ => None,
        })
        .collect();
    assert!(
        completed.iter().any(|n| n == "slow-node"),
        "在跑的兄弟 SOP 不得被连坐中止；已完成节点：{completed:?}"
    );
    assert!(
        !events.iter().any(|ev| matches!(
            ev,
            ExecutionEvent::NodeStarted { node_name } if node_name == "blocked-node"
        )),
        "依赖失败 SOP 的下游不得启动"
    );
    assert_eq!(tool.calls("tool_never"), 0, "被阻塞的下游工具不得被调用");
}

// ===========================================================================
// 复现 ⑥b：SOP 级并发无上限（16N 放大）
// ===========================================================================

#[tokio::test]
async fn repro_p1_6b_sop_concurrency_is_capped() {
    const N: usize = 12;
    let (kernel, tool) = make_kernel().await;

    let mut composite = CompositeGraph::new();
    for i in 0..N {
        composite.add_graph(
            &format!("g{i}"),
            make_single_node_config(&format!("gate-node-{i}"), "tool_gate"),
        );
    }

    let output = kernel
        .execute_composite(&composite, None, "test-project")
        .await
        .expect("门控不得导致死锁或失败");
    assert_eq!(output.results.len(), N, "全部独立 SOP 必须完成");

    let peak = tool.peak_in_flight.load(Ordering::SeqCst);
    eprintln!("SOP 门控峰值 in-flight 工具调用：{peak}（上限 8，任务总数 {N}）");
    assert!(peak <= 8, "SOP 并发门控被打破：峰值 {peak} > 8");
    assert!(peak >= 2, "sanity：独立 SOP 应真实并行（峰值 {peak}）");
    assert_eq!(tool.calls("tool_gate"), N);
}

// ===========================================================================
// 复现 ⑦：分支决策越界静默停摆
// ===========================================================================

struct AlwaysOk;

#[async_trait]
impl ToolExecutor for AlwaysOk {
    async fn execute(
        &self,
        tool_id: &str,
        _args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, TypesError> {
        Ok(format!("ok:{tool_id}").into_bytes())
    }
}

fn ok_node(name: &str) -> Arc<WasmSOPNode> {
    Arc::new(WasmSOPNode {
        name: name.into(),
        tool_id: "fixture".into(),
        fuel_limit: 100,
        executor: Arc::new(AlwaysOk),
        args: Vec::new(),
    })
}

fn core_executor(dir: &tempfile::TempDir) -> Executor {
    Executor::new(
        Arc::new(sisyphus_tools_executor::executor::test_cmds::TestExecutor::new()),
        Arc::new(sisyphus_core::intervention_manager::InterventionManager::new()),
        Arc::new(
            sisyphus_core::internalization_manager::InternalizationManager::new(
                Arc::new(sisyphus_tools_llm::DummyLlmClient),
                Arc::new(sisyphus_knowledge::AssetManager::new(
                    dir.path().join("knowledge"),
                )),
            ),
        ),
        None,
    )
}

#[tokio::test]
async fn repro_p1_7_out_of_range_branch_decision_is_rejected() {
    let dir = tempfile::tempdir().expect("临时目录");
    let mut graph = SOPGraph::new();
    let start = graph.add_node(ok_node("start"));
    let branch = graph.add_node(Arc::new(BranchSOPNode {
        name: "branch".into(),
        branches: HashMap::from([("no-such-key".to_string(), 99)]),
        default_next: 99,
    }));
    let t1 = graph.add_node(ok_node("t1"));
    let t2 = graph.add_node(ok_node("t2"));
    graph.add_edge(start, branch);
    graph.add_edge(branch, t1);
    graph.add_edge(branch, t2);

    let result = core_executor(&dir).execute(Arc::new(graph), start, Arc::new(AlwaysOk)).await;

    match result {
        Err(CoreError::NodeExecutionFailed { node_name, source }) => {
            assert_eq!(node_name, "branch");
            match *source {
                TypesError::NodeExecutionError(msg) => {
                    assert!(
                        msg.contains("does not match any outgoing edge"),
                        "实际错误：{msg}"
                    );
                },
                other => panic!("期望 NodeExecutionError，实际：{other:?}"),
            }
        },
        Ok(_) => panic!("FIX MISSING: 越界分支决策被静默接受（旧行为：全部出边被跳过且返回 Ok）"),
        Err(other) => panic!("期望 NodeExecutionFailed，实际：{other:?}"),
    }
}
