use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::node::{SOPNode, WasmSOPNode, RetrySOPNode};
use crate::TypesError;

/// A data mapping from a source graph node output to a target graph parameter.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DataMapping {
    /// Node name in the source graph whose output is used.
    pub source_node: String,
    /// Target placeholder (e.g. "{{source.output}}") that will be interpolated
    /// into the target graph's execution context.
    pub target_placeholder: String,
}

/// Describes a dependency between two SOP graphs.
/// Graph B (target_sop_id) depends on Graph A (source_sop_id).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GraphDependency {
    pub source_sop_id: String,
    pub target_sop_id: String,
    /// How source node outputs map into the target graph's context.
    pub mappings: Vec<DataMapping>,
}

/// A composite workflow: multiple SOP graphs with cross-graph dependencies.
/// The kernel executes graphs in topological order, injecting dependency
/// outputs into each graph's ExecutionContext before execution.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CompositeGraph {
    pub graphs: HashMap<String, SOPGraphConfig>,
    pub dependencies: Vec<GraphDependency>,
    /// Ordered list of sop_ids to execute (resolved by DependencyResolver).
    /// Empty means "resolve from dependencies".
    pub execution_order: Vec<String>,
}

impl CompositeGraph {
    pub fn new() -> Self {
        Self {
            graphs: HashMap::new(),
            dependencies: Vec::new(),
            execution_order: Vec::new(),
        }
    }

    pub fn add_graph(&mut self, sop_id: &str, config: SOPGraphConfig) {
        self.graphs.insert(sop_id.to_string(), config);
    }

    pub fn add_dependency(&mut self, dep: GraphDependency) {
        self.dependencies.push(dep);
    }
}

impl Default for CompositeGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum NodeKind {
    Wasm {
        tool_id: String,
        fuel_limit: u64,
    },
    Retry {
        child: Box<NodeKind>,
        max_retries: u32,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SOPNodeConfig {
    pub name: String,
    pub kind: NodeKind,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SOPGraphConfig {
    pub nodes: Vec<SOPNodeConfig>,
    pub edges: Vec<(usize, usize)>,
}

impl SOPGraphConfig {
    pub fn from_graph(graph: &SOPGraph) -> Self {
        let mut nodes = Vec::new();
        let mut node_indices = HashMap::new();

        for idx in graph.graph.node_indices() {
            let node = graph
                .graph
                .node_weight(idx)
                .expect("node_indices yields valid indices");
            nodes.push(SOPNodeConfig {
                name: node.name().to_string(),
                kind: node.to_kind(),
            });
            node_indices.insert(idx, nodes.len() - 1);
        }

        let mut edges = Vec::new();
        for edge in graph.graph.edge_indices() {
            let (source, target) = graph
                .graph
                .edge_endpoints(edge)
                .expect("edge_indices yields valid endpoints");
            if let (Some(&s), Some(&t)) = (node_indices.get(&source), node_indices.get(&target)) {
                edges.push((s, t));
            }
        }

        Self { nodes, edges }
    }
}

#[derive(Clone)]
pub struct SOPGraph {
    pub graph: DiGraph<Arc<dyn SOPNode>, ()>,
}

impl Default for SOPGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl SOPGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
        }
    }

    pub fn from_config(config: &SOPGraphConfig, executor: Arc<dyn crate::node::ToolExecutor>) -> Result<Self, TypesError> {
        let mut graph = SOPGraph::new();
        let mut node_indices = Vec::new();

        fn build_node(
            kind: &NodeKind,
            name: &str,
            executor: Arc<dyn crate::node::ToolExecutor>,
        ) -> Arc<dyn SOPNode> {
            match kind {
                NodeKind::Wasm {
                    tool_id,
                    fuel_limit,
                } => Arc::new(WasmSOPNode {
                    name: name.to_string(),
                    tool_id: tool_id.clone(),
                    fuel_limit: *fuel_limit,
                    executor,
                }),
                NodeKind::Retry { child, max_retries } => Arc::new(RetrySOPNode {
                    name: name.to_string(),
                    child: build_node(child, name, executor),
                    max_retries: *max_retries,
                }),
            }
        }

        for node_cfg in &config.nodes {
            let node = build_node(&node_cfg.kind, &node_cfg.name, executor.clone());
            node_indices.push(graph.add_node(node));
        }

        let node_count = node_indices.len();
        for &(source, target) in &config.edges {
            let &src = node_indices.get(source).ok_or_else(|| {
                TypesError::GraphError(format!(
                    "edge source index {source} out of bounds (node count: {node_count})"
                ))
            })?;
            let &tgt = node_indices.get(target).ok_or_else(|| {
                TypesError::GraphError(format!(
                    "edge target index {target} out of bounds (node count: {node_count})"
                ))
            })?;
            graph.add_edge(src, tgt);
        }

        Ok(graph)
    }

    pub fn add_node(&mut self, node: Arc<dyn SOPNode>) -> NodeIndex {
        self.graph.add_node(node)
    }

    pub fn add_edge(&mut self, from: NodeIndex, to: NodeIndex) {
        self.graph.add_edge(from, to, ());
    }
}
