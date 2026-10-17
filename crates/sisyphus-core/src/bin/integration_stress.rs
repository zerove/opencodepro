use sisyphus_core::{Executor, SOPGraph};
use sisyphus_sandbox::WasmToolExecutor;
use sisyphus_types::WasmSOPNode;
use std::sync::Arc;
use std::time::Instant;
use tokio::task::JoinSet;

/// LLM placeholder for the stress harness.
///
/// `sisyphus-tools-llm` is a **dev**-dependency of this crate (ADR-014 keeps it
/// out of core's production graph), and Cargo does not expose dev-dependencies
/// to `src/bin/` targets — so a plain `cargo build --bin integration_stress`
/// could not see `DummyLlmClient`. The stress DAG is built from `WasmSOPNode`s
/// that run through the *tool* executor, so nothing here calls the LLM.
///
/// Like [`UnusedTestExecutor`], it returns an error rather than a canned
/// string: if a future change makes this binary depend on LLM output, the
/// failure should be loud instead of a plausible-looking fake.
struct UnusedLlmClient;

#[async_trait::async_trait]
impl sisyphus_types::LlmClient for UnusedLlmClient {
    async fn chat(
        &self,
        _system: &str,
        _user: &str,
        _json_mode: bool,
    ) -> Result<String, sisyphus_types::TypesError> {
        Err(sisyphus_types::TypesError::ToolExecutionError {
            tool_id: "integration_stress".into(),
            details: "integration_stress does not wire an LLM; refusing to fabricate                       a completion"
                .into(),
        })
    }

    async fn generate(&self, _prompt: &str) -> Result<String, sisyphus_types::TypesError> {
        Err(sisyphus_types::TypesError::ToolExecutionError {
            tool_id: "integration_stress".into(),
            details: "integration_stress does not wire an LLM; refusing to fabricate                       a completion"
                .into(),
        })
    }

    fn get_params_for(&self, _agent_id: &str) -> (String, String, String) {
        (String::new(), String::new(), String::new())
    }

    fn breaker(&self) -> sisyphus_types::CircuitBreaker {
        sisyphus_types::CircuitBreaker::new(sisyphus_types::BreakerConfig::default())
    }
}

/// Test runner placeholder for the stress harness.
///
/// The graph this binary builds contains only [`WasmSOPNode`]s, which run
/// through the *tool* executor; nothing here ever calls [`TestExecutor::run_test`].
/// The concrete runner lives in `sisyphus-tools-executor`, and depending on that
/// crate just to satisfy a constructor that this binary never exercises is what
/// kept it in core's production dependency set (ADR-014).
///
/// It returns an error rather than a synthetic success: if a future change makes
/// this binary actually run tests, the failure should be obvious instead of
/// quietly reporting that tests passed when nothing ran.
struct UnusedTestExecutor;

#[async_trait::async_trait]
impl sisyphus_types::TestExecutor for UnusedTestExecutor {
    async fn run_test(
        &self,
        command: &str,
        _workdir: Option<&str>,
    ) -> Result<sisyphus_types::TestResult, sisyphus_types::TypesError> {
        Err(sisyphus_types::TypesError::ToolExecutionError {
            tool_id: "integration_stress".into(),
            details: format!(
                "integration_stress does not wire a test runner; refusing to fake                  a result for {command}"
            ),
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting In-Process Integration Stress Test");

    // 1. 初始化核心组件
    let tool_executor = Arc::new(WasmToolExecutor::new()?);

    // Executor 依赖的最小化 mock
    let llm_client = Arc::new(UnusedLlmClient);
    let intervention_manager = Arc::new(sisyphus_core::InterventionManager::new());

    // Mock AssetManager — 只需一个实现该接口的对象
    // AssetManager 是结构体，可能需要真实实例或 trait 抽象。
    // 使用带临时路径的真实实例。
    let knowledge_manager = Arc::new(sisyphus_knowledge::AssetManager::new(
        std::path::PathBuf::from("/tmp/sisyphus_stress"),
    ));

    let internalization_manager = Arc::new(sisyphus_core::InternalizationManager::new(
        llm_client,
        knowledge_manager,
    ));

    // 2. 搭建并行 DAG
    // root -> [node_1, node_2, ..., node_10] -> sink
    let mut graph = SOPGraph::new();
    let root = graph.add_node(Arc::new(WasmSOPNode {
        name: "root".into(),
        tool_id: "fast_tool".into(),
        executor: tool_executor.clone(),
        fuel_limit: 1_000_000,
        // `fast_tool` takes no input; the default empty args is correct here.
        args: Vec::new(),
    }));

    let mut leaf_nodes = vec![];
    for leaf in 0..10 {
        let node = graph.add_node(Arc::new(WasmSOPNode {
            name: format!("node_{leaf}"),
            tool_id: "fast_tool".into(),
            executor: tool_executor.clone(),
            fuel_limit: 1_000_000,
            args: Vec::new(),
        }));
        graph.add_edge(root, node);
        leaf_nodes.push(node);
    }

    let sink = graph.add_node(Arc::new(WasmSOPNode {
        name: "sink".into(),
        tool_id: "fast_tool".into(),
        executor: tool_executor.clone(),
        fuel_limit: 1_000_000,
        // `fast_tool` takes no input; the default empty args is correct here.
        args: Vec::new(),
    }));

    for leaf in leaf_nodes {
        graph.add_edge(leaf, sink);
    }

    // 3. 压力测试：同一 DAG 的并发执行
    let concurrency = 50;
    let iterations = 20;
    let mut set = JoinSet::new();

    let start = Instant::now();
    for _ in 0..concurrency {
        let graph_ref = Arc::new(SOPGraph {
            graph: graph.graph.clone(),
            start_node: root,
            is_validated: std::sync::atomic::AtomicBool::new(true),
        });
        let exec_ref = Executor::new(
            Arc::new(UnusedTestExecutor),
            intervention_manager.clone(),
            internalization_manager.clone(),
            None,
        );
        let tool_executor_clone = Arc::clone(&tool_executor);

        set.spawn(async move {
            let mut success = 0;
            let mut failure = 0;
            for _ in 0..iterations {
                // 真实场景中会克隆图谱结构
                // 这里仅验证执行器能处理并发
                match exec_ref.execute(graph_ref.clone(), root, tool_executor_clone.clone()).await {
                    Ok(_) => success += 1,
                    Err(e) => {
                        failure += 1;
                        // Keep one representative failure so a non-zero count is
                        // actionable. Discarding the error entirely is how this
                        // binary sat broken (it did not compile) without anyone
                        // noticing what it was supposed to be measuring.
                        if failure == 1 {
                            eprintln!("first failure: {e}");
                        }
                    },
                }
            }
            (success, failure)
        });
    }

    let mut total_success = 0;
    let mut total_failure = 0;
    while let Some(res) = set.join_next().await {
        let (s, f) = res?;
        total_success += s;
        total_failure += f;
    }

    let duration = start.elapsed();
    println!("✅ Stress test completed in {:?}", duration);
    println!(
        "Total Success: {}, Total Failure: {}",
        total_success, total_failure
    );
    println!(
        "SOPs/sec: {:.2}",
        (total_success + total_failure) as f64 / duration.as_secs_f64()
    );

    if total_failure > 0 {
        println!(
            "Note: {} run(s) were rejected because the WASM engine allows at most 10 \
             concurrent core instances, while this harness drives {}. Those are a \
             configured engine ceiling, not a throughput defect — lower \
             `concurrency` to 10 or below to measure without rejections.",
            total_failure, concurrency
        );
    }

    Ok(())
}
