//! 其余 lib 级测试（非 DAG）。

use super::*;
use sisyphus_types::{NodeKind, NodeOutput, SOPGraphConfig, SOPNodeConfig};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// 共享 mock
// ---------------------------------------------------------------------------

struct MockToolExecutor;
#[async_trait::async_trait]
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

/// 总是执行失败的工具执行器。
///
/// 用途：让**真实** `StandardReflector` 有真实失败可观察。
/// `MockToolExecutor` 永远返回 `Ok("mock")`，trace 里不会出现错误，
/// 因此用真实反思器验证 pivot 链路时必须换成会失败的那个。
///
/// 注意：触发偏差的不是 `reflect.rs:730-736` 那个「trace 含 Error 就 0.8」
/// 的兜底（实测该分支不生效，见 `test_all_pivoted_with_real_reflector_is_error`
/// 的注释），而是同一个 `else` 臂里的 **0.1**。
struct FailingToolExecutor;

#[async_trait::async_trait]
impl sisyphus_types::ToolExecutor for FailingToolExecutor {
    async fn execute(
        &self,
        _tool_id: &str,
        _args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
        Err(sisyphus_types::TypesError::NodeExecutionError(
            "mock tool failure for real-reflector pivot test".into(),
        ))
    }
}

struct MockTestExecutor;
#[async_trait::async_trait]
impl sisyphus_types::TestExecutor for MockTestExecutor {
    async fn run_test(
        &self,
        _command: &str,
        _workdir: Option<&str>,
    ) -> Result<sisyphus_types::TestResult, sisyphus_types::TypesError> {
        Ok(sisyphus_types::TestResult {
            success: true,
            stdout: "Mock Success".into(),
            stderr: "".into(),
            exit_code: 0,
            duration_ms: 10,
            error_summary: None,
        })
    }
}

struct MockConsensus;
#[async_trait::async_trait]
impl Consensus for MockConsensus {
    async fn is_leader(&self) -> bool {
        true
    }
    async fn get_leader(&self) -> String {
        "mock-leader".into()
    }
    async fn propose(
        &self,
        _key: &str,
        _value: serde_json::Value,
    ) -> Result<(), sisyphus_types::TypesError> {
        Ok(())
    }
}

struct MockLogicalSimulator;
#[async_trait::async_trait]
impl sisyphus_intel::simulation::LogicalSimulator for MockLogicalSimulator {
    async fn simulate_logic(
        &self,
        _graph: Arc<SOPGraph>,
        _context: &ExecutionContext,
        _sop_id: &str,
        _goal_text: Option<&str>,
    ) -> Result<sisyphus_intel::simulation::LogicalSimulationResult, sisyphus_types::IntelError>
    {
        Ok(sisyphus_intel::simulation::LogicalSimulationResult {
            is_valid: true,
            flaws: vec![],
            predicted_path: vec!["mock".to_string()],
            confidence: 1.0,
        })
    }
}

struct MockReflector;
#[async_trait::async_trait]
impl sisyphus_intel::reflect::Reflector for MockReflector {
    async fn reflect(
        &self,
        _trace: &str,
        _predicted: &sisyphus_intel::SimulationResult,
        _experiences: Option<&[sisyphus_types::experience::ExperienceTuple]>,
    ) -> Result<sisyphus_intel::reflect::ReflectionReport, sisyphus_types::IntelError> {
        Ok(sisyphus_intel::reflect::ReflectionReport {
            original_sop_id: "mock".into(),
            deviation_score: 0.0,
            suggested_improvements: vec![],
            improvement_signals: vec![],
            evolved_sop_structure: None,
            records: vec![],
        })
    }
}

struct MockEvolver;
#[async_trait::async_trait]
impl sisyphus_intel::reflect::SOPEvolver for MockEvolver {
    async fn evolve(
        &self,
        graph: Arc<SOPGraph>,
        _report: &sisyphus_intel::reflect::ReflectionReport,
        _prediction: &sisyphus_intel::SimulationResult,
        _insights: Option<&str>,
    ) -> Result<Arc<SOPGraph>, sisyphus_types::IntelError> {
        Ok(graph)
    }
}

struct MockExperienceBank;
#[async_trait::async_trait]
impl sisyphus_types::experience::ExperienceBank for MockExperienceBank {
    async fn record(
        &self,
        _exp: sisyphus_types::experience::ExperienceTuple,
    ) -> Result<(), sisyphus_types::TypesError> {
        Ok(())
    }
    async fn query_similar(
        &self,
        _query: sisyphus_types::experience::ExperienceQuery,
    ) -> Result<Vec<sisyphus_types::experience::ExperienceTuple>, sisyphus_types::TypesError> {
        Ok(vec![])
    }
}

/// 使用 mock 依赖构建测试用 `Executor`。
fn build_executor() -> Executor {
    let llm: Arc<dyn sisyphus_tools_llm::LlmClient> = Arc::new(sisyphus_tools_llm::DummyLlmClient);
    let knowledge = Arc::new(sisyphus_knowledge::AssetManager::new(
        std::path::PathBuf::from(".omo/test_knowledge"),
    ));
    Executor::new(
        Arc::new(MockTestExecutor),
        Arc::new(crate::intervention_manager::InterventionManager::new()),
        Arc::new(crate::internalization_manager::InternalizationManager::new(
            llm, knowledge,
        )),
        None,
    )
}

fn make_sop_graph(config: &SOPGraphConfig) -> Arc<SOPGraph> {
    SOPGraph::from_config(
        config,
        Arc::new(MockToolExecutor),
        Arc::new(MockTestExecutor),
    )
    .expect("graph from config")
}

// ---------------------------------------------------------------------------
// CoreError 显示测试
// ---------------------------------------------------------------------------

#[test]
fn test_core_error_display() {
    assert_eq!(
        CoreError::CycleDetected.to_string(),
        "SOPGraph contains cycles"
    );
    assert_eq!(
        CoreError::DependencyCycleDetected.to_string(),
        "Cross-graph dependency cycle detected"
    );
    assert_eq!(
        CoreError::MissingSourceNode.to_string(),
        "Missing source node"
    );
    assert_eq!(
        CoreError::MissingTargetNode.to_string(),
        "Missing target node"
    );
    assert_eq!(
        CoreError::MfsSaveError("fail".into()).to_string(),
        "MFS save error: fail"
    );
    assert_eq!(
        CoreError::MfsLoadError("fail".into()).to_string(),
        "MFS load error: fail"
    );
    assert_eq!(
        CoreError::SopNotFound("sop1".into()).to_string(),
        "SOP sop1 not found"
    );
    assert_eq!(
        CoreError::NoResultProduced.to_string(),
        "Execution failed to produce result"
    );
    assert_eq!(
        CoreError::GraphNotFound("g1".into()).to_string(),
        "Graph g1 not found in composite"
    );
    assert_eq!(
        CoreError::GraphEmpty("g1".into()).to_string(),
        "Graph g1 has no nodes"
    );
    assert_eq!(
        CoreError::Internal("err".into()).to_string(),
        "Internal error: err"
    );
}

#[test]
fn test_core_error_to_string() {
    let err = CoreError::CycleDetected;
    assert_eq!(err.to_string(), "SOPGraph contains cycles");
}

// ---------------------------------------------------------------------------
// Executor 测试
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_executor_execute_cycle() {
    let executor = build_executor();

    // 创建含环的配置：Node 0 -> Node 1 -> Node 0
    let config = SOPGraphConfig {
        nodes: vec![
            SOPNodeConfig {
                name: "n0".into(),
                kind: NodeKind::Wasm {
                    tool_id: "tool0".into(),
                    fuel_limit: 100,
                },
            },
            SOPNodeConfig {
                name: "n1".into(),
                kind: NodeKind::Wasm {
                    tool_id: "tool1".into(),
                    fuel_limit: 100,
                },
            },
        ],
        edges: vec![(0, 1), (1, 0)],
    };

    let graph = make_sop_graph(&config);
    let res = executor
        .execute(
            graph,
            petgraph::graph::NodeIndex::new(0),
            Arc::new(MockToolExecutor),
        )
        .await;

    assert!(matches!(res, Err(CoreError::CycleDetected)));
}

#[test]
fn test_struct_creation() {
    // CognitiveIteration（认知迭代）
    let iter = CognitiveIteration {
        iteration: 1,
        prediction: sisyphus_intel::SimulationResult {
            path_id: "p1".into(),
            success_probability: 0.8,
            estimated_cost: 100u64,
            predicted_stability: 1.0,
            predicted_effort: 50,
            predicted_outcome: "ok".into(),
        },
        deviation_score: 0.1,
        suggestions: vec!["s1".into()],
        evolved: true,
    };
    assert_eq!(iter.iteration, 1);
    assert_eq!(iter.deviation_score, 0.1);

    // CognitiveReport（认知报告）
    let report = CognitiveReport {
        iterations: 1,
        converged: true,
        final_deviation: 0.1,
        history: vec![iter],
        traces: vec![],
    };
    assert_eq!(report.iterations, 1);
    assert!(report.converged);

    // CompositeOutput（复合输出）
    let mut results = HashMap::new();
    results.insert(
        "sop1".into(),
        (
            NodeOutput::from(serde_json::Value::String("out".into())),
            "report".into(),
        ),
    );
    let output = CompositeOutput {
        results,
        execution_order: vec!["sop1".into()],
    };
    assert_eq!(output.execution_order.len(), 1);
    assert!(output.results.contains_key("sop1"));
}

#[tokio::test]
async fn test_executor_execute_with_context() {
    let executor = build_executor();

    // 简单 DAG：n0 -> n1
    let config = SOPGraphConfig {
        nodes: vec![
            SOPNodeConfig {
                name: "n0".into(),
                kind: NodeKind::Wasm {
                    tool_id: "tool0".into(),
                    fuel_limit: 100,
                },
            },
            SOPNodeConfig {
                name: "n1".into(),
                kind: NodeKind::Wasm {
                    tool_id: "tool1".into(),
                    fuel_limit: 100,
                },
            },
        ],
        edges: vec![(0, 1)],
    };

    let graph = make_sop_graph(&config);
    let (tx, mut rx) = mpsc::unbounded_channel();

    let res = executor
        .execute_with_context(
            graph,
            petgraph::graph::NodeIndex::new(0),
            None,
            Some(tx),
            Arc::new(MockToolExecutor),
            None,
            None,
            None,
        )
        .await;

    assert!(res.is_ok());
    let ctx = res.unwrap();

    // 验证结果已存储
    assert!(ctx.get_result("n0").is_some());
    assert!(ctx.get_result("n1").is_some());

    // 验证事件已发送
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }

    // 预期两个节点都产生 NodeStarted 与 NodeCompleted
    assert!(events
        .iter()
        .any(|e| matches!(e, ExecutionEvent::NodeStarted { node_name } if node_name == "n0")));
    assert!(events.iter().any(
        |e| matches!(e, ExecutionEvent::NodeCompleted { node_name, .. } if node_name == "n0")
    ));
    assert!(events
        .iter()
        .any(|e| matches!(e, ExecutionEvent::NodeStarted { node_name } if node_name == "n1")));
    assert!(events.iter().any(
        |e| matches!(e, ExecutionEvent::NodeCompleted { node_name, .. } if node_name == "n1")
    ));
}

