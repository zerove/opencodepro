//! DAG execution engine — dependency resolution, composite scheduling, and
//! cross-graph data flow.

use dashmap::DashMap;
use petgraph::algo::toposort;
use std::collections::HashMap;
use std::sync::Arc;

use sisyphus_types::{CompositeGraph, ExecutionContext, NodeOutput};

use crate::CoreError;

/// Output from a composite execution run.
pub struct CompositeOutput {
    /// sop_id -> (node_output, report_string) for each executed graph.
    pub results: HashMap<String, (NodeOutput, String)>,
    /// Order in which graphs were executed.
    pub execution_order: Vec<String>,
}

/// Resolves execution order for a CompositeGraph via topological sort
/// over the dependency graph.
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

    /// Given a CompositeGraph, return sop_ids in execution order.
    /// Uses petgraph to build a dependency DAG and performs topological sort.
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

        // Add edges: source -> target (source executes BEFORE target)
        for dep in &composite.dependencies {
            let src = node_map
                .get(&dep.source_sop_id)
                .ok_or(CoreError::MissingSourceNode)?;
            let tgt = node_map
                .get(&dep.target_sop_id)
                .ok_or(CoreError::MissingTargetNode)?;
            dep_graph.add_edge(*src, *tgt, ());
        }

        // Topological sort
        let order = toposort(&dep_graph, None).map_err(|_| CoreError::DependencyCycleDetected)?;

        Ok(order
            .into_iter()
            .filter_map(|idx| dep_graph.node_weight(idx).cloned())
            .filter(|id| composite.graphs.contains_key(id))
            .collect())
    }
}

/// Build an optional execution context populated with dependency outputs for a graph.
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
