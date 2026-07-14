//! Remaining lib-level tests (non-DAG).

use super::*;
use sisyphus_types::{
    NodeKind, NodeOutput, SOPGraphConfig, SOPNodeConfig,
};
use std::collections::HashMap;

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
fn test_core_error_from_string() {
    let err = CoreError::CycleDetected;
    let s: String = err.into();
    assert_eq!(s, "SOPGraph contains cycles");
}

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
    async fn run_test(&self, _command: &str, _workdir: Option<&str>) -> Result<sisyphus_types::TestResult, sisyphus_types::TypesError> {
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

#[tokio::test]
async fn test_executor_execute_cycle() {
    let executor = Executor::new();

    // Create a config with a cycle: Node 0 -> Node 1 -> Node 0
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

    let graph = SOPGraph::from_config(&config, Arc::new(MockToolExecutor), Arc::new(MockTestExecutor)).unwrap();
    let res = executor
        .execute(&graph, petgraph::graph::NodeIndex::new(0))
        .await;

    assert!(matches!(res, Err(CoreError::CycleDetected)));
}

#[test]
fn test_struct_creation() {
    // CognitiveIteration
    let iter = CognitiveIteration {
        iteration: 1,
        prediction: sisyphus_intel::SimulationResult {
            path_id: "p1".into(),
            success_probability: 0.8,
            estimated_cost: 100u64,
            predicted_outcome: "ok".into(),
        },
        deviation_score: 0.1,
        suggestions: vec!["s1".into()],
        evolved: true,
    };
    assert_eq!(iter.iteration, 1);
    assert_eq!(iter.deviation_score, 0.1);

    // CognitiveReport
    let report = CognitiveReport {
        iterations: 1,
        converged: true,
        final_deviation: 0.1,
        history: vec![iter],
    };
    assert_eq!(report.iterations, 1);
    assert!(report.converged);

    // CompositeOutput
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
    let executor = Executor::new();

    // Simple DAG: n0 -> n1
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

    let graph = SOPGraph::from_config(&config, Arc::new(MockToolExecutor), Arc::new(MockTestExecutor)).unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();

    let res = executor
        .execute_with_context(&graph, petgraph::graph::NodeIndex::new(0), None, Some(tx))
        .await;

    assert!(res.is_ok());
    let ctx = res.unwrap();

    // Verify results are stored
    assert!(ctx.get_result("n0").is_some());
    assert!(ctx.get_result("n1").is_some());

    // Verify events were sent
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }

    // Expect NodeStarted and NodeCompleted for both nodes
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
    let executor = Executor::new();

    // Diamond: n0 -> [n1, n2] -> n3
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

    let graph = SOPGraph::from_config(&config, Arc::new(MockToolExecutor), Arc::new(MockTestExecutor)).unwrap();
    let res = executor
        .execute(&graph, petgraph::graph::NodeIndex::new(0))
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
    let executor = Executor::new();

    // Mock executor that fails on n0
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
                Err(sisyphus_types::TypesError::ToolExecutionError { tool_id: tool_id.to_string(), details: "intentional failure".into() })
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

    let graph = SOPGraph::from_config(&config, Arc::new(FailOnFirst), Arc::new(MockTestExecutor)).unwrap();
    let (tx, _rx) = mpsc::unbounded_channel();

    let res = executor
        .execute_with_context(&graph, petgraph::graph::NodeIndex::new(0), None, Some(tx))
        .await;

    assert!(res.is_ok());
    let ctx = res.unwrap();

    // Failed node stores error as Value::String
    let n0_out = ctx.get_result("n0").unwrap();
    assert_eq!(
        n0_out.0,
        serde_json::Value::String(
            sisyphus_types::TypesError::ToolExecutionError {
                tool_id: "fail".to_string(),
                details: "intentional failure".to_string(),
            }
            .to_string()
        )
    );

    // n1 still executes because the executor continues after failures
    let n1_out = ctx.get_result("n1").unwrap();
    assert_eq!(n1_out.0, serde_json::Value::String("ok".into()));
}

#[tokio::test]
async fn test_executor_empty_graph() {
    let executor = Executor::new();
    let config = SOPGraphConfig {
        nodes: vec![],
        edges: vec![],
    };
    let graph = SOPGraph::from_config(&config, Arc::new(MockToolExecutor), Arc::new(MockTestExecutor)).unwrap();
    let res = executor
        .execute(&graph, petgraph::graph::NodeIndex::new(0))
        .await;
    assert!(res.is_ok());
    let ctx = res.unwrap();
    assert!(ctx.get_result("nonexistent").is_none());
}

async fn build_test_kernel() -> SisyphusKernel {
    let state_store = Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:")
            .await
            .expect("in-memory MfsStore"),
    );
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(MockToolExecutor);

    let intel = Arc::new(sisyphus_intel::SisyphusIntel::new(
        Arc::new(sisyphus_intel::MonteCarloSimulator::new(1)),
        Arc::new(sisyphus_intel::StandardReflector),
        Arc::new(sisyphus_intel::TopologicalSOPEvolver::new()),
    ));

    let synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer> = Arc::new(
        sisyphus_intel::GoalDrivenSynthesizer::new(tool_executor.clone()),
    );

    SisyphusKernel::new(state_store, tool_executor, intel, Arc::new(sisyphus_tools_llm::DummyLlmClient), synthesizer)
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
    let graph = SOPGraph::from_config(&config, Arc::new(MockToolExecutor), Arc::new(MockTestExecutor)).unwrap();

    kernel.save_sop("test_roundtrip", "default", &graph).await.unwrap();
    let loaded = kernel.load_sop("test_roundtrip").await.unwrap();

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
async fn test_kernel_save_load_empty_graph() {
    let kernel = build_test_kernel().await;

    let config = SOPGraphConfig {
        nodes: vec![],
        edges: vec![],
    };
    let graph = SOPGraph::from_config(&config, Arc::new(MockToolExecutor), Arc::new(MockTestExecutor)).unwrap();

    kernel.save_sop("test_empty", "default", &graph).await.unwrap();
    let loaded = kernel.load_sop("test_empty").await.unwrap();

    assert_eq!(loaded.graph.node_count(), 0);
}