#[tokio::test]
async fn test_executor_diamond_dag() {
    let executor = build_executor();

    // 菱形：n0 -> [n1, n2] -> n3
    let config = SOPGraphConfig {
        nodes: vec![
            SOPNodeConfig {
                name: "n0".into(),
                kind: NodeKind::Wasm {
                    tool_id: "t".into(),
                    fuel_limit: 100,
                },
            },
            SOPNodeConfig {
                name: "n1".into(),
                kind: NodeKind::Wasm {
                    tool_id: "t".into(),
                    fuel_limit: 100,
                },
            },
            SOPNodeConfig {
                name: "n2".into(),
                kind: NodeKind::Wasm {
                    tool_id: "t".into(),
                    fuel_limit: 100,
                },
            },
            SOPNodeConfig {
                name: "n3".into(),
                kind: NodeKind::Wasm {
                    tool_id: "t".into(),
                    fuel_limit: 100,
                },
            },
        ],
        edges: vec![(0, 1), (0, 2), (1, 3), (2, 3)],
    };

    let graph = make_sop_graph(&config);
    let res = executor
        .execute(
            graph,
            petgraph::graph::NodeIndex::new(0),
            Arc::new(MockToolExecutor),
        )
        .await;
    assert!(res.is_ok());
    let ctx = res.unwrap();
    assert!(ctx.get_result("n0").is_some());
    assert!(ctx.get_result("n1").is_some());
    assert!(ctx.get_result("n2").is_some());
    assert!(ctx.get_result("n3").is_some());
}

#[tokio::test]
async fn test_executor_node_failure_blocks_unrecovered_execution() {
    let executor = build_executor();

    // 在 n0 上失败的 mock 执行器
    struct FailOnFirst;
    #[async_trait::async_trait]
    impl ToolExecutor for FailOnFirst {
        async fn execute(
            &self,
            tool_id: &str,
            _args: Vec<u8>,
            _fuel_limit: u64,
        ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
            if tool_id == "fail" {
                Err(sisyphus_types::TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: "intentional failure".into(),
                })
            } else {
                Ok(b"ok".to_vec())
            }
        }
    }

    let config = SOPGraphConfig {
        nodes: vec![
            SOPNodeConfig {
                name: "n0".into(),
                kind: NodeKind::Wasm {
                    tool_id: "fail".into(),
                    fuel_limit: 100,
                },
            },
            SOPNodeConfig {
                name: "n1".into(),
                kind: NodeKind::Wasm {
                    tool_id: "ok".into(),
                    fuel_limit: 100,
                },
            },
        ],
        edges: vec![(0, 1)],
    };

    let graph = SOPGraph::from_config(&config, Arc::new(FailOnFirst), Arc::new(MockTestExecutor))
        .expect("graph from config");
    let (tx, mut rx) = mpsc::unbounded_channel();

    let res = executor
        .execute_with_context(
            graph,
            petgraph::graph::NodeIndex::new(0),
            None,
            Some(tx),
            Arc::new(FailOnFirst),
            None,
            None,
            None,
        )
        .await;

    // 没有分支恢复入口的失败必须让整次执行失败，不能把错误文本当成功结果返回。
    let err = res.expect_err("未恢复的节点失败不得返回成功上下文");
    assert!(
        matches!(&err, CoreError::NodeExecutionFailed { node_name, .. } if node_name == "n0"),
        "期望结构化节点失败，实际：{err:?}"
    );
    assert!(
        std::error::Error::source(&err).is_some(),
        "必须保留原始错误来源"
    );

    // 普通后继不是恢复入口，不应被调度。
    let mut failed_nodes = Vec::new();
    while let Some(event) = rx.recv().await {
        if let ExecutionEvent::NodeFailed { node_name, .. } = event {
            failed_nodes.push(node_name);
        }
    }
    assert_eq!(failed_nodes, vec!["n0".to_string()]);
}

#[tokio::test]
async fn test_executor_empty_graph() {
    let executor = build_executor();
    let graph: Arc<SOPGraph> = Arc::new(SOPGraph::new());
    let res = executor
        .execute(
            graph,
            petgraph::graph::NodeIndex::new(0),
            Arc::new(MockToolExecutor),
        )
        .await;
    assert!(res.is_ok());
    let ctx = res.unwrap();
    assert!(ctx.get_result("nonexistent").is_none());
}

// ---------------------------------------------------------------------------
// 内核保存/加载测试
// ---------------------------------------------------------------------------

async fn build_test_kernel() -> SisyphusKernel {
    let state_store = Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("in-memory MfsStore"),
    );
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.expect("sqlite pool");
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(MockToolExecutor);

    let intel = Arc::new(sisyphus_intel::SisyphusIntel::new(
        Arc::new(sisyphus_intel::MonteCarloSimulator::new(1)),
        Arc::new(MockLogicalSimulator),
        Arc::new(sisyphus_intel::StandardReflector),
        Arc::new(sisyphus_intel::StandardEvolver::new()),
    ));

    let synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer> = Arc::new(
        sisyphus_intel::GoalDrivenSynthesizer::new(tool_executor.clone()),
    );

    let llm: Arc<dyn sisyphus_tools_llm::LlmClient> = Arc::new(sisyphus_tools_llm::DummyLlmClient);

    SisyphusKernel::new(
        state_store,
        Arc::new(sisyphus_mem::SopSqlStore::new(pool.clone()).await.expect("sop store")),
        Arc::new(sisyphus_mem::VectorMemoryStore::new(pool.clone()).await.expect("vector store")),
        Arc::new(MockExperienceBank),
        tool_executor,
        intel,
        llm.clone(),
        llm,
        synthesizer,
        Arc::new(parking_lot::Mutex::new(
            sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(".omo/test_meta"),
        )),
        Arc::new(MockConsensus),
        Arc::new({
            let store =
                sisyphus_mem::RoadmapSqlStore::new(pool.clone()).await.expect("roadmap store");
            store.init_schema().await.expect("init roadmap schema");
            store
        }),
        Arc::new(sisyphus_tools_llm::DummyEmbeddingClient),
    )
}

#[tokio::test]
async fn test_kernel_save_load_roundtrip() {
    let kernel = build_test_kernel().await;

    let config = SOPGraphConfig {
        nodes: vec![
            SOPNodeConfig {
                name: "n0".into(),
                kind: NodeKind::Wasm {
                    tool_id: "t0".into(),
                    fuel_limit: 100,
                },
            },
            SOPNodeConfig {
                name: "n1".into(),
                kind: NodeKind::Wasm {
                    tool_id: "t1".into(),
                    fuel_limit: 200,
                },
            },
        ],
        edges: vec![(0, 1)],
    };
    let graph = make_sop_graph(&config);

    kernel.save_sop("test_roundtrip", "default", &graph).await.unwrap();
    let loaded = kernel.load_sop("test_roundtrip", "default").await.unwrap();

    assert_eq!(loaded.graph.node_count(), 2);
    let names: Vec<String> = loaded.graph.node_weights().map(|n| n.name().to_string()).collect();
    assert!(names.contains(&"n0".to_string()));
    assert!(names.contains(&"n1".to_string()));

    let order =
        petgraph::algo::toposort(&loaded.graph, None).expect("loaded graph must be acyclic");
    assert_eq!(order.len(), 2);
}

#[tokio::test]
async fn test_kernel_empty_graph_load_rejected() {
    let kernel = build_test_kernel().await;

    let graph: Arc<SOPGraph> = Arc::new(SOPGraph::new());

    // 空图谱的序列化成功
    kernel.save_sop("test_empty", "default", &graph).await.unwrap();

    // 但重新加载失败，因为 from_graph 要求至少一个节点
    assert!(
        kernel.load_sop("test_empty", "default").await.is_err(),
        "empty graph load should be rejected"
    );
}

#[tokio::test]
async fn test_kernel_load_nonexistent() {
    let kernel = build_test_kernel().await;

    match kernel.load_sop("nonexistent_sop", "default").await {
        Err(e) => {
            assert!(
                e.to_string().contains("nonexistent_sop"),
                "error should mention the sop_id: got {}",
                e
            );
        },
        Ok(_) => panic!("expected SopNotFound error"),
    }
}

// ---------------------------------------------------------------------------
// 模拟阶段验证：战略管理器接受演进
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_strategic_manager_evolution_verification() {
    use sisyphus_types::{NodeKind, SOPGraphConfig, SOPNodeConfig};

    // MockSimulator 与 setup_manager 已提升到模块级（见文件末尾）。

    // 场景 1：演进被接受（0.5 -> 0.8）
    {
        let manager = setup_manager(vec![0.5, 0.8]).await;
        let config_sop = SOPGraphConfig {
            nodes: vec![SOPNodeConfig {
                name: "n0".into(),
                kind: NodeKind::Wasm {
                    tool_id: "t".into(),
                    fuel_limit: 100,
                },
            }],
            edges: vec![],
        };
        let sop_graph = make_sop_graph(&config_sop);

        let (res, report, evolved) = manager
            .run_cognitive_loop(
                sop_graph,
                "test_acc",
                None,
                None,
                None,
                None,
                |_id, _e| {},
                None,
                None,
            )
            .await
            .unwrap();

        assert!(
            evolved.is_some(),
            "Evolution should be accepted when probability increases"
        );
        assert!(report.iterations >= 1);
        assert!(!res.to_string().is_empty());
    }

    // 场景 2：演进被拒绝（0.5 -> 0.3）
    {
        let manager = setup_manager(vec![0.5, 0.3]).await;
        let config_sop = SOPGraphConfig {
            nodes: vec![SOPNodeConfig {
                name: "n0".into(),
                kind: NodeKind::Wasm {
                    tool_id: "t".into(),
                    fuel_limit: 100,
                },
            }],
            edges: vec![],
        };
        let sop_graph = make_sop_graph(&config_sop);

        let (res, report, evolved) = manager
            .run_cognitive_loop(
                sop_graph,
                "test_rej",
                None,
                None,
                None,
                None,
                |_id, _e| {},
                None,
                None,
            )
            .await
            .unwrap();

        assert!(
            evolved.is_none(),
            "Evolution should be rejected when probability decreases"
        );
        assert!(report.iterations >= 1);
        assert!(!res.to_string().is_empty());
    }
}

// ---------------------------------------------------------------------------
// B3：LLM 批判超时治理（评审建议 #8）
// ---------------------------------------------------------------------------

struct SlowLlmClient {
    delay: std::time::Duration,
}

#[async_trait::async_trait]
impl sisyphus_tools_llm::LlmClient for SlowLlmClient {
    async fn chat(
        &self,
        _system: &str,
        _user: &str,
        _json_mode: bool,
    ) -> Result<String, sisyphus_types::TypesError> {
        tokio::time::sleep(self.delay).await;
        Ok("{}".into())
    }

    async fn generate(&self, _prompt: &str) -> Result<String, sisyphus_types::TypesError> {
        tokio::time::sleep(self.delay).await;
        Ok(r#"{"critical_findings": [], "risk_score": 0.2,
               "suggested_refinement": "slow", "should_proceed": true}"#
            .into())
    }

    fn get_params_for(&self, _agent_id: &str) -> (String, String, String) {
        ("m".into(), "http://localhost".into(), "k".into())
    }

    fn breaker(&self) -> sisyphus_tools_llm::circuit_breaker::CircuitBreaker {
        sisyphus_tools_llm::circuit_breaker::CircuitBreaker::default()
    }
}

fn critique_fixture() -> (
    sisyphus_intel::ReflectionReport,
    sisyphus_intel::SimulationResult,
) {
    let report = sisyphus_intel::ReflectionReport {
        original_sop_id: "sop_test".into(),
        deviation_score: 0.1,
        suggested_improvements: vec!["improve".into()],
        improvement_signals: vec![],
        evolved_sop_structure: None,
        records: Vec::new(),
    };
    let prediction = sisyphus_intel::SimulationResult {
        path_id: "p1".into(),
        success_probability: 0.9,
        estimated_cost: 10,
        predicted_stability: 1.0,
        predicted_outcome: "ok".into(),
        predicted_effort: 1,
    };
    (report, prediction)
}

