//! 其余 lib 级测试（非 DAG）。

use super::*;
use sisyphus_types::{NodeKind, NodeOutput, SOPGraphConfig, SOPNodeConfig};
use std::collections::HashMap;
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
async fn test_executor_node_failure_stored_as_value() {
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
    let (tx, _rx) = mpsc::unbounded_channel();

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

    assert!(res.is_ok());
    let ctx = res.unwrap();

    // 失败节点将错误存储为 Value::String
    let n0_out = ctx.get_result("n0").unwrap();
    assert_eq!(
        n0_out.0,
        Arc::new(serde_json::Value::String(
            sisyphus_types::TypesError::ToolExecutionError {
                tool_id: "fail".to_string(),
                details: "intentional failure".to_string(),
            }
            .to_string()
        ))
    );

    // n1 仍会执行，因为执行器在失败后继续
    let n1_out = ctx.get_result("n1").unwrap();
    assert_eq!(n1_out.0, Arc::new(serde_json::Value::String("ok".into())));
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
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("sqlite pool");
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
        Arc::new(
            sisyphus_mem::SopSqlStore::new(pool.clone())
                .await
                .expect("sop store"),
        ),
        Arc::new(
            sisyphus_mem::VectorMemoryStore::new(pool.clone())
                .await
                .expect("vector store"),
        ),
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
        Arc::new(
            sisyphus_mem::RoadmapSqlStore::new(pool)
                .await
                .expect("roadmap store"),
        ),
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

    kernel
        .save_sop("test_roundtrip", "default", &graph)
        .await
        .unwrap();
    let loaded = kernel.load_sop("test_roundtrip", "default").await.unwrap();

    assert_eq!(loaded.graph.node_count(), 2);
    let names: Vec<String> = loaded
        .graph
        .node_weights()
        .map(|n| n.name().to_string())
        .collect();
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
    kernel
        .save_sop("test_empty", "default", &graph)
        .await
        .unwrap();

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
        }
        Ok(_) => panic!("expected SopNotFound error"),
    }
}

// ---------------------------------------------------------------------------
// 模拟阶段验证：战略管理器接受演进
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_strategic_manager_evolution_verification() {
    use crate::strategic_manager::*;
    use sisyphus_types::config::CognitiveLoopConfig;
    use sisyphus_types::{NodeKind, SOPGraphConfig, SOPNodeConfig};
    use std::collections::VecDeque;

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

    async fn setup_manager(probs: Vec<f64>) -> StrategicManager {
        let simulator = Arc::new(MockSimulator::new(probs));
        let intel = Arc::new(sisyphus_intel::SisyphusIntel::new(
            simulator,
            Arc::new(MockLogicalSimulator),
            Arc::new(MockReflector),
            Arc::new(MockEvolver),
        ));

        let llm: Arc<dyn sisyphus_tools_llm::LlmClient> =
            Arc::new(sisyphus_tools_llm::DummyLlmClient);
        let synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer> = Arc::new(
            sisyphus_intel::GoalDrivenSynthesizer::new(Arc::new(MockToolExecutor)),
        );

        let intelligence_manager =
            Arc::new(IntelligenceManager::new(intel, synthesizer, llm.clone()));

        let state_store = Arc::new(
            sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
                .await
                .expect("in-memory MfsStore"),
        );
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("sqlite pool");
        let sop_store = Arc::new(
            sisyphus_mem::SopSqlStore::new(pool.clone())
                .await
                .expect("sop store"),
        );
        let knowledge = Arc::new(sisyphus_knowledge::AssetManager::new(
            std::path::PathBuf::from(".omo/test_knowledge"),
        ));
        let intervention = Arc::new(crate::intervention_manager::InterventionManager::new());
        let internalization =
            Arc::new(crate::internalization_manager::InternalizationManager::new(
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
            critique_timeout_secs: 30,
            adaptive_depth: sisyphus_types::config::AdaptiveDepthConfig::default(),
        };

        let benchmark = Arc::new(parking_lot::Mutex::new(
            sisyphus_tools_build::benchmark::ContinuousBenchmark::new(".omo/test_bench"),
        ));
        let meta_learner = Arc::new(parking_lot::Mutex::new(
            sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(".omo/test_meta"),
        ));
        let roadmap = Arc::new(sisyphus_roadmap::RoadmapManager::new(
            state_store.clone(),
            Arc::new(
                sisyphus_mem::RoadmapSqlStore::new(pool)
                    .await
                    .expect("roadmap store"),
            ),
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
    let result = mgr
        .critique_reflection(&report, &prediction, 1)
        .await
        .unwrap();
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
    let result = mgr
        .critique_reflection(&report, &prediction, 5)
        .await
        .unwrap();
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
    let result = mgr
        .critique_reflection(&report, &prediction, 0)
        .await
        .unwrap();
    assert!(start.elapsed() < std::time::Duration::from_millis(50));
    assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
    assert!(result.suggested_refinement.contains("disabled"));
}