#[tokio::test]
async fn test_kernel_load_nonexistent() {
    let kernel = build_test_kernel().await;

    match kernel.load_sop("nonexistent_sop").await {
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

#[tokio::test]
async fn test_strategic_manager_evolution_verification() {
    use crate::strategic_manager::*;
    use sisyphus_types::{SOPGraphConfig, SOPNodeConfig, NodeKind};
    use std::sync::Arc;
    use parking_lot::Mutex;
    use sisyphus_types::config::CognitiveLoopConfig;
    use std::collections::VecDeque;

    struct MockSimulator {
        probs: Mutex<VecDeque<f64>>,
    }
    impl MockSimulator {
        fn new(p: Vec<f64>) -> Self {
            Self { probs: Mutex::new(VecDeque::from(p)) }
        }
    }
    #[async_trait::async_trait]
    impl sisyphus_intel::Simulator for MockSimulator {
        async fn simulate(&self, _g: Arc<SOPGraph>, _ctx: &ExecutionContext, _id: &str, _goal: Option<&str>) -> Result<sisyphus_intel::SimulationResult, String> {
            let p = self.probs.lock().pop_front().unwrap_or(0.5);
            Ok(sisyphus_intel::SimulationResult {
                path_id: "p1".into(),
                success_probability: p,
                estimated_cost: 100,
                predicted_outcome: "ok".into(),
                predicted_stability: 1.0,
            })
        }
    }

    struct MockEvolver;
    #[async_trait::async_trait]
    impl sisyphus_intel::Evolver for MockEvolver {
        async fn evolve(&self, g: Arc<SOPGraph>, _ref: &sisyphus_intel::reflect::ReflectionReport, _pred: &sisyphus_intel::SimulationResult, _ins: Option<&str>) -> Result<Arc<SOPGraph>, String> {
            Ok(Arc::clone(g))
        }
    }

    struct MockReflector;
    #[async_trait::async_trait]
    impl sisyphus_intel::Reflector for MockReflector {
        async fn reflect(&self, _trace: &str, _pred: &sisyphus_intel::SimulationResult) -> Result<sisyphus_intel::reflect::ReflectionReport, String> {
            Ok(sisyphus_intel::reflect::ReflectionReport {
                deviation_score: 0.5, // Force evolution
                suggested_improvements: vec!["improve".into()],
                original_sop_id: "test".into(),
                evolved_sop_structure: None,
                records: vec![],
            })
        }
    }

    async fn setup_manager(probs: Vec<f64>) -> StrategicManager {
        let simulator = Arc::new(MockSimulator::new(probs));
        let intel = Arc::new(sisyphus_intel::SisyphusIntel::new(
            simulator,
            Arc::new(MockReflector),
            Arc::new(MockEvolver),
        ));
        
        let intelligence_manager = Arc::new(IntelligenceManager::new(intel));
        let execution_manager = Arc::new(ExecutionManager::new(Arc::new(sisyphus_tools_executor::ExecutorRouter::new())));
        
        let config = CognitiveLoopConfig {
            max_iterations: 1,
            convergence_threshold: 0.01,
        };
        
        StrategicManager::new(
            intelligence_manager,
            execution_manager,
            config,
            Arc::new(crate::AgentChannel::new()),
            Arc::new(Mutex::new(crate::ContinuousBenchmark::default())),
            Arc::new(Mutex::new(crate::MetaLearner::default())),
            Arc::new(sisyphus_types::experience::InMemoryExperienceBank::new()),
            Arc::new(sisyphus_roadmap::RoadmapManager::default()),
        )
    }

    // Case 1: Evolution Accepted (0.5 -> 0.8)
    {
        let manager = setup_manager(vec![0.5, 0.8]).await;
        let config_sop = SOPGraphConfig {
            nodes: vec![SOPNodeConfig { name: "n0".into(), kind: NodeKind::Wasm { tool_id: "t".into(), fuel_limit: 100 } }],
            edges: vec![],
        };
        let sop_graph = Arc::new(SOPGraph::from_config(&config_sop, Arc::new(MockToolExecutor), Arc::new(MockTestExecutor)).unwrap());
        
        let (res, report, evolved) = manager.run_cognitive_loop(
            sop_graph, "test_acc", None, None, |_id, _e| {}, None
        ).await.unwrap();
        
        assert!(evolved.is_some(), "Evolution should be accepted when probability increases");
        assert!(report.iterations >= 1);
    }

    // Case 2: Evolution Rejected (0.5 -> 0.3)
    {
        let manager = setup_manager(vec![0.5, 0.3]).await;
        let config_sop = SOPGraphConfig {
            nodes: vec![SOPNodeConfig { name: "n0".into(), kind: NodeKind::Wasm { tool_id: "t".into(), fuel_limit: 100 } }],
            edges: vec![],
        };
        let sop_graph = Arc::new(SOPGraph::from_config(&config_sop, Arc::new(MockToolExecutor), Arc::new(MockTestExecutor)).unwrap());
        
        let (res, report, evolved) = manager.run_cognitive_loop(
            sop_graph, "test_rej", None, None, |_id, _e| {}, None
        ).await.unwrap();
        
        assert!(evolved.is_none(), "Evolution should be rejected when probability decreases");
    }
}