struct CritiqueStubSimulator;
#[async_trait::async_trait]
impl sisyphus_intel::Simulator for CritiqueStubSimulator {
    async fn simulate(
        &self,
        _g: Arc<SOPGraph>,
        _ctx: &ExecutionContext,
        _id: &str,
        _sop_id: &str,
        _goal: Option<&str>,
    ) -> Result<sisyphus_intel::SimulationResult, sisyphus_types::IntelError> {
        Ok(critique_fixture().1)
    }
}

async fn critique_manager(llm: Arc<dyn sisyphus_tools_llm::LlmClient>) -> IntelligenceManager {
    // 其余三个组件用空 trait 对象占位，仅满足类型约束。
    struct NopLogical;
    #[async_trait::async_trait]
    impl sisyphus_intel::simulation::LogicalSimulator for NopLogical {
        async fn simulate_logic(
            &self,
            _graph: Arc<SOPGraph>,
            _context: &ExecutionContext,
            _sop_id: &str,
            _goal_text: Option<&str>,
        ) -> Result<sisyphus_intel::simulation::LogicalSimulationResult, sisyphus_types::IntelError>
        {
            unimplemented!("not used in critique tests")
        }
    }
    struct NopReflector;
    #[async_trait::async_trait]
    impl sisyphus_intel::reflect::Reflector for NopReflector {
        async fn reflect(
            &self,
            _t: &str,
            _p: &sisyphus_intel::SimulationResult,
            _e: Option<&[sisyphus_types::experience::ExperienceTuple]>,
        ) -> Result<sisyphus_intel::ReflectionReport, sisyphus_types::IntelError> {
            Ok(critique_fixture().0)
        }
    }
    struct NopEvolver;
    #[async_trait::async_trait]
    impl sisyphus_intel::reflect::SOPEvolver for NopEvolver {
        async fn evolve(
            &self,
            g: Arc<SOPGraph>,
            _r: &sisyphus_intel::ReflectionReport,
            _p: &sisyphus_intel::SimulationResult,
            _i: Option<&str>,
        ) -> Result<Arc<SOPGraph>, sisyphus_types::IntelError> {
            Ok(g)
        }
    }
    let intel = Arc::new(sisyphus_intel::SisyphusIntel::new(
        Arc::new(CritiqueStubSimulator),
        Arc::new(NopLogical),
        Arc::new(NopReflector),
        Arc::new(NopEvolver),
    ));
    let synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer> = Arc::new(
        sisyphus_intel::GoalDrivenSynthesizer::new(Arc::new(MockToolExecutor)),
    );
    IntelligenceManager::new(intel, synthesizer, llm)
}

#[tokio::test]
async fn test_critique_timeout_degrades_permissively() {
    // LLM（延迟 5s）慢于配置的超时（1s）：调用应在约 1s 处被切断并降级，
    // 而不是等满 LLM 的 5s。
    let mgr = critique_manager(Arc::new(SlowLlmClient {
        delay: std::time::Duration::from_secs(5),
    }))
    .await;
    let (report, prediction) = critique_fixture();
    let start = std::time::Instant::now();
    let result = mgr.critique_reflection(&report, &prediction, 1).await.unwrap();
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(4),
        "took {elapsed:?}"
    );
    assert!(result.should_proceed);
    assert_eq!(result.risk_score, 0.0);
    assert!(result.suggested_refinement.contains("timed out"));
}

#[tokio::test]
async fn test_critique_within_budget_returns_llm_result() {
    // LLM 在预算内返回：结果应原样透传而非降级。
    let mgr = critique_manager(Arc::new(SlowLlmClient {
        delay: std::time::Duration::from_millis(50),
    }))
    .await;
    let (report, prediction) = critique_fixture();
    let result = mgr.critique_reflection(&report, &prediction, 5).await.unwrap();
    assert_eq!(result.risk_score, 0.2);
    assert_eq!(result.suggested_refinement, "slow");
}

#[tokio::test]
async fn test_critique_zero_timeout_disables_stage() {
    // 零超时：跳过 LLM 调用，立即返回禁用语义的宽松默认。
    let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    struct ProbeLlm(Arc<std::sync::atomic::AtomicBool>);
    #[async_trait::async_trait]
    impl sisyphus_tools_llm::LlmClient for ProbeLlm {
        async fn chat(
            &self,
            _: &str,
            _: &str,
            _: bool,
        ) -> Result<String, sisyphus_types::TypesError> {
            Ok("{}".into())
        }
        async fn generate(&self, _: &str) -> Result<String, sisyphus_types::TypesError> {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok("{}".into())
        }
        fn get_params_for(&self, _: &str) -> (String, String, String) {
            ("m".into(), "e".into(), "k".into())
        }
        fn breaker(&self) -> sisyphus_tools_llm::circuit_breaker::CircuitBreaker {
            sisyphus_tools_llm::circuit_breaker::CircuitBreaker::default()
        }
    }
    let probe = Arc::new(ProbeLlm(called.clone()));
    let mgr = critique_manager(probe as Arc<dyn sisyphus_tools_llm::LlmClient>).await;
    let (report, prediction) = critique_fixture();
    let start = std::time::Instant::now();
    let result = mgr.critique_reflection(&report, &prediction, 0).await.unwrap();
    assert!(start.elapsed() < std::time::Duration::from_millis(50));
    assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
    assert!(result.suggested_refinement.contains("disabled"));
}

// ===================================================================
// TokenBudget 配置与跟踪器单元测试
// ===================================================================

#[test]
fn test_token_budget_default_is_unbounded() {
    use sisyphus_types::config::sub::TokenBudgetConfig;
    let cfg = TokenBudgetConfig::default();
    assert!(cfg.cap.is_none(), "默认应无 cap");
    assert!((0.0..=1.0).contains(&cfg.reserve_ratio));
    let tracker = if let Some(cap) = cfg.cap {
        sisyphus_intel::token_budget::TokenBudgetTracker::new(cap, cfg.reserve_ratio)
    } else {
        sisyphus_intel::token_budget::TokenBudgetTracker::unbounded()
    };
    assert!(!tracker.should_skip_llm());
    tracker.record(1_000_000);
    assert!(!tracker.should_skip_llm());
}

#[test]
fn test_token_budget_capped_skips_llm() {
    // cap=100, reserve=0.5: 剩余 < 50 时 skip
    let tracker = sisyphus_intel::token_budget::TokenBudgetTracker::new(100, 0.5);
    assert!(!tracker.should_skip_llm());
    tracker.record(40); // 剩 60, > 50
    assert!(!tracker.should_skip_llm());
    tracker.record(10); // 剩 50, < 50 false
    assert!(!tracker.should_skip_llm());
    tracker.record(1); // 剩 49, < 50
    assert!(tracker.should_skip_llm());
}

#[test]
fn test_token_budget_reserve_ratio() {
    let tracker = sisyphus_intel::token_budget::TokenBudgetTracker::new(100, 0.5);
    tracker.record(40);
    assert!(!tracker.should_skip_llm());
    tracker.record(10);
    assert!(!tracker.should_skip_llm());
    tracker.record(1);
    assert!(tracker.should_skip_llm());
}

#[test]
fn test_token_budget_config_serde_roundtrip() {
    use sisyphus_types::config::sub::TokenBudgetConfig;
    let cfg = TokenBudgetConfig {
        cap: Some(50000),
        reserve_ratio: 0.2,
    };
    let json = serde_json::to_string(&cfg).expect("serialize");
    let back: TokenBudgetConfig = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.cap, Some(50000));
    assert!((back.reserve_ratio - 0.2).abs() < 1e-9);
}

// ---------------------------------------------------------------------------
// 参数顺序回归：goal_text 与 project_id 不得错位（评审 P0-0）
// ---------------------------------------------------------------------------

/// 记录最近一次查询的经验库，用于断言目标文本正确传入。
struct RecordingExperienceBank {
    last_query: parking_lot::Mutex<Option<String>>,
    last_record: parking_lot::Mutex<Option<sisyphus_types::experience::ExperienceTuple>>,
}

impl RecordingExperienceBank {
    fn new() -> Self {
        Self {
            last_query: parking_lot::Mutex::new(None),
            last_record: parking_lot::Mutex::new(None),
        }
    }
}

#[async_trait::async_trait]
impl sisyphus_types::experience::ExperienceBank for RecordingExperienceBank {
    async fn record(
        &self,
        exp: sisyphus_types::experience::ExperienceTuple,
    ) -> Result<(), sisyphus_types::TypesError> {
        *self.last_record.lock() = Some(exp);
        Ok(())
    }

    async fn query_similar(
        &self,
        query: sisyphus_types::experience::ExperienceQuery,
    ) -> Result<Vec<sisyphus_types::experience::ExperienceTuple>, sisyphus_types::TypesError> {
        *self.last_query.lock() = Some(query.query_text);
        Ok(vec![])
    }
}

#[tokio::test]
async fn test_cognitive_loop_receives_goal_text_not_project_id() {
    let recorder = Arc::new(RecordingExperienceBank::new());

    // 构建注入 RecordingExperienceBank 的内核，走真实 kernel 公共入口。
    let state_store = Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("in-memory MfsStore"),
    );
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.expect("sqlite pool");
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(MockToolExecutor);

    let intel = Arc::new(sisyphus_intel::SisyphusIntel::new(
        Arc::new(sisyphus_intel::MonteCarloSimulator::new(1)),
        Arc::new(MockLogicalSimulator),
        Arc::new(sisyphus_intel::StandardReflector),
        Arc::new(sisyphus_intel::StandardEvolver::new()),
    ));

    let synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer> = Arc::new(
        sisyphus_intel::GoalDrivenSynthesizer::new(tool_executor.clone()),
    );

    let llm: Arc<dyn sisyphus_tools_llm::LlmClient> = Arc::new(sisyphus_tools_llm::DummyLlmClient);

    let kernel = SisyphusKernel::new(
        state_store,
        Arc::new(sisyphus_mem::SopSqlStore::new(pool.clone()).await.expect("sop store")),
        Arc::new(sisyphus_mem::VectorMemoryStore::new(pool.clone()).await.expect("vector store")),
        recorder.clone(),
        tool_executor,
        intel,
        llm.clone(),
        llm,
        synthesizer,
        Arc::new(parking_lot::Mutex::new(
            sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(".omo/test_meta"),
        )),
        Arc::new(MockConsensus),
        Arc::new({
            let store =
                sisyphus_mem::RoadmapSqlStore::new(pool.clone()).await.expect("roadmap store");
            store.init_schema().await.expect("init roadmap schema");
            store
        }),
        Arc::new(sisyphus_tools_llm::DummyEmbeddingClient),
    );

    // 通过真实存储路径预置项目/里程碑/阶段/目标，goal_text 应来自 roadmap。
    let unique_goal = "goal-marker-9f31c2";
    let project_id = "proj-8a7d21";
    let sop_id = "goal-param-regression";
    kernel
        .roadmap_manager
        .create_project(project_id, "param-regression", "test project")
        .await
        .expect("create project");
    kernel
        .roadmap_manager
        .add_milestone(
            project_id,
            sisyphus_types::Milestone {
                id: "ms-1".into(),
                name: "ms".into(),
                phases: vec![],
                status: sisyphus_types::MilestoneStatus::Pending,
            },
        )
        .await
        .expect("add milestone");
    kernel
        .roadmap_manager
        .add_phase(
            project_id,
            "ms-1",
            sisyphus_types::Phase {
                id: "ph-1".into(),
                name: "ph".into(),
                goals: vec![],
                depends_on: vec![],
            },
        )
        .await
        .expect("add phase");
    kernel
        .roadmap_manager
        .add_goal(
            project_id,
            "ms-1",
            "ph-1",
            sisyphus_types::GoalRef {
                goal_id: sop_id.to_string(),
                description: unique_goal.to_string(),
                status: sisyphus_types::MilestoneStatus::Pending,
            },
        )
        .await
        .expect("add goal");

    let config_sop = SOPGraphConfig {
        nodes: vec![SOPNodeConfig {
            name: "n0".into(),
            kind: NodeKind::Wasm {
                tool_id: "t".into(),
                fuel_limit: 100,
            },
        }],
        edges: vec![],
    };
    let sop_graph = make_sop_graph(&config_sop);

    // execute_with_cognition 内部从 roadmap 取 goal_description 后调用
    // run_cognitive_loop。若参数错位，经验查询文本将是项目标识。
    let _ = kernel
        .execute_with_cognition(sop_graph, sop_id, Some(project_id), None, None, None)
        .await
        .expect("loop runs");

    let observed = recorder.last_query.lock().clone();
    assert_eq!(
        observed.as_deref(),
        Some(unique_goal),
        "经验检索必须收到目标文本，而不是项目标识"
    );
}

