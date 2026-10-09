//! 未恢复节点失败的独立回归测试。无沙箱、外部服务或真实工具命令。
//! 修改前单节点失败被包装为成功上下文，这里的失败断言用于锁定该缺陷。
//! 测试尚需获得独占测试时段后运行，不能以源码审阅替代实际复现。

use async_trait::async_trait;
use sisyphus_core::{CoreError, ExecutionContext, ExecutionEvent, Executor, SOPGraph};
use sisyphus_types::{
    BranchSOPNode, NodeOutput, RetrySOPNode, RuntimeSupervisor, SupervisorDecision, ToolExecutor,
    TypesError, WasmSOPNode,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

struct Tools {
    failures_left: AtomicUsize,
    calls: AtomicUsize,
}

impl Tools {
    fn new(failures: usize) -> Arc<Self> {
        Arc::new(Self {
            failures_left: AtomicUsize::new(failures),
            calls: AtomicUsize::new(0),
        })
    }
}

fn tool_failure() -> TypesError {
    TypesError::ToolExecutionError {
        tool_id: "fixture".into(),
        details: "intentional failure".into(),
    }
}

#[async_trait]
impl ToolExecutor for Tools {
    async fn execute(
        &self,
        _tool_id: &str,
        _args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, TypesError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self
            .failures_left
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(1)
            })
            .is_ok()
        {
            Err(tool_failure())
        } else {
            Ok(b"ok".to_vec())
        }
    }
}

fn node(name: &str, tools: Arc<Tools>) -> Arc<WasmSOPNode> {
    Arc::new(WasmSOPNode {
        name: name.into(),
        tool_id: "fixture".into(),
        fuel_limit: 100,
        executor: tools,
        args: Vec::new(),
    })
}

