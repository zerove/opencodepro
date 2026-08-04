use sisyphus_core::{Executor, SOPGraph};
use sisyphus_sandbox::WasmToolExecutor;
use sisyphus_types::WasmSOPNode;
use std::sync::Arc;
use std::time::Instant;
use tokio::task::JoinSet;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting In-Process Integration Stress Test");

    // 1. 初始化核心组件
    let tool_executor =
        Arc::new(WasmToolExecutor::new().expect("Failed to create WasmToolExecutor"));

    // Executor 依赖的最小化 mock
    let llm_client = Arc::new(sisyphus_tools_llm::DummyLlmClient);
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
    }));

    let mut leaf_nodes = vec![];
    for leaf in 0..10 {
        let node = graph.add_node(Arc::new(WasmSOPNode {
            name: format!("node_{leaf}"),
            tool_id: "fast_tool".into(),
            executor: tool_executor.clone(),
            fuel_limit: 1_000_000,
        }));
        graph.add_edge(root, node);
        leaf_nodes.push(node);
    }

    let sink = graph.add_node(Arc::new(WasmSOPNode {
        name: "sink".into(),
        tool_id: "fast_tool".into(),
        executor: tool_executor.clone(),
        fuel_limit: 1_000_000,
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
            Arc::new(sisyphus_tools_executor::executor::test_cmds::TestExecutor::new()),
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
                match exec_ref
                    .execute(graph_ref.clone(), root, tool_executor_clone.clone())
                    .await
                {
                    Ok(_) => success += 1,
                    Err(_) => failure += 1,
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

    Ok(())
}