// ---------------------------------------------------------------------------
// 仿真门禁回归：Critical 缺陷重试耗尽后必须阻断，不得静默执行（评审 P1-1）
// ---------------------------------------------------------------------------

/// 始终报告 Critical 缺陷的逻辑仿真替身。
#[derive(Default)]
struct CriticalFlawSimulator {
    calls: AtomicUsize,
}

#[derive(Default)]
struct CountingToolExecutor {
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl ToolExecutor for CountingToolExecutor {
    async fn execute(
        &self,
        _tool_id: &str,
        _args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(b"mock".to_vec())
    }
}

fn assert_critical_gate_blocked(result: Result<(NodeOutput, String), CoreError>) {
    match result {
        Err(CoreError::Internal(message)) => assert_eq!(
            message,
            "Logical simulation still reports 1 critical flaw(s) after 3 refinement attempts; execution blocked: [n0] persistent critical flaw",
            "必须是严重仿真门禁错误，其他失败不能证明门禁生效"
        ),
        other => panic!("应由严重仿真门禁阻断，实际为 {other:?}"),
    }
}

#[async_trait::async_trait]
impl sisyphus_intel::simulation::LogicalSimulator for CriticalFlawSimulator {
    async fn simulate_logic(
        &self,
        _graph: Arc<SOPGraph>,
        _context: &ExecutionContext,
        _sop_id: &str,
        _goal_text: Option<&str>,
    ) -> Result<sisyphus_intel::simulation::LogicalSimulationResult, sisyphus_types::IntelError>
    {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(sisyphus_intel::simulation::LogicalSimulationResult {
            is_valid: false,
            flaws: vec![sisyphus_intel::simulation::LogicalFlaw {
                node_name: "n0".into(),
                description: "persistent critical flaw".into(),
                severity: sisyphus_intel::simulation::FlawSeverity::Critical,
            }],
            predicted_path: vec![],
            confidence: 0.1,
        })
    }
}

/// 脚本化 LLM：generate 不含 UltraWork，意图分析回退到 Standard；chat 对审查请求返回 approved，
/// 其余（提案/修正）返回与测试图等价的合法配置 JSON。
struct ScriptedLlmClient;

#[async_trait::async_trait]
impl sisyphus_tools_llm::LlmClient for ScriptedLlmClient {
    async fn generate(&self, _prompt: &str) -> Result<String, sisyphus_types::TypesError> {
        Ok(r#"{"critical_findings": [], "risk_score": 0.1, "suggested_refinement": "none", "should_proceed": true}"#.into())
    }

    async fn chat(
        &self,
        _system: &str,
        user: &str,
        _stream: bool,
    ) -> Result<String, sisyphus_types::TypesError> {
        if user.contains("review this proposal") {
            Ok(r#"{"approved": true, "feedback": "ok"}"#.into())
        } else {
            let cfg = SOPGraphConfig {
                nodes: vec![SOPNodeConfig {
                    name: "n0".into(),
                    kind: NodeKind::Wasm {
                        tool_id: "t".into(),
                        fuel_limit: 100,
                    },
                }],
                edges: vec![],
            };
            serde_json::to_string(&cfg)
                .map_err(|e| sisyphus_types::TypesError::InternalError(e.to_string()))
        }
    }

    fn get_params_for(&self, _agent_id: &str) -> (String, String, String) {
        (
            "test_key".into(),
            "http://test:8080".into(),
            "test-model".into(),
        )
    }

    fn breaker(&self) -> sisyphus_tools_llm::circuit_breaker::CircuitBreaker {
        sisyphus_tools_llm::circuit_breaker::CircuitBreaker::default()
    }
}

#[tokio::test]
async fn test_execute_goal_blocks_when_critical_flaws_persist() {
    let state_store = Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("in-memory MfsStore"),
    );
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.expect("sqlite pool");
    let tool_executor = Arc::new(CountingToolExecutor::default());
    let simulator = Arc::new(CriticalFlawSimulator::default());

    let intel = Arc::new(sisyphus_intel::SisyphusIntel::new(
        Arc::new(sisyphus_intel::MonteCarloSimulator::new(1)),
        simulator.clone(),
        Arc::new(sisyphus_intel::StandardReflector),
        Arc::new(sisyphus_intel::StandardEvolver::new()),
    ));

    let synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer> = Arc::new(
        sisyphus_intel::GoalDrivenSynthesizer::new(tool_executor.clone()),
    );

    let llm: Arc<dyn sisyphus_tools_llm::LlmClient> = Arc::new(ScriptedLlmClient);

    let kernel = SisyphusKernel::new(
        state_store,
        Arc::new(sisyphus_mem::SopSqlStore::new(pool.clone()).await.expect("sop store")),
        Arc::new(sisyphus_mem::VectorMemoryStore::new(pool.clone()).await.expect("vector store")),
        Arc::new(MockExperienceBank),
        tool_executor.clone(),
        intel,
        llm.clone(),
        llm,
        synthesizer,
        Arc::new(parking_lot::Mutex::new(
            sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(".omo/test_meta"),
        )),
        Arc::new(MockConsensus),
        Arc::new({
            let store =
                sisyphus_mem::RoadmapSqlStore::new(pool.clone()).await.expect("roadmap store");
            store.init_schema().await.expect("init roadmap schema");
            store
        }),
        Arc::new(sisyphus_tools_llm::DummyEmbeddingClient),
    );

    // 仿真持续报告 Critical 缺陷。重试耗尽后应阻断执行而非放行。
    let result = kernel
        .execute_goal(
            "goal with persistent critical flaw",
            "sop-critical",
            "default",
            None,
            None,
        )
        .await;

    assert_critical_gate_blocked(result);
    assert_eq!(
        simulator.calls.load(Ordering::SeqCst),
        3,
        "必须耗尽三次仿真尝试"
    );
    assert_eq!(
        tool_executor.calls.load(Ordering::SeqCst),
        0,
        "门禁阻断前不得执行工具"
    );
}

#[tokio::test]
async fn test_execute_goal_streaming_blocks_when_critical_flaws_persist() {
    // 与门禁测试相同的内核构造（CriticalFlawSimulator + ScriptedLlmClient）。
    let state_store = Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("in-memory MfsStore"),
    );
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.expect("sqlite pool");
    let tool_executor = Arc::new(CountingToolExecutor::default());
    let simulator = Arc::new(CriticalFlawSimulator::default());

    let intel = Arc::new(sisyphus_intel::SisyphusIntel::new(
        Arc::new(sisyphus_intel::MonteCarloSimulator::new(1)),
        simulator.clone(),
        Arc::new(sisyphus_intel::StandardReflector),
        Arc::new(sisyphus_intel::StandardEvolver::new()),
    ));

    let synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer> = Arc::new(
        sisyphus_intel::GoalDrivenSynthesizer::new(tool_executor.clone()),
    );

    let llm: Arc<dyn sisyphus_tools_llm::LlmClient> = Arc::new(ScriptedLlmClient);

    let kernel = SisyphusKernel::new(
        state_store,
        Arc::new(sisyphus_mem::SopSqlStore::new(pool.clone()).await.expect("sop store")),
        Arc::new(sisyphus_mem::VectorMemoryStore::new(pool.clone()).await.expect("vector store")),
        Arc::new(MockExperienceBank),
        tool_executor.clone(),
        intel,
        llm.clone(),
        llm,
        synthesizer,
        Arc::new(parking_lot::Mutex::new(
            sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(".omo/test_meta"),
        )),
        Arc::new(MockConsensus),
        Arc::new({
            let store =
                sisyphus_mem::RoadmapSqlStore::new(pool.clone()).await.expect("roadmap store");
            store.init_schema().await.expect("init roadmap schema");
            store
        }),
        Arc::new(sisyphus_tools_llm::DummyEmbeddingClient),
    );

    // 流式入口必须与非流式共享同一仿真门禁：持久 Critical 缺陷应阻断。
    let result = kernel
        .execute_goal_streaming(
            "goal via streaming",
            "sop-critical-stream",
            "default",
            None,
            None,
        )
        .await;

    assert_critical_gate_blocked(result);
    assert_eq!(
        simulator.calls.load(Ordering::SeqCst),
        3,
        "流式入口必须耗尽三次仿真尝试"
    );
    assert_eq!(
        tool_executor.calls.load(Ordering::SeqCst),
        0,
        "流式门禁阻断前不得执行工具"
    );
}

async fn build_recording_kernel(
    state_store: Arc<sisyphus_mem::MfsStore>,
    recorder: Arc<dyn sisyphus_types::experience::ExperienceBank>,
) -> SisyphusKernel {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.expect("sqlite pool");
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(MockToolExecutor);

    let intel = Arc::new(sisyphus_intel::SisyphusIntel::new(
        Arc::new(sisyphus_intel::MonteCarloSimulator::new(1)),
        Arc::new(MockLogicalSimulator),
        Arc::new(sisyphus_intel::StandardReflector),
        Arc::new(sisyphus_intel::StandardEvolver::new()),
    ));

    let synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer> = Arc::new(
        sisyphus_intel::GoalDrivenSynthesizer::new(tool_executor.clone()),
    );

    let llm: Arc<dyn sisyphus_tools_llm::LlmClient> = Arc::new(ScriptedLlmClient);

    SisyphusKernel::new(
        state_store,
        Arc::new(sisyphus_mem::SopSqlStore::new(pool.clone()).await.expect("sop store")),
        Arc::new(sisyphus_mem::VectorMemoryStore::new(pool.clone()).await.expect("vector store")),
        recorder,
        tool_executor,
        intel,
        llm.clone(),
        llm,
        synthesizer,
        Arc::new(parking_lot::Mutex::new(
            sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(".omo/test_meta"),
        )),
        Arc::new(MockConsensus),
        Arc::new({
            let store =
                sisyphus_mem::RoadmapSqlStore::new(pool.clone()).await.expect("roadmap store");
            store.init_schema().await.expect("init roadmap schema");
            store
        }),
        Arc::new(sisyphus_tools_llm::DummyEmbeddingClient),
    )
}

#[tokio::test]
async fn test_execute_and_record_persists_structured_metadata() {
    let recorder = Arc::new(RecordingExperienceBank::new());
    let state_store = Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("in-memory MfsStore"),
    );
    let kernel = build_recording_kernel(state_store, recorder.clone()).await;

    let project_id = "proj-rec-7c1d";
    kernel
        .roadmap_manager
        .create_project(project_id, "rec", "structured metadata test")
        .await
        .expect("create project");

    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let (output, _, _) = kernel
        .execute_and_record(
            "structured metadata goal",
            "sop-rec",
            project_id,
            "ms-x",
            "ph-x",
            "goal-x",
            None,
        )
        .await
        .expect("execute_and_record ok");

