//! DAG 执行引擎 — 依赖解析、复合调度与跨图谱数据流。
//! 跨图谱数据流。

use dashmap::DashMap;
use petgraph::algo::toposort;
use std::collections::HashMap;
use std::sync::Arc;

use sisyphus_types::{CompositeGraph, ExecutionContext, NodeOutput};

use crate::CoreError;

/// 复合执行运行的输出。
pub struct CompositeOutput {
    /// 每个已执行图谱的 sop_id -> (node_output, report_string)。
    pub results: HashMap<String, (NodeOutput, String)>,
    /// 图谱的执行顺序。
    pub execution_order: Vec<String>,
}

/// 通过对依赖图进行拓扑排序，解析 CompositeGraph 的执行顺序。
/// 依赖图。
pub struct DependencyResolver;

impl Default for DependencyResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl DependencyResolver {
    pub fn new() -> Self {
        Self
    }

    /// 给定 CompositeGraph，按执行顺序返回 sop_ids。
    /// 使用 petgraph 构建依赖 DAG 并执行拓扑排序。
    pub fn resolve(composite: &CompositeGraph) -> Result<Vec<String>, CoreError> {
        let mut dep_graph = petgraph::graph::DiGraph::<String, ()>::new();
        let estimated_nodes = composite.graphs.len() + composite.dependencies.len();
        let mut node_map: HashMap<String, _> = HashMap::with_capacity(estimated_nodes);
        for sop_id in composite.graphs.keys() {
            if !node_map.contains_key(sop_id) {
                let idx = dep_graph.add_node(sop_id.clone());
                node_map.insert(sop_id.clone(), idx);
            }
        }
        for dep in &composite.dependencies {
            if !node_map.contains_key(&dep.source_sop_id) {
                let idx = dep_graph.add_node(dep.source_sop_id.clone());
                node_map.insert(dep.source_sop_id.clone(), idx);
            }
            if !node_map.contains_key(&dep.target_sop_id) {
                let idx = dep_graph.add_node(dep.target_sop_id.clone());
                node_map.insert(dep.target_sop_id.clone(), idx);
            }
        }

        // 添加边：source -> target（source 先于 target 执行）
        for dep in &composite.dependencies {
            let src = node_map
                .get(&dep.source_sop_id)
                .ok_or(CoreError::MissingSourceNode)?;
            let tgt = node_map
                .get(&dep.target_sop_id)
                .ok_or(CoreError::MissingTargetNode)?;
            dep_graph.add_edge(*src, *tgt, ());
        }

        // 拓扑排序
        let order = toposort(&dep_graph, None).map_err(|_| CoreError::DependencyCycleDetected)?;

        Ok(order
            .into_iter()
            .filter_map(|idx| dep_graph.node_weight(idx).cloned())
            .filter(|id| composite.graphs.contains_key(id))
            .collect())
    }
}

/// 为图谱构建填充了依赖输出的可选执行上下文。
pub(crate) fn build_dep_ctx_for(
    sop_id: &str,
    composite: &CompositeGraph,
    accumulated: &DashMap<String, NodeOutput>,
) -> Option<Arc<ExecutionContext>> {
    let ctx = ExecutionContext::new();
    let mut has_deps = false;
    for dep in &composite.dependencies {
        if dep.target_sop_id == sop_id {
            for mapping in &dep.mappings {
                if let Some(output) = accumulated.get(&mapping.source_node) {
                    ctx.set_result(mapping.target_placeholder.clone(), output.clone());
                    has_deps = true;
                }
            }
        }
    }
    if has_deps {
        Some(Arc::new(ctx))
    } else {
        None
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod property_tests;