fn executor(dir: &tempfile::TempDir) -> Executor {
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

fn assert_tool_failure(error: CoreError, expected_node: &str) {
    assert!(
        std::error::Error::source(&error).is_some(),
        "必须保留原始错误来源"
    );
    match error {
        CoreError::NodeExecutionFailed { node_name, source } => {
            assert_eq!(node_name, expected_node);
            assert!(
                matches!(*source, TypesError::ToolExecutionError { tool_id, details }
                if tool_id == "fixture" && details == "intentional failure")
            );
        },
        other => panic!("期望结构化节点失败，实际：{other:?}"),
    }
}

async fn single_node_failure(streaming: bool) {
    let dir = tempfile::tempdir().expect("临时目录");
    let tools = Tools::new(usize::MAX);
    let mut graph = SOPGraph::new();
    let start = graph.add_node(node("failed", tools.clone()));
    let exec = executor(&dir);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let result = if streaming {
        exec.execute_with_context(
            Arc::new(graph),
            start,
            None,
            Some(tx),
            tools.clone(),
            None,
            None,
            None,
        )
        .await
    } else {
        drop(tx);
        exec.execute(Arc::new(graph), start, tools.clone()).await
    };
    assert_tool_failure(result.expect_err("失败工具不得返回成功上下文"), "failed");
    // 默认测量执行失败后会回退普通执行，既有重试次数不变。
    assert_eq!(tools.calls.load(Ordering::SeqCst), 2);
    if streaming {
        let mut failed = 0;
        while let Some(event) = rx.recv().await {
            match event {
                ExecutionEvent::NodeFailed {
                    node_name, error, ..
                } => {
                    assert_eq!(node_name, "failed");
                    assert!(error.contains("intentional failure"));
                    failed += 1;
                },
                ExecutionEvent::NodeCompleted { .. } => panic!("失败节点不得发送完成事件"),
                _ => {},
            }
        }
        assert_eq!(failed, 1);
    }
}

#[tokio::test]
async fn failed_tool_is_error_without_stream() {
    single_node_failure(false).await;
}

#[tokio::test]
async fn failed_tool_is_error_with_stream() {
    single_node_failure(true).await;
}

#[tokio::test]
async fn successful_tools_still_complete_in_both_modes() {
    for streaming in [false, true] {
        let dir = tempfile::tempdir().expect("临时目录");
        let tools = Tools::new(0);
        let mut graph = SOPGraph::new();
        let start = graph.add_node(node("first", tools.clone()));
        let end = graph.add_node(node("last", tools.clone()));
        graph.add_edge(start, end);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let ctx = executor(&dir)
            .execute_with_context(
                Arc::new(graph),
                start,
                None,
                streaming.then_some(tx),
                tools.clone(),
                None,
                None,
                None,
            )
            .await
            .expect("正常链路成功");
        assert_eq!(
            ctx.get_result("last").expect("末节点输出").as_str(),
            Some("ok")
        );
        assert_eq!(tools.calls.load(Ordering::SeqCst), 2);
        let mut completed = 0;
        while let Some(event) = rx.recv().await {
            match event {
                ExecutionEvent::NodeCompleted { .. } => completed += 1,
                ExecutionEvent::NodeFailed { .. } => panic!("成功节点不得发送失败事件"),
                _ => {},
            }
        }
        assert_eq!(completed, if streaming { 2 } else { 0 });
    }
}

#[tokio::test]
async fn unrecovered_failure_does_not_schedule_normal_successor() {
    let dir = tempfile::tempdir().expect("临时目录");
    let failing = Tools::new(usize::MAX);
    let downstream = Tools::new(0);
    let mut graph = SOPGraph::new();
    let start = graph.add_node(node("failed", failing.clone()));
    let next = graph.add_node(node("downstream", downstream.clone()));
    graph.add_edge(start, next);
    let result = executor(&dir).execute(Arc::new(graph), start, failing).await;
    assert_tool_failure(result.expect_err("普通后继不构成恢复"), "failed");
    assert_eq!(downstream.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn successor_failure_is_not_hidden_by_successful_root() {
    let dir = tempfile::tempdir().expect("临时目录");
    let tools = Tools::new(0);
    let mut graph = SOPGraph::new();
    let start = graph.add_node(node("first", tools.clone()));
    let failed = graph.add_node(node("failed", Tools::new(usize::MAX)));
    graph.add_edge(start, failed);
    let result = executor(&dir).execute(Arc::new(graph), start, tools).await;
    assert_tool_failure(result.expect_err("后继失败必须传播"), "failed");
}

#[tokio::test]
async fn retry_and_measurement_fallback_can_recover() {
    for failures in [1, 2] {
        let dir = tempfile::tempdir().expect("临时目录");
        let tools = Tools::new(failures);
        let mut graph = SOPGraph::new();
        let start = graph.add_node(Arc::new(RetrySOPNode {
            name: "retry".into(),
            child: node("child", tools.clone()),
            max_retries: 1,
        }));
        let ctx = executor(&dir)
            .execute(Arc::new(graph), start, tools.clone())
            .await
            .expect("测量回退或节点重试成功仍应成功");
        assert_eq!(
            ctx.get_result("retry").expect("重试输出").as_str(),
            Some("ok")
        );
        assert_eq!(tools.calls.load(Ordering::SeqCst), failures + 1);
    }
}

#[tokio::test]
async fn exhausted_retry_is_an_error() {
    let dir = tempfile::tempdir().expect("临时目录");
    let tools = Tools::new(usize::MAX);
    let mut graph = SOPGraph::new();
    let start = graph.add_node(Arc::new(RetrySOPNode {
        name: "retry".into(),
        child: node("child", tools.clone()),
        max_retries: 1,
    }));
    let result = executor(&dir).execute(Arc::new(graph), start, tools.clone()).await;
    assert!(matches!(
        result,
        Err(CoreError::NodeExecutionFailed { node_name, source })
            if node_name == "retry"
                && matches!(*source, TypesError::NodeExecutionError(_))
    ));
    assert_eq!(tools.calls.load(Ordering::SeqCst), 4);
}

async fn branch_recovery(recovery_fails: bool) {
    let dir = tempfile::tempdir().expect("临时目录");
    let failing = Tools::new(usize::MAX);
    let recovered = Tools::new(if recovery_fails { usize::MAX } else { 0 });
    let unselected = Tools::new(usize::MAX);
    let mut graph = SOPGraph::new();
    let start = graph.add_node(node("failed", failing.clone()));
    let branch = graph.add_node(Arc::new(BranchSOPNode {
        name: "branch".into(),
        branches: HashMap::from([(tool_failure().to_string(), 2)]),
        default_next: 3,
    }));
    let recovery = graph.add_node(node("recovery", recovered.clone()));
    let skipped = graph.add_node(node("skipped", unselected.clone()));
    graph.add_edge(start, branch);
    graph.add_edge(branch, recovery);
    graph.add_edge(branch, skipped);
    let result = executor(&dir).execute(Arc::new(graph), start, failing).await;
    if recovery_fails {
        assert_tool_failure(result.expect_err("恢复节点失败不得成功"), "recovery");
    } else {
        let ctx = result.expect("既有显式错误分支仍可恢复");
        assert_eq!(
            ctx.get_result("recovery").expect("恢复输出").as_str(),
            Some("ok")
        );
        assert_eq!(
            ctx.get_result("failed").expect("分支兼容输入").as_str(),
            Some(tool_failure().to_string().as_str())
        );
    }
    assert!(recovered.calls.load(Ordering::SeqCst) > 0);
    assert_eq!(unselected.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn explicit_error_branch_can_recover() {
    branch_recovery(false).await;
}

#[tokio::test]
async fn failed_recovery_branch_still_fails() {
    branch_recovery(true).await;
}

struct Supervisor(bool);

#[async_trait]
impl RuntimeSupervisor for Supervisor {
    async fn evaluate(
        &self,
        _node_name: &str,
        _output: &NodeOutput,
        _ctx: Arc<ExecutionContext>,
    ) -> Result<SupervisorDecision, TypesError> {
        if self.0 {
            Ok(SupervisorDecision::Pivot {
                reason: "recover through evolution".into(),
                suggested_correction: None,
            })
        } else {
            Ok(SupervisorDecision::Continue { deviation: 0.0 })
        }
    }
}

#[tokio::test]
async fn supervisor_continue_cannot_erase_failure_but_pivot_is_preserved() {
    for pivot in [false, true] {
        let dir = tempfile::tempdir().expect("临时目录");
        let tools = Tools::new(usize::MAX);
        let mut graph = SOPGraph::new();
        let start = graph.add_node(node("failed", tools.clone()));
        let result = executor(&dir)
            .execute_with_context(
                Arc::new(graph),
                start,
                None,
                None,
                tools,
                Some(Arc::new(Supervisor(pivot))),
                None,
                None,
            )
            .await;
        if pivot {
            assert!(
                matches!(result, Err(CoreError::StrategicPivot(reason)) if reason == "recover through evolution")
            );
        } else {
            assert_tool_failure(result.expect_err("监督器继续不代表工具已恢复"), "failed");
        }
    }
}

// 复用只读夹具，不引用或修改 src/tests.rs。
mod common;

struct Prediction;

#[async_trait]
impl sisyphus_intel::Simulator for Prediction {
    async fn simulate(
        &self,
        _graph: Arc<SOPGraph>,
        _ctx: &ExecutionContext,
        _project: &str,
        _sop: &str,
        _goal: Option<&str>,
    ) -> Result<sisyphus_intel::SimulationResult, sisyphus_types::IntelError> {
        Ok(sisyphus_intel::SimulationResult {
            path_id: "fixture".into(),
            success_probability: 1.0,
            estimated_cost: 1,
            predicted_stability: 1.0,
            predicted_effort: 1,
            predicted_outcome: "ok".into(),
        })
    }
}

fn fixture_config() -> sisyphus_types::SOPGraphConfig {
    sisyphus_types::SOPGraphConfig {
        nodes: vec![sisyphus_types::SOPNodeConfig {
            name: "fixture-node".into(),
            kind: sisyphus_types::NodeKind::Wasm {
                tool_id: "fixture".into(),
                fuel_limit: 100,
                args: Vec::new(),
            },
        }],
        edges: vec![],
    }
}

struct ScriptedLlm;

#[async_trait]
impl sisyphus_tools_llm::LlmClient for ScriptedLlm {
    async fn generate(&self, _prompt: &str) -> Result<String, TypesError> {
        Ok(r#"{"critical_findings": [], "risk_score": 0.0, "suggested_refinement": "none", "should_proceed": true}"#.into())
    }

    async fn chat(&self, _system: &str, user: &str, _stream: bool) -> Result<String, TypesError> {
        if user.contains("review this proposal") {
            Ok(r#"{"approved": true, "feedback": "ok"}"#.into())
        } else {
            serde_json::to_string(&fixture_config())
                .map_err(|e| TypesError::InternalError(e.to_string()))
        }
    }

    fn get_params_for(&self, _agent: &str) -> (String, String, String) {
        // 此夹具直接实现所有调用，不连接任何服务或读取密钥。
        (String::new(), String::new(), String::new())
    }

    fn breaker(&self) -> sisyphus_tools_llm::circuit_breaker::CircuitBreaker {
        sisyphus_tools_llm::circuit_breaker::CircuitBreaker::default()
    }
}

#[derive(Default)]
struct Experiences(parking_lot::Mutex<Vec<sisyphus_types::experience::ExperienceTuple>>);

#[async_trait]
impl sisyphus_types::experience::ExperienceBank for Experiences {
    async fn record(
        &self,
        experience: sisyphus_types::experience::ExperienceTuple,
    ) -> Result<(), TypesError> {
        self.0.lock().push(experience);
        Ok(())
    }

    async fn query_similar(
        &self,
        _query: sisyphus_types::experience::ExperienceQuery,
    ) -> Result<Vec<sisyphus_types::experience::ExperienceTuple>, TypesError> {
        Ok(vec![])
    }
}

async fn kernel_fixture(
    dir: &tempfile::TempDir,
    tools: Arc<Tools>,
    experiences: Arc<Experiences>,
) -> sisyphus_core::SisyphusKernel {
    let state = Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("内存状态库"),
    );
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.expect("内存数据库");
    let roadmap = sisyphus_mem::RoadmapSqlStore::new(pool.clone()).await.expect("路线图库");
    roadmap.init_schema().await.expect("路线图表结构");
    let llm = Arc::new(ScriptedLlm);
    let mut kernel = sisyphus_core::SisyphusKernel::new(
        state,
        Arc::new(sisyphus_mem::SopSqlStore::new(pool.clone()).await.expect("流程库")),
        Arc::new(sisyphus_mem::VectorMemoryStore::new(pool).await.expect("向量库")),
        experiences,
        tools,
        Arc::new(sisyphus_intel::SisyphusIntel::new(
            Arc::new(Prediction),
            Arc::new(common::MockLogicalSimulator),
            Arc::new(common::MockReflector),
            Arc::new(common::MockEvolver),
        )),
        llm.clone(),
        llm,
        Arc::new(common::MockSynthesizer),
        Arc::new(parking_lot::Mutex::new(
            sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(
                dir.path().to_str().expect("临时目录编码"),
            ),
        )),
        Arc::new(common::MockConsensus),
        Arc::new(roadmap),
        Arc::new(sisyphus_tools_llm::DummyEmbeddingClient),
    )
    .with_cognitive_loop_config(sisyphus_types::config::CognitiveLoopConfig {
        max_iterations: 1,
        ..Default::default()
    });
    kernel.micro_reflector = None;
    kernel
}

#[tokio::test]
async fn goal_entrypoints_preserve_failure_and_success_semantics() {
    for streaming in [false, true] {
        for fails in [false, true] {
            let dir = tempfile::tempdir().expect("临时目录");
            let tools = Tools::new(if fails { usize::MAX } else { 0 });
            let kernel =
                kernel_fixture(&dir, tools.clone(), Arc::new(Experiences::default())).await;
            let (tx, mut events) = mpsc::unbounded_channel();
            let (stream_tx, mut cognition) = mpsc::unbounded_channel();
            let result = if streaming {
                kernel
                    .execute_goal_streaming(
                        "fixture goal",
                        "fixture-sop",
                        "fixture-project",
                        Some(tx),
                        Some(stream_tx),
                    )
                    .await
            } else {
                drop(stream_tx);
                kernel
                    .execute_goal(
                        "fixture goal",
                        "fixture-sop",
                        "fixture-project",
                        Some(tx),
                        None,
                    )
                    .await
            };
            assert!(
                tools.calls.load(Ordering::SeqCst) > 0,
                "必须抵达工具执行，而非在准备阶段失败"
            );
            if fails {
                assert_tool_failure(
                    result.expect_err("目标入口必须传播工具错误"),
                    "fixture-node",
                );
            } else {
                assert_eq!(result.expect("正常目标成功").0.as_str(), Some("ok"));
            }
            let mut failed = 0;
            let mut completed = 0;
            // 认知循环使用后台轨迹记录器，以限时收至通道关闭避免事件竞态。
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while let Some(event) = events.recv().await {
                    match event {
                        ExecutionEvent::NodeFailed { .. } => failed += 1,
                        ExecutionEvent::NodeCompleted { .. } => completed += 1,
                        _ => {},
                    }
                }
            })
            .await
            .expect("轨迹记录器应关闭转发通道");
            assert_eq!(failed, usize::from(fails));
            assert_eq!(completed, usize::from(!fails));
            let mut execute = 0;
            let mut complete = 0;
            while let Some(event) = cognition.recv().await {
                match event {
                    sisyphus_types::CognitiveEvent::Execute { .. } => execute += 1,
                    sisyphus_types::CognitiveEvent::Complete { .. } => complete += 1,
                    _ => {},
                }
            }
            let expected = usize::from(streaming && !fails);
            assert_eq!(execute, expected, "失败不得包装成正常认知执行结果");
            assert_eq!(complete, expected, "失败不得发送认知成功完成事件");
        }
    }
}

#[tokio::test]
async fn records_real_failure_and_success_in_outcome_and_experience() {
    for fails in [false, true] {
        let dir = tempfile::tempdir().expect("临时目录");
        let tools = Tools::new(if fails { usize::MAX } else { 0 });
        let experiences = Arc::new(Experiences::default());
        let kernel = kernel_fixture(&dir, tools.clone(), experiences.clone()).await;
        kernel
            .roadmap_manager
            .create_project("fixture-project", "fixture", "fixture")
            .await
            .expect("创建测试项目");
        let result = kernel
            .execute_and_record(
                "fixture goal",
                "fixture-sop",
                "fixture-project",
                "milestone",
                "phase",
                "goal",
                None,
            )
            .await;
        assert!(tools.calls.load(Ordering::SeqCst) > 0);
        if fails {
            assert_tool_failure(
                result.expect_err("记录失败不能改变返回错误"),
                "fixture-node",
            );
        } else {
            assert_eq!(result.expect("正常执行记录成功").0.as_str(), Some("ok"));
        }
        let outcomes =
            kernel.roadmap_manager.get_outcomes("fixture-project").await.expect("读取结果");
        let outcome = outcomes.iter().find(|o| o.goal_id == "goal").expect("持久化执行结果");
        assert_eq!(outcome.success, !fails);
        assert_eq!(outcome.error.is_some(), fails);
        assert_eq!(outcome.deviation_score.is_none(), fails);
        if fails {
            assert!(outcome.error.as_deref().expect("失败详情").contains("intentional failure"));
        }
        let recorded = experiences.0.lock();
        let experience = recorded.iter().find(|e| e.action_id == "fixture-sop").expect("经验记录");
        assert_eq!(experience.outcome_success, !fails);
        assert_eq!(experience.project_id.as_deref(), Some("fixture-project"));
    }
}