    let after = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // outcome.timestamp 必须是墙钟时间而非耗时（耗时对快速执行约为 0）。
    let outcomes = kernel.roadmap_manager.get_outcomes(project_id).await.expect("outcomes");
    let outcome = outcomes.iter().find(|o| o.goal_id == "goal-x").expect("outcome persisted");
    assert!(
        outcome.timestamp >= before && outcome.timestamp <= after,
        "outcome.timestamp 必须是墙钟时间 [{before}, {after}]，实际 {}",
        outcome.timestamp
    );

    // 经验元组必须携带项目范围。
    let recorded = recorder.last_record.lock().clone().expect("experience recorded");
    assert_eq!(
        recorded.project_id.as_deref(),
        Some(project_id),
        "经验必须记录项目范围"
    );

    // 当前 reflect_phase 传入 n0:"mock" 文本而非逐行 JSON 记录，
    // StandardReflector 使用无错误文本的兜底偏差 0.1，不是结构化记录的 0.0。
    assert_eq!(output.as_value(), &serde_json::json!("mock"));
    assert!(outcome.success);
    assert!(outcome.error.is_none());
    assert_eq!(outcome.project_id, project_id);
    assert_eq!(outcome.milestone_id, "ms-x");
    assert_eq!(outcome.phase_id, "ph-x");
    let dev = outcome.deviation_score.expect("成功执行必须记录偏差分数");
    assert_eq!(dev, 0.1, "必须持久化当前反思链路的真实偏差");
    assert_eq!(recorded.action_id, "sop-rec");
    assert_eq!(recorded.context_text, "structured metadata goal");
    assert_eq!(
        recorded.context_hash,
        format!("{:x}", md5::compute("structured metadata goal"))
    );
    assert!(recorded.outcome_success);
    assert_eq!(
        recorded.deviation_score, dev,
        "同次执行的经验与目标结果必须记录相同偏差"
    );
}

// 没有现成的日志捕获测试设施。此订阅器只记录当前异步调用的告警，
// 不安装全局订阅器，不新增依赖，也不依赖格式化后的终端文本。
#[derive(Clone, Default)]
struct WarningCapture {
    events: Arc<parking_lot::Mutex<Vec<HashMap<String, String>>>>,
}

#[derive(Default)]
struct WarningFields(HashMap<String, String>);

impl tracing::field::Visit for WarningFields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().to_string(), format!("{value:?}"));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().to_string(), value.to_string());
    }
}

impl tracing::Subscriber for WarningCapture {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        *metadata.level() == tracing::Level::WARN
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        static NEXT_SPAN: AtomicUsize = AtomicUsize::new(1);
        tracing::span::Id::from_u64(
            u64::try_from(NEXT_SPAN.fetch_add(1, Ordering::Relaxed)).expect("span id"),
        )
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        if *event.metadata().level() != tracing::Level::WARN
            || event.metadata().target() != "sisyphus_core::kernel::execution"
        {
            return;
        }
        let mut fields = WarningFields::default();
        event.record(&mut fields);
        self.events.lock().push(fields.0);
    }
}

struct FailingExperienceBank;

#[async_trait::async_trait]
impl sisyphus_types::experience::ExperienceBank for FailingExperienceBank {
    async fn record(
        &self,
        _: sisyphus_types::experience::ExperienceTuple,
    ) -> Result<(), sisyphus_types::TypesError> {
        Err(sisyphus_types::TypesError::StateStoreError(
            "experience-write-sentinel".into(),
        ))
    }

    async fn query_similar(
        &self,
        _: sisyphus_types::experience::ExperienceQuery,
    ) -> Result<Vec<sisyphus_types::experience::ExperienceTuple>, sisyphus_types::TypesError> {
        Ok(vec![])
    }
}

async fn assert_recording_failure_warns(fail_outcome: bool) {
    use tracing::instrument::WithSubscriber;

    let state_store = if fail_outcome {
        // MfsStore 是具体类型，写入先入队，关闭 SQLite 并不能使 record_outcome
        // 同步失败。让独立运行时退出以取消后台写入器，使发送端确定性关闭。
        // SQLite 连接池归当前运行时所有，仍可正常读取项目执行上下文。
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.expect("state pool");
        Arc::new(
            tokio::task::spawn_blocking(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("writer runtime");
                runtime
                    .block_on(sisyphus_mem::MfsStore::with_pool(
                        pool,
                        std::num::NonZeroUsize::new(64).expect("nonzero capacity"),
                        1,
                        10,
                    ))
                    .expect("state store")
            })
            .await
            .expect("writer runtime task"),
        )
    } else {
        Arc::new(
            sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
                .await
                .expect("state store"),
        )
    };

    if fail_outcome {
        let failure = sisyphus_types::StateStore::set_state_json(
            state_store.as_ref(),
            "warn-project",
            "closed-writer-probe",
            serde_json::json!(true),
        )
        .await
        .expect_err("后台接收端必须已关闭，不能用无关数据库错误代替写入失败");
        assert!(
            matches!(failure, sisyphus_types::TypesError::StateStoreError(ref message)
            if message == "channel closed"),
            "实际故障为 {failure:?}"
        );
    }

    let recorder = Arc::new(RecordingExperienceBank::new());
    let bank: Arc<dyn sisyphus_types::experience::ExperienceBank> = if fail_outcome {
        recorder.clone()
    } else {
        Arc::new(FailingExperienceBank)
    };
    let mut kernel = build_recording_kernel(state_store, bank).await;
    let simulator = Arc::new(CriticalFlawSimulator::default());
    kernel.logical_simulator = simulator.clone();
    kernel
        .roadmap_manager
        .create_project("warn-project", "warnings", "test")
        .await
        .expect("create project");

    let capture = WarningCapture::default();
    let result = kernel
        .execute_and_record(
            "warning capture goal",
            "warn-sop",
            "warn-project",
            "warn-ms",
            "warn-phase",
            "warn-goal",
            None,
        )
        .with_subscriber(capture.clone())
        .await;
    // 写入告警不得覆盖原始门禁错误，也不能把其他执行失败误认成门禁成功。
    assert_critical_gate_blocked(result.map(|(output, report, _)| (output, report)));
    assert_eq!(simulator.calls.load(Ordering::SeqCst), 3);

    let events = capture.events.lock().clone();
    assert_eq!(
        events.len(),
        1,
        "只应出现所注入失败分支的告警，实际 {events:?}"
    );
    let warning = &events[0];
    assert_eq!(
        warning.get("project_id").map(String::as_str),
        Some("warn-project")
    );
    let error = warning.get("error").expect("告警必须携带实际写入错误");
    if fail_outcome {
        assert_eq!(
            warning.get("message").map(String::as_str),
            Some("Failed to persist execution outcome; health evaluation will use stale data")
        );
        assert_eq!(
            warning.get("goal_id").map(String::as_str),
            Some("warn-goal")
        );
        assert!(
            error.contains("channel closed"),
            "告警必须包含闭合发送通道错误：{error}"
        );
        let experience =
            recorder.last_record.lock().clone().expect("结果写入失败后仍须尝试记录经验");
        assert_eq!(experience.action_id, "warn-sop");
        assert!(!experience.outcome_success);
    } else {
        assert_eq!(
            warning.get("message").map(String::as_str),
            Some("Failed to record experience")
        );
        assert!(
            error.contains("experience-write-sentinel"),
            "告警必须包含经验写入错误：{error}"
        );
        let outcomes = kernel.roadmap_manager.get_outcomes("warn-project").await.expect("outcomes");
        let outcome = outcomes
            .iter()
            .find(|outcome| outcome.goal_id == "warn-goal")
            .expect("经验写入失败不得影响已保存的结果");
        assert!(!outcome.success);
        assert_eq!(outcome.deviation_score, None);
    }
}

#[tokio::test]
async fn test_execute_and_record_warns_when_outcome_write_fails() {
    assert_recording_failure_warns(true).await;
}

#[tokio::test]
async fn test_execute_and_record_warns_when_experience_write_fails() {
    assert_recording_failure_warns(false).await;
}

#[tokio::test]
async fn test_concurrent_loops_have_independent_depth_history() {
    let manager = setup_manager(vec![0.5, 0.6, 0.7, 0.8]).await;
    manager.depth_controller.observe_iteration(0.37);
    let depth_before = manager.depth_controller.confidence_history();
    // 预置可从公共报告查询到的非空历史，防止 reset 后的空历史造成假阳性。
    let anomaly_before = {
        let detector = manager.anomaly_detector.lock();
        for _ in 0..4 {
            detector.observe("shared-history-sentinel", 100, 10, false);
        }
        detector.generate_report()
    };
    assert_eq!(anomaly_before.anomalies.len(), 1);
    assert_eq!(anomaly_before.anomalies[0].observed, 4.0);

    let config_sop = SOPGraphConfig {
        nodes: vec![SOPNodeConfig {
            name: "n0".into(),
            kind: NodeKind::Wasm {
                tool_id: "t".into(),
                fuel_limit: 100,
            },
        }],
        edges: vec![],
    };
    let graph_a = make_sop_graph(&config_sop);
    let graph_b = make_sop_graph(&config_sop);

    // 两个循环并发执行。共享控制器下，B 启动时 reset() 会清掉
    // A 的置信度历史；隔离实现下互不影响。
    let (a, b) = tokio::join!(
        manager.run_cognitive_loop(
            graph_a,
            "sop-a",
            None,
            None,
            None,
            None,
            |_id, _e| {},
            None,
            None,
        ),
        manager.run_cognitive_loop(
            graph_b,
            "sop-b",
            None,
            None,
            None,
            None,
            |_id, _e| {},
            None,
            None,
        ),
    );
    assert!(
        a.is_ok(),
        "loop a failed: {:?}",
        a.as_ref().err().map(|e| e.to_string())
    );
    assert!(
        b.is_ok(),
        "loop b failed: {:?}",
        b.as_ref().err().map(|e| e.to_string())
    );

    assert_eq!(
        manager.depth_controller.confidence_history(),
        depth_before,
        "运行不得触碰或重置管理器共享的深度控制器历史"
    );
    // 公共报告不暴露完整燃料/耗时历史，但新增节点或清空历史会改变此错误率摘要。
    let anomaly_after = manager.anomaly_detector.lock().generate_report();
    assert_eq!(
        anomaly_after.overall_severity,
        anomaly_before.overall_severity
    );
    assert_eq!(
        anomaly_after.should_extra_reflect,
        anomaly_before.should_extra_reflect
    );
    assert_eq!(
        anomaly_after.anomalies.len(),
        anomaly_before.anomalies.len()
    );
    for (after, before) in anomaly_after.anomalies.iter().zip(&anomaly_before.anomalies) {
        assert_eq!(after.node_name, before.node_name);
        assert_eq!(after.severity, before.severity);
        assert_eq!(after.metric, before.metric);
        assert_eq!(after.observed, before.observed);
        assert_eq!(after.expected, before.expected);
        assert_eq!(after.description, before.description);
    }
}

struct MockSimulator {
    probs: parking_lot::Mutex<VecDeque<f64>>,
}
impl MockSimulator {
    fn new(p: Vec<f64>) -> Self {
        Self {
            probs: parking_lot::Mutex::new(VecDeque::from(p)),
        }
    }
}
#[async_trait::async_trait]
impl sisyphus_intel::Simulator for MockSimulator {
    async fn simulate(
        &self,
        _g: Arc<SOPGraph>,
        _ctx: &ExecutionContext,
        _id: &str,
        _sop_id: &str,
        _goal: Option<&str>,
    ) -> Result<sisyphus_intel::SimulationResult, sisyphus_types::IntelError> {
        let p = self.probs.lock().pop_front().unwrap_or(0.5);
        Ok(sisyphus_intel::SimulationResult {
            path_id: "p1".into(),
            success_probability: p,
            estimated_cost: 100,
            predicted_stability: 1.0,
            predicted_effort: 50,
            predicted_outcome: "ok".into(),
        })
    }
}

