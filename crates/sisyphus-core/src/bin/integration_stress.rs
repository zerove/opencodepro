use sisyphus_core::{Executor, SOPGraph};
use sisyphus_sandbox::WasmToolExecutor;
use sisyphus_types::WasmSOPNode;
use std::sync::Arc;
use std::time::Instant;
use tokio::task::JoinSet;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting In-Process Integration Stress Test");

    // 1. Initialize Core Components
    let tool_executor =
        Arc::new(WasmToolExecutor::new().expect("Failed to create WasmToolExecutor"));

    // 2. Setup a Parallel DAG
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

    // 3. Stress Test: Concurrent Execution of the same DAG
    let concurrency = 50;
    let iterations = 20;
    let mut set = JoinSet::new();

    let start = Instant::now();
    for _ in 0..concurrency {
        let graph_ref = Arc::new(SOPGraph {
            graph: graph.graph.clone(),
            start_node: root,
        });
        let exec_ref = Executor::new();

        set.spawn(async move {
            let mut success = 0;
            let mut failure = 0;
            for _ in 0..iterations {
                // In a real scenario, we'd clone the graph structure
                // Here we just verify the executor can handle the concurrency
                match exec_ref
                    .execute(
                        &SOPGraph {
                            graph: graph_ref.graph.clone(),
                            start_node: root,
                        },
                        root,
                    )
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
