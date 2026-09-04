//! 示例 2：构造 CompositeGraph（SOP 注册表），演示跨 SOP 依赖 + 拓扑序。
//!
//! 运行：
//!   cargo run -p sisyphus-core --example sop_graph
//!
//! 期望输出：
//!   "CompositeGraph with 3 SOPs: [ingest, analyze, report]"
//!   "Topological order: [ingest, analyze, report]"

use sisyphus_types::graph::{CompositeGraph, GraphDependency, SOPGraphConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 创建空的 SOP 注册表
    let mut registry = CompositeGraph::new();

    // 2. 注册 3 个 SOP（空配置：每个含零节点）
    registry.add_graph("ingest", SOPGraphConfig::default());
    registry.add_graph("analyze", SOPGraphConfig::default());
    registry.add_graph("report", SOPGraphConfig::default());

    eprintln!(
        "[INFO] CompositeGraph registered SOPs: {:?}",
        registry.graphs.keys().collect::<Vec<_>>()
    );

    // 3. 定义跨 SOP 依赖（ingest → analyze → report）
    registry.add_dependency(GraphDependency {
        source_sop_id: "ingest".to_string(),
        target_sop_id: "analyze".to_string(),
        mappings: vec![],
    });
    registry.add_dependency(GraphDependency {
        source_sop_id: "analyze".to_string(),
        target_sop_id: "report".to_string(),
        mappings: vec![],
    });
    eprintln!(
        "[INFO] added {} cross-SOP dependencies",
        registry.dependencies.len()
    );

    // 4. 拓扑序由 CompositeGraph 计算后存放在 execution_order 字段
    eprintln!("[INFO] topological order: {:?}", registry.execution_order);

    // 5. 序列化为 JSON 便于查看完整结构
    let json = serde_json::to_string_pretty(&registry)?;
    println!("\n--- CompositeGraph (JSON) ---\n{json}");

    Ok(())
}