use crate::strategic_manager::StrategicManager;
use sisyphus_types::config::CognitiveLoopConfig;
use std::collections::VecDeque;

async fn setup_manager(probs: Vec<f64>) -> StrategicManager {
    let simulator = Arc::new(MockSimulator::new(probs));
    let intel = Arc::new(sisyphus_intel::SisyphusIntel::new(
        simulator,
        Arc::new(MockLogicalSimulator),
        Arc::new(MockReflector),
        Arc::new(MockEvolver),
    ));

    let llm: Arc<dyn sisyphus_tools_llm::LlmClient> = Arc::new(sisyphus_tools_llm::DummyLlmClient);
    let synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer> = Arc::new(
        sisyphus_intel::GoalDrivenSynthesizer::new(Arc::new(MockToolExecutor)),
    );

    let intelligence_manager = Arc::new(IntelligenceManager::new(intel, synthesizer, llm.clone()));

    let state_store = Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("in-memory MfsStore"),
    );
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.expect("sqlite pool");
    let sop_store =
        Arc::new(sisyphus_mem::SopSqlStore::new(pool.clone()).await.expect("sop store"));
    let knowledge = Arc::new(sisyphus_knowledge::AssetManager::new(
        std::path::PathBuf::from(".omo/test_knowledge"),
    ));
    let intervention = Arc::new(crate::intervention_manager::InterventionManager::new());
    let internalization = Arc::new(crate::internalization_manager::InternalizationManager::new(
        llm.clone(),
        knowledge.clone(),
    ));
    let execution_manager = Arc::new(ExecutionManager::new(
        state_store.clone(),
        sop_store,
        Arc::new(MockToolExecutor),
        Arc::new(MockTestExecutor),
        intervention.clone(),
        internalization.clone(),
        None,
    ));

    let config = CognitiveLoopConfig {
        max_iterations: 1,
        convergence_threshold: 0.01,
        llm_stage_timeout_secs: 30,
        adaptive_depth: sisyphus_types::config::AdaptiveDepthConfig::default(),
        anomaly_detector: sisyphus_types::config::AnomalyDetectorConfig::default(),
        token_budget: sisyphus_types::config::TokenBudgetConfig::default(),
    };

    let benchmark = Arc::new(parking_lot::Mutex::new(
        sisyphus_tools_build::benchmark::ContinuousBenchmark::new(".omo/test_bench"),
    ));
    let meta_learner = Arc::new(parking_lot::Mutex::new(
        sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(".omo/test_meta"),
    ));
    let roadmap = Arc::new(sisyphus_roadmap::RoadmapManager::new(
        state_store.clone(),
        Arc::new({
            let store =
                sisyphus_mem::RoadmapSqlStore::new(pool.clone()).await.expect("roadmap store");
            store.init_schema().await.expect("init roadmap schema");
            store
        }),
        Arc::new(sisyphus_roadmap::proposal_manager::ProposalManager::new(
            state_store.clone(),
        )),
    ));

    StrategicManager::new(
        intelligence_manager,
        execution_manager,
        config,
        Arc::new(crate::AgentChannel::new()),
        benchmark,
        meta_learner,
        Arc::new(MockExperienceBank),
        knowledge,
        roadmap,
        intervention,
        internalization,
        None,
    )
}

// ---------------------------------------------------------------------------
// 零真实产出的硬失败拦截（AllIterationsPivoted）
// ---------------------------------------------------------------------------

/// 恒定返回指定偏差分的反射器，用来把 supervisor 的 pivot 判定变成确定行为。
///
/// 偏差阈值在 `setup_manager` 中是 `convergence_threshold = 0.01`，
/// 因此返回值 >0.01 即稳定触发 `SupervisorDecision::Pivot`。
struct ScriptedReflector(f64);
#[async_trait::async_trait]
impl sisyphus_intel::reflect::Reflector for ScriptedReflector {
    async fn reflect(
        &self,
        _trace: &str,
        _predicted: &sisyphus_intel::SimulationResult,
        _experiences: Option<&[sisyphus_types::experience::ExperienceTuple]>,
    ) -> Result<sisyphus_intel::reflect::ReflectionReport, sisyphus_types::IntelError> {
        Ok(sisyphus_intel::reflect::ReflectionReport {
            original_sop_id: "scripted".into(),
            deviation_score: self.0,
            suggested_improvements: vec![],
            improvement_signals: vec![],
            evolved_sop_structure: None,
            records: vec![],
        })
    }
}

/// 构造一个只差反射器与仿真概率的 StrategicManager。
///
/// 与 `setup_manager` 同构，区别有二：
/// 1. 注入脚本化偏差分，从而能精确控制 supervisor 是否触发 StrategicPivot；
/// 2. `success_probability` 固定为 0.1，低于 `SisyphusSupervisor` 的
///    快速放行门槛（`>= 0.5`）。若输出被判定为健康且概率达标，
///    supervisor 会直接返回 `Continue{deviation: 0.05}` 并**跳过反射器**，
///    脚本化偏差分将完全不起作用。
async fn setup_manager_with_reflector(deviation: f64) -> StrategicManager {
    let simulator = Arc::new(MockSimulator::new(vec![0.1]));
    let intel = Arc::new(sisyphus_intel::SisyphusIntel::new(
        simulator,
        Arc::new(MockLogicalSimulator),
        Arc::new(ScriptedReflector(deviation)),
        Arc::new(MockEvolver),
    ));

    let llm: Arc<dyn sisyphus_tools_llm::LlmClient> = Arc::new(sisyphus_tools_llm::DummyLlmClient);
    let synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer> = Arc::new(
        sisyphus_intel::GoalDrivenSynthesizer::new(Arc::new(MockToolExecutor)),
    );

    let intelligence_manager = Arc::new(IntelligenceManager::new(intel, synthesizer, llm.clone()));

    let state_store = Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("in-memory MfsStore"),
    );
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.expect("sqlite pool");
    let sop_store =
        Arc::new(sisyphus_mem::SopSqlStore::new(pool.clone()).await.expect("sop store"));
    let knowledge = Arc::new(sisyphus_knowledge::AssetManager::new(
        std::path::PathBuf::from(".omo/test_knowledge_pivot"),
    ));
    let intervention = Arc::new(crate::intervention_manager::InterventionManager::new());
    let internalization = Arc::new(crate::internalization_manager::InternalizationManager::new(
        llm.clone(),
        knowledge.clone(),
    ));
    let execution_manager = Arc::new(ExecutionManager::new(
        state_store.clone(),
        sop_store,
        Arc::new(MockToolExecutor),
        Arc::new(MockTestExecutor),
        intervention.clone(),
        internalization.clone(),
        None,
    ));

    let config = CognitiveLoopConfig {
        max_iterations: 1,
        convergence_threshold: 0.01,
        llm_stage_timeout_secs: 30,
        adaptive_depth: sisyphus_types::config::AdaptiveDepthConfig::default(),
        anomaly_detector: sisyphus_types::config::AnomalyDetectorConfig::default(),
        token_budget: sisyphus_types::config::TokenBudgetConfig::default(),
    };

    let benchmark = Arc::new(parking_lot::Mutex::new(
        sisyphus_tools_build::benchmark::ContinuousBenchmark::new(".omo/test_bench_pivot"),
    ));
    let meta_learner = Arc::new(parking_lot::Mutex::new(
        sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(".omo/test_meta_pivot"),
    ));
    let roadmap = Arc::new(sisyphus_roadmap::RoadmapManager::new(
        state_store.clone(),
        Arc::new({
            let store =
                sisyphus_mem::RoadmapSqlStore::new(pool.clone()).await.expect("roadmap store");
            store.init_schema().await.expect("init roadmap schema");
            store
        }),
        Arc::new(sisyphus_roadmap::proposal_manager::ProposalManager::new(
            state_store.clone(),
        )),
    ));

    StrategicManager::new(
        intelligence_manager,
        execution_manager,
        config,
        Arc::new(crate::AgentChannel::new()),
        benchmark,
        meta_learner,
        Arc::new(MockExperienceBank),
        knowledge,
        roadmap,
        intervention,
        internalization,
        None,
    )
}

/// 单节点成功图：执行器烘焙进节点，返回 `Ok("mock")`。
///
/// P0.1 的脚本化 reflector 用例依赖它，让偏差完全由注入的 reflector 决定。
fn single_node_sop() -> Arc<SOPGraph> {
    make_sop_graph(&SOPGraphConfig {
        nodes: vec![SOPNodeConfig {
            name: "n0".into(),
            kind: NodeKind::Wasm {
                tool_id: "t".into(),
                fuel_limit: 100,
            },
        }],
        edges: vec![],
    })
}

/// 单节点失败图，执行器**烘焙进节点**——这是让真实执行真正失败的唯一途径。
///
/// `SOPGraph::from_config` 会把执行器绑进每个 `WasmSOPNode`
/// （`sisyphus-types/src/graph.rs:263-268`），因此给 `ExecutionManager`
/// 换执行器对 `run_cognitive_loop` 无效，必须在这里建图。
fn failing_node_sop() -> Arc<SOPGraph> {
    SOPGraph::from_config(
        &SOPGraphConfig {
            nodes: vec![SOPNodeConfig {
                name: "n0".into(),
                kind: NodeKind::Wasm {
                    tool_id: "t".into(),
                    fuel_limit: 100,
                },
            }],
            edges: vec![],
        },
        Arc::new(FailingToolExecutor),
        Arc::new(MockTestExecutor),
    )
    .expect("failing graph from config")
}

/// 用**真实** `StandardReflector` 复验零产出守卫（非合成夹具）。
///
/// 覆盖动机：P0.1 的其余用例都用 `ScriptedReflector`/`SequencedReflector`
/// 直接返回脚本化偏差，属合成夹具——它锁的是「守卫读的那个数」，而不是
/// 「真实反思器会不会产出那个数」。本用例不注入任何脚本化 reflector，
/// 改用生产 `StandardReflector`，其偏差完全由真实 trace 推导。
///
/// 实际走通的链路（由变异验证反推得出，**不要按直觉写**）：
///
/// 1. `FailingToolExecutor` 执行失败 → 节点不写成功 trace，轨迹为空；
/// 2. `SisyphusSupervisor` 的 `output.indicates_failure()` 为真，
///    跳过快速通道，`build_trace(&ctx)` 得到空字符串；
/// 3. `StandardReflector::reflect` 走 `records.is_empty()` 兜底分支，
///    空串既不含 `Error` 也不含 `ResourceExhausted`，因此命中的是
///    **`else` 臂的 0.1**，而不是同一 `if` 的 0.8 臂；
/// 4. `effective_threshold = base * max(success_prob, 0.1)`，
///    `MockSimulator` 首次给 0.1，队列耗尽后回退 0.5；
///    探针实测两次调用的 base 均为 **0.01**（不是 Low 档的 0.2），
///    effective 分别为 0.001 与 0.005，0.1 > 二者均触发 `StrategicPivot`；
/// 5. 唯一迭代只产出 pivot 占位文本 → `AllIterationsPivoted`。
///
/// 变异证据（已实测，均使本用例失败）：
/// - 把 `else` 臂的 0.1 改成 0.0 → 失败，错误变为 `NodeExecutionFailed`；
/// - 让 `FailingToolExecutor` 改返回 `Ok` → 失败，`got Ok: "success"`。
///
/// 反之，把 0.8 臂改成 0.1、或把 `analyze()` 的偏差改坏，**本用例仍全绿**
/// ——说明这两处与本路径无关。这正是「测试通过 ≠ 测试有效」的实例。
#[tokio::test]
async fn test_all_pivoted_with_real_reflector_is_error() {
    let manager = setup_manager_with_real_reflector(".omo/test_knowledge_real_reflector").await;

    let err = match manager
        .run_cognitive_loop(
            failing_node_sop(),
            "test_real_reflector_pivot",
            None,
            None,
            None,
            None,
            |_id, _e| {},
            None,
            None,
        )
        .await
    {
        Ok((res, _, _)) => {
            panic!("real reflector must detect the failure and pivot, got Ok: {res}")
        },
        Err(e) => e,
    };

    assert!(
        matches!(err, CoreError::AllIterationsPivoted(..)),
        "expected AllIterationsPivoted from the real reflector path, got: {err:?}"
    );

    let msg = err.to_string();
    assert!(
        !msg.contains("SUCCESS"),
        "error message must not claim success: {msg}"
    );
}

/// 构造使用生产 `StandardReflector` 的 manager（不注入任何脚本化偏差）。
///
/// 与 `setup_manager_with_reflector` 的唯一区别是 reflector 实例；其余
/// 夹具（内存 store、DummyLlm、MockToolExecutor）保持一致，以便对比。
///
/// 注意：这里给 `ExecutionManager` 传 `FailingToolExecutor` 是**多余**的——
/// `run_cognitive_loop` 执行时用的是「调用方传入的图」，而图在
/// `SOPGraph::from_config` 时就把执行器绑进了每个 `WasmSOPNode`
/// （`sisyphus-types/src/graph.rs:263-268`）。真正生效的是
/// `failing_node_sop()` 建图时用的执行器。保留此处是为了让 manager
/// 自身与图保持一致。
async fn setup_manager_with_real_reflector(knowledge_dir: &str) -> StrategicManager {
    let simulator = Arc::new(MockSimulator::new(vec![0.1]));
    let intel = Arc::new(sisyphus_intel::SisyphusIntel::new(
        simulator,
        Arc::new(MockLogicalSimulator),
        Arc::new(sisyphus_intel::StandardReflector),
        Arc::new(MockEvolver),
    ));

    let llm: Arc<dyn sisyphus_tools_llm::LlmClient> = Arc::new(sisyphus_tools_llm::DummyLlmClient);
    let synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer> = Arc::new(
        sisyphus_intel::GoalDrivenSynthesizer::new(Arc::new(MockToolExecutor)),
    );

    let intelligence_manager = Arc::new(IntelligenceManager::new(intel, synthesizer, llm.clone()));

    let state_store = Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("in-memory MfsStore"),
    );
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.expect("sqlite pool");
    let sop_store =
        Arc::new(sisyphus_mem::SopSqlStore::new(pool.clone()).await.expect("sop store"));
    let knowledge = Arc::new(sisyphus_knowledge::AssetManager::new(
        std::path::PathBuf::from(knowledge_dir),
    ));
    let intervention = Arc::new(crate::intervention_manager::InterventionManager::new());
    let internalization = Arc::new(crate::internalization_manager::InternalizationManager::new(
        llm.clone(),
        knowledge.clone(),
    ));
    let execution_manager = Arc::new(ExecutionManager::new(
        state_store.clone(),
        sop_store,
        Arc::new(FailingToolExecutor),
        Arc::new(MockTestExecutor),
        intervention.clone(),
        internalization.clone(),
        None,
    ));

    let config = CognitiveLoopConfig {
        max_iterations: 2,
        convergence_threshold: 0.01,
        llm_stage_timeout_secs: 30,
        adaptive_depth: sisyphus_types::config::AdaptiveDepthConfig::default(),
        anomaly_detector: sisyphus_types::config::AnomalyDetectorConfig::default(),
        token_budget: sisyphus_types::config::TokenBudgetConfig::default(),
    };

    let benchmark = Arc::new(parking_lot::Mutex::new(
        sisyphus_tools_build::benchmark::ContinuousBenchmark::new(".omo/test_bench_real_refl"),
    ));
    let meta_learner = Arc::new(parking_lot::Mutex::new(
        sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(".omo/test_meta_real_refl"),
    ));
    let roadmap = Arc::new(sisyphus_roadmap::RoadmapManager::new(
        state_store.clone(),
        Arc::new({
            let store =
                sisyphus_mem::RoadmapSqlStore::new(pool.clone()).await.expect("roadmap store");
            store.init_schema().await.expect("init roadmap schema");
            store
        }),
        Arc::new(sisyphus_roadmap::proposal_manager::ProposalManager::new(
            state_store.clone(),
        )),
    ));

    StrategicManager::new(
        intelligence_manager,
        execution_manager,
        config,
        Arc::new(crate::AgentChannel::new()),
        benchmark,
        meta_learner,
        Arc::new(MockExperienceBank),
        knowledge,
        roadmap,
        intervention,
        internalization,
        None,
    )
}

/// 全部迭代都在执行期 pivot 时，必须返回 `AllIterationsPivoted` 而不是 Ok。
///
/// 回归动机（71b01ac）：此前 `execute_phase` 把 StrategicPivot 吞掉并构造
/// `"Strategic Pivot: ..."` 占位文本，`final_result` 非空，于是上层
/// `execute_sop_impl` 直接返回 `status="SUCCESS"`——所有节点都没真正执行成功
/// 却被伪装成成功。
#[tokio::test]
async fn test_all_iterations_pivoted_is_error_not_placeholder_success() {
    let manager = setup_manager_with_reflector(0.9).await;

    // SOPGraph 未实现 Debug，不能用 expect_err/expect（它们要求 Ok 类型: Debug），
    // 改为显式 match 取出错误。
    let err = match manager
        .run_cognitive_loop(
            single_node_sop(),
            "test_all_pivot",
            None,
            None,
            None,
            None,
            |_id, _e| {},
            None,
            None,
        )
        .await
    {
        Ok((res, _, _)) => panic!("every iteration pivoted, yet the loop returned Ok: {res}"),
        Err(e) => e,
    };

    assert!(
        matches!(err, CoreError::AllIterationsPivoted(..)),
        "expected AllIterationsPivoted, got: {err:?}"
    );

    let msg = err.to_string();
    assert!(
        !msg.contains("SUCCESS"),
        "error message must not claim success: {msg}"
    );
}

/// 偏差分低于阈值时执行真实成功，必须仍然返回 Ok。
///
/// 这是 `AllIterationsPivoted` 拦截的反向保护：若统计条件写错（例如把
/// 预测期的 pivot_reason 也算作「零真实执行」），合法执行会被误判为失败。
#[tokio::test]
async fn test_successful_execution_is_not_flagged_as_all_pivoted() {
    // 0.0 < convergence_threshold(0.01)，supervisor 放行，节点产出真实结果。
    let manager = setup_manager_with_reflector(0.0).await;

    let (res, report, _evolved) = manager
        .run_cognitive_loop(
            single_node_sop(),
            "test_no_pivot",
            None,
            None,
            None,
            None,
            |_id, _e| {},
            None,
            None,
        )
        .await
        .expect("legitimate execution must still succeed");

    assert!(
        !res.to_string().is_empty(),
        "successful execution must return a real result"
    );
    assert!(
        !res.to_string().contains("Strategic Pivot"),
        "successful execution must not return pivot placeholder text: {res}"
    );
    assert!(report.iterations >= 1);
}

/// 预测期「Logical Flaw Detected」不等于「零真实执行」。
///
/// `pivot_reason` 有两个来源：预测阶段的逻辑缺陷（第 723 行）与执行阶段的
/// StrategicPivot（第 855 行）。若统计条件误用 `pivot_reason` 而不是执行期
/// 信号，则一次「预测期发现缺陷、但执行阶段正常产出」的健康迭代会被判为
/// 失败——这是本用例要防的误伤。
#[tokio::test]
async fn test_prediction_phase_logical_flaw_is_not_counted_as_zero_execution() {
    let manager = setup_manager_with_critical_flaw_reflector(0.0).await;

    let (res, _report, _evolved) = manager
        .run_cognitive_loop(
            single_node_sop(),
            "test_predict_flaw",
            None,
            None,
            None,
            None,
            |_id, _e| {},
            None,
            None,
        )
        .await
        .expect("prediction-phase logical flaw must not be treated as zero real execution");

    assert!(
        !res.to_string().contains("Strategic Pivot"),
        "prediction-phase flaw must not produce pivot placeholder: {res}"
    );
}

/// 反射器按调用次序返回脚本化的偏差序列，用尽后重复最后一个值。
///
/// 用于构造「先 pivot、随后恢复」的多轮场景：单轮夹具无法表达
/// 「中途失败但整体成功」这条合法路径。
struct SequencedReflector {
    deviations: std::sync::Mutex<std::collections::VecDeque<f64>>,
}

impl SequencedReflector {
    fn new(deviations: impl IntoIterator<Item = f64>) -> Self {
        Self {
            deviations: std::sync::Mutex::new(deviations.into_iter().collect()),
        }
    }
}

#[async_trait::async_trait]
impl sisyphus_intel::reflect::Reflector for SequencedReflector {
    async fn reflect(
        &self,
        _trace: &str,
        _predicted: &sisyphus_intel::SimulationResult,
        _experiences: Option<&[sisyphus_types::experience::ExperienceTuple]>,
    ) -> Result<sisyphus_intel::reflect::ReflectionReport, sisyphus_types::IntelError> {
        let deviation = {
            let mut queue = self.deviations.lock().map_err(|_| {
                sisyphus_types::IntelError::Internal("reflector lock poisoned".into())
            })?;
            if queue.len() > 1 {
                // More scripted deviations remain: consume one but keep the tail
                // so later calls still have something to fall back on.
                queue.pop_front().unwrap_or(0.0)
            } else {
                // Last scripted value (or an empty sequence) repeats forever, so
                // extra iterations behave deterministically.
                queue.front().copied().unwrap_or(0.0)
            }
        };
        Ok(sisyphus_intel::reflect::ReflectionReport {
            original_sop_id: "sequenced".into(),
            deviation_score: deviation,
            suggested_improvements: vec![],
            improvement_signals: vec![],
            evolved_sop_structure: None,
            records: vec![],
        })
    }
}

/// 构造一个反射器按次序返回 `deviations`、迭代上限为 `max_iterations` 的 manager。
///
/// 与 `setup_manager_with_reflector` 同构，但支持多轮：偏差序列里较大的值
/// 会触发执行期 StrategicPivot，较小的值则放行，从而覆盖
/// 「中途 pivot 后恢复」这条合法路径。
async fn setup_manager_with_sequenced_reflector(
    deviations: impl IntoIterator<Item = f64>,
    max_iterations: u32,
    knowledge_dir: &str,
) -> StrategicManager {
    setup_manager_with_sequenced_reflector_and_logic(
        deviations,
        max_iterations,
        knowledge_dir,
        false,
    )
    .await
}

/// 与 `setup_manager_with_sequenced_reflector` 相同，但可让逻辑仿真报告一个
/// Critical 缺陷，从而使**预测期**的 `pivot_reason` 也被赋值。
///
/// 三个信号源必须能同时存在才能验证占位文本判定：
/// - 预测期 `pivot_reason`（Logical Flaw）
/// - 执行期 `exec_pivot`（StrategicPivot）
/// - 执行成功（`exec_pivot == false` 且 `pivot_reason.is_some()`）
///
/// 若夹具不报 flaw，则 `pivot_reason` 恒为 `None`，此时
/// `pivot_reason.is_some()` 与 `exec_pivot` 在行为上完全等价，
/// 占位判定的变异无法被任何用例发现。
async fn setup_manager_with_sequenced_reflector_and_logic(
    deviations: impl IntoIterator<Item = f64>,
    max_iterations: u32,
    knowledge_dir: &str,
    critical_flaw: bool,
) -> StrategicManager {
    let simulator = Arc::new(MockSimulator::new(vec![0.1]));
    let logic: Arc<dyn sisyphus_intel::simulation::LogicalSimulator> = if critical_flaw {
        Arc::new(CriticalFlawSimulator::default())
    } else {
        Arc::new(MockLogicalSimulator)
    };
    let intel = Arc::new(sisyphus_intel::SisyphusIntel::new(
        simulator,
        logic,
        Arc::new(SequencedReflector::new(deviations)),
        Arc::new(MockEvolver),
    ));

    let llm: Arc<dyn sisyphus_tools_llm::LlmClient> = Arc::new(sisyphus_tools_llm::DummyLlmClient);
    let synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer> = Arc::new(
        sisyphus_intel::GoalDrivenSynthesizer::new(Arc::new(MockToolExecutor)),
    );

    let intelligence_manager = Arc::new(IntelligenceManager::new(intel, synthesizer, llm.clone()));

    let state_store = Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("in-memory MfsStore"),
    );
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.expect("sqlite pool");
    let sop_store =
        Arc::new(sisyphus_mem::SopSqlStore::new(pool.clone()).await.expect("sop store"));
    let knowledge = Arc::new(sisyphus_knowledge::AssetManager::new(
        std::path::PathBuf::from(knowledge_dir),
    ));
    let intervention = Arc::new(crate::intervention_manager::InterventionManager::new());
    let internalization = Arc::new(crate::internalization_manager::InternalizationManager::new(
        llm.clone(),
        knowledge.clone(),
    ));
    let execution_manager = Arc::new(ExecutionManager::new(
        state_store.clone(),
        sop_store,
        Arc::new(MockToolExecutor),
        Arc::new(MockTestExecutor),
        intervention.clone(),
        internalization.clone(),
        None,
    ));

    let config = CognitiveLoopConfig {
        max_iterations,
        convergence_threshold: 0.01,
        llm_stage_timeout_secs: 30,
        adaptive_depth: sisyphus_types::config::AdaptiveDepthConfig::default(),
        anomaly_detector: sisyphus_types::config::AnomalyDetectorConfig::default(),
        token_budget: sisyphus_types::config::TokenBudgetConfig::default(),
    };

    let benchmark = Arc::new(parking_lot::Mutex::new(
        sisyphus_tools_build::benchmark::ContinuousBenchmark::new(".omo/test_bench_seq"),
    ));
    let meta_learner = Arc::new(parking_lot::Mutex::new(
        sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(".omo/test_meta_seq"),
    ));
    let roadmap = Arc::new(sisyphus_roadmap::RoadmapManager::new(
        state_store.clone(),
        Arc::new({
            let store =
                sisyphus_mem::RoadmapSqlStore::new(pool.clone()).await.expect("roadmap store");
            store.init_schema().await.expect("init roadmap schema");
            store
        }),
        Arc::new(sisyphus_roadmap::proposal_manager::ProposalManager::new(
            state_store.clone(),
        )),
    ));

    StrategicManager::new(
        intelligence_manager,
        execution_manager,
        config,
        Arc::new(crate::AgentChannel::new()),
        benchmark,
        meta_learner,
        Arc::new(MockExperienceBank),
        knowledge,
        roadmap,
        intervention,
        internalization,
        None,
    )
}

/// 中途 pivot、随后恢复并成功，必须仍然返回 Ok。
///
/// 回归动机（71b01ac）：`AllIterationsPivoted` 守卫的注释明确写着
/// 「中途 pivot 后恢复是合法路径」，但此前没有任何测试覆盖这条路径——
/// 只有「全部 pivot」和「全部成功」两个极端，守卫条件写错（例如写成
/// `exec_pivot == true` 即失败）不会被任何用例发现。
#[tokio::test]
async fn test_partial_pivot_then_recovery_still_succeeds() {
    // 第 1 轮偏差 0.9（> convergence_threshold）→ 执行期 StrategicPivot；
    // 第 2 轮偏差 0.0 → 放行并产出真实执行结果。
    let manager =
        setup_manager_with_sequenced_reflector([0.9, 0.0], 2, ".omo/test_knowledge_partial_pivot")
            .await;

    let (res, report, _evolved) = manager
        .run_cognitive_loop(
            single_node_sop(),
            "test_partial_pivot",
            None,
            None,
            None,
            None,
            |_id, _e| {},
            None,
            None,
        )
        .await
        .expect("a later real execution must make the loop succeed despite an earlier pivot");

    assert!(
        !res.to_string().is_empty(),
        "successful execution must return a real result"
    );
    assert!(
        !res.to_string().contains("Strategic Pivot"),
        "the final result must be the recovered execution, not the pivot placeholder: {res}"
    );
    assert_eq!(
        report.iterations, 2,
        "both iterations should have run: one pivoted, one recovered"
    );
}

/// 中途 pivot 后恢复时，`pivot_reason` 诊断信息不得出现在最终结果里。
///
/// 覆盖 `execute_phase` 占位文本判定的第二个来源：即使本轮是**成功**的
/// 执行，只要历史上曾发生执行期 pivot，最终返回的就必须是真实产出，
/// 而不是任何形式的 pivot 说明文本。
#[tokio::test]
async fn test_pivot_diagnostics_do_not_leak_into_recovered_result() {
    let manager =
        setup_manager_with_sequenced_reflector([0.9, 0.0], 2, ".omo/test_knowledge_pivot_leak")
            .await;

    let (res, _report, _evolved) = manager
        .run_cognitive_loop(
            single_node_sop(),
            "test_pivot_leak",
            None,
            None,
            None,
            None,
            |_id, _e| {},
            None,
            None,
        )
        .await
        .expect("recovery after pivot must succeed");

    let text = res.to_string();
    assert!(
        !text.contains("Strategic Pivot"),
        "pivot placeholder leaked into a recovered result: {text}"
    );
    assert!(
        !text.contains("Logical Flaw"),
        "prediction-phase diagnostics leaked into a recovered result: {text}"
    );
}

/// 预测期 flaw 与「中途 pivot 后恢复」同时出现时，占位判定必须只认执行期信号。
///
/// 这是本组用例里**唯一**能捕获占位判定变异的一个。实测证据：
/// 把 `execute_phase` 的判定从 `*exec_pivot` 改回 `pivot_reason.is_some()`
/// 后，本文件其余 4 个 pivot 用例**全部照常通过**——因为它们用
/// `MockLogicalSimulator`（不报 flaw），此时 `pivot_reason` 恒为 `None`，
/// `None.is_some()` 恒为 `false`，与 `exec_pivot` 行为完全一致。
///
/// 只有当预测期 `pivot_reason` 与执行期 `exec_pivot` **取值相反**时，
/// 两种写法才会产生可观测的差异：
/// - 第 2 轮：预测期报 flaw（`pivot_reason = Some`），但 supervisor 放行
///   （`exec_pivot = false`）→ 真实执行成功 → 必须返回真实产出。
#[tokio::test]
async fn test_prediction_flaw_plus_recovery_uses_execution_signal_only() {
    let manager = setup_manager_with_sequenced_reflector_and_logic(
        [0.9, 0.0],
        2,
        ".omo/test_knowledge_flaw_recovery",
        true,
    )
    .await;

    let (res, _report, _evolved) = manager
        .run_cognitive_loop(
            single_node_sop(),
            "test_flaw_recovery",
            None,
            None,
            None,
            None,
            |_id, _e| {},
            None,
            None,
        )
        .await
        .expect("recovery must succeed even with a prediction-phase flaw present");

    let text = res.to_string();
    assert!(
        !text.contains("Strategic Pivot"),
        "a successful iteration was replaced by pivot placeholder text: {text}"
    );
    assert!(
        !text.contains("Logical Flaw"),
        "prediction-phase diagnostics leaked into an executed result: {text}"
    );
    assert!(
        !text.is_empty(),
        "successful execution must return a real result, not diagnostics"
    );
}

/// 与 `setup_manager_with_reflector` 相同，但逻辑仿真报告一个 Critical 缺陷，
/// 从而在预测期把 `pivot_reason` 置为 `Logical Flaw Detected: ...`。
///
/// 执行期反射器返回 0.0，supervisor 放行，因此本轮确有真实执行结果。
async fn setup_manager_with_critical_flaw_reflector(deviation: f64) -> StrategicManager {
    let simulator = Arc::new(MockSimulator::new(vec![0.1]));
    let intel = Arc::new(sisyphus_intel::SisyphusIntel::new(
        simulator,
        Arc::new(CriticalFlawSimulator::default()),
        Arc::new(ScriptedReflector(deviation)),
        Arc::new(MockEvolver),
    ));

    let llm: Arc<dyn sisyphus_tools_llm::LlmClient> = Arc::new(sisyphus_tools_llm::DummyLlmClient);
    let synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer> = Arc::new(
        sisyphus_intel::GoalDrivenSynthesizer::new(Arc::new(MockToolExecutor)),
    );

    let intelligence_manager = Arc::new(IntelligenceManager::new(intel, synthesizer, llm.clone()));

    let state_store = Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("in-memory MfsStore"),
    );
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.expect("sqlite pool");
    let sop_store =
        Arc::new(sisyphus_mem::SopSqlStore::new(pool.clone()).await.expect("sop store"));
    let knowledge = Arc::new(sisyphus_knowledge::AssetManager::new(
        std::path::PathBuf::from(".omo/test_knowledge_flaw"),
    ));
    let intervention = Arc::new(crate::intervention_manager::InterventionManager::new());
    let internalization = Arc::new(crate::internalization_manager::InternalizationManager::new(
        llm.clone(),
        knowledge.clone(),
    ));
    let execution_manager = Arc::new(ExecutionManager::new(
        state_store.clone(),
        sop_store,
        Arc::new(MockToolExecutor),
        Arc::new(MockTestExecutor),
        intervention.clone(),
        internalization.clone(),
        None,
    ));

    let config = CognitiveLoopConfig {
        max_iterations: 1,
        convergence_threshold: 0.01,
        llm_stage_timeout_secs: 30,
        adaptive_depth: sisyphus_types::config::AdaptiveDepthConfig::default(),
        anomaly_detector: sisyphus_types::config::AnomalyDetectorConfig::default(),
        token_budget: sisyphus_types::config::TokenBudgetConfig::default(),
    };

    let benchmark = Arc::new(parking_lot::Mutex::new(
        sisyphus_tools_build::benchmark::ContinuousBenchmark::new(".omo/test_bench_flaw"),
    ));
    let meta_learner = Arc::new(parking_lot::Mutex::new(
        sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(".omo/test_meta_flaw"),
    ));
    let roadmap = Arc::new(sisyphus_roadmap::RoadmapManager::new(
        state_store.clone(),
        Arc::new({
            let store =
                sisyphus_mem::RoadmapSqlStore::new(pool.clone()).await.expect("roadmap store");
            store.init_schema().await.expect("init roadmap schema");
            store
        }),
        Arc::new(sisyphus_roadmap::proposal_manager::ProposalManager::new(
            state_store.clone(),
        )),
    ));

    StrategicManager::new(
        intelligence_manager,
        execution_manager,
        config,
        Arc::new(crate::AgentChannel::new()),
        benchmark,
        meta_learner,
        Arc::new(MockExperienceBank),
        knowledge,
        roadmap,
        intervention,
        internalization,
        None,
    )
}
