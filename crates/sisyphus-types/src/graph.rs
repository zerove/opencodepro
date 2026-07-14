use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::node::{SOPNode, WasmSOPNode, RetrySOPNode};
use crate::execution::ExecutionContext;
use crate::TypesError;

/// A data mapping from a source graph node output to a target graph parameter.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DataMapping {
    pub source_node: String,
    pub target_placeholder: String,
}

/// Describes a dependency between two SOP graphs.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GraphDependency {
    pub source_sop_id: String,
    pub target_sop_id: String,
    pub mappings: Vec<DataMapping>,
}

/// A composite workflow: multiple SOP graphs with cross-graph dependencies.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CompositeGraph {
    pub graphs: HashMap<String, SOPGraphConfig>,
    pub dependencies: Vec<GraphDependency>,
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
    Branch {
        branches: std::collections::HashMap<String, usize>,
        default_next: usize,
    },
    SubGraph {
        config: Box<SOPGraphConfig>,
    },
    Test {
        command: String,
        workdir: Option<String>,
    },
    VisualAnalysis {
        prompt: String,
        image_source: String,
    },
    VisualAction {
        action_type: String,
        target_selector: String,
        params: Value,
    },
    HumanIntervention {
        prompt: String,
        timeout_secs: u64,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SOPNodeConfig {
    pub name: String,
    pub kind: NodeKind,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
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

    pub fn into_cognitive_plan(self, plan_id: String, goal: String) -> crate::execution::CognitivePlan {
        let steps = self.nodes.into_iter().enumerate().map(|(i, cfg)| {
            let action = match cfg.kind {
                NodeKind::Wasm { tool_id, .. } => crate::execution::CognitiveAction::ToolCall { tool_id, args: serde_json::json!({}) },
                NodeKind::Retry { .. } => crate::execution::CognitiveAction::ToolCall { tool_id: "retry_wrapper".into(), args: serde_json::json!({}) },
                NodeKind::Branch { .. } => crate::execution::CognitiveAction::ToolCall { tool_id: "branch_logic".into(), args: serde_json::json!({}) },
                NodeKind::SubGraph { .. } => crate::execution::CognitiveAction::ToolCall { tool_id: "subgraph_exec".into(), args: serde_json::json!({}) },
                NodeKind::Test { .. } => crate::execution::CognitiveAction::ToolCall { tool_id: "test_exec".into(), args: serde_json::json!({}) },
                _ => crate::execution::CognitiveAction::ToolCall { tool_id: "internal_handler".into(), args: serde_json::json!({}) },
            };
            crate::execution::CognitiveStep {
                id: i.to_string(),
                name: cfg.name,
                action,
                verification: None,
                priority: 0,
            }
        }).collect();

        let edges = self.edges.into_iter().map(|(s, t)| {
            crate::execution::PlanEdge {
                from: s.to_string(),
                to: t.to_string(),
            }
        }).collect();

        crate::execution::CognitivePlan {
            plan_id,
            goal,
            steps,
            edges,
            start_node_id: "0".to_string(),
        }
    }
}


pub struct SOPGraph {
    pub graph: DiGraph<Arc<dyn SOPNode>, ()>,
    pub start_node: NodeIndex,
    pub is_validated: AtomicBool,
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
            start_node: NodeIndex::new(0), // Note: this is a placeholder; must be updated by adding nodes
            is_validated: AtomicBool::new(false),
        }
    }

    pub fn from_config(
        config: &SOPGraphConfig,
        executor: Arc<dyn crate::node::ToolExecutor>,
        test_executor: Arc<dyn crate::node::TestExecutor>,
    ) -> Result<Arc<Self>, TypesError> {
        let mut graph = DiGraph::new();
        let mut node_indices = Vec::new();

        fn build_node(
            kind: &NodeKind,
            name: &str,
            executor: Arc<dyn crate::node::ToolExecutor>,
            test_executor: Arc<dyn crate::node::TestExecutor>,
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
                    child: build_node(child, name, executor, test_executor),
                    max_retries: *max_retries,
                }),
                NodeKind::Branch { branches, default_next } => Arc::new(crate::node::BranchSOPNode {
                    name: name.to_string(),
                    branches: branches.clone(),
                    default_next: *default_next,
                }),
                NodeKind::SubGraph { config } => Arc::new(crate::node::SubGraphSOPNode {
                    name: name.to_string(),
                    config: config.as_ref().clone(),
                }),
                NodeKind::Test { command, workdir } => Arc::new(crate::node::TestSOPNode {
                    name: name.to_string(),
                    test_command: command.clone(),
                    workdir: workdir.clone(),
                    test_executor,
                }),
                NodeKind::VisualAnalysis { prompt, image_source } => Arc::new(crate::node::VisualSOPNode::new(
                    name.to_string(),
                    NodeKind::VisualAnalysis {
                        prompt: prompt.clone(),
                        image_source: image_source.clone(),
                    },
                )),
                NodeKind::VisualAction { action_type, target_selector, params } => Arc::new(crate::node::VisualSOPNode::new(
                    name.to_string(),
                    NodeKind::VisualAction {
                        action_type: action_type.clone(),
                        target_selector: target_selector.clone(),
                        params: params.clone(),
                    },
                )),
                NodeKind::HumanIntervention { prompt, timeout_secs } => Arc::new(crate::node::HumanInterventionSOPNode {
                    name: name.to_string(),
                    prompt: prompt.clone(),
                    timeout_secs: *timeout_secs,
                }),
            }
        }

        for node_cfg in &config.nodes {
            let node = build_node(&node_cfg.kind, &node_cfg.name, executor.clone(), test_executor.clone());
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
            graph.add_edge(src, tgt, ());
        }

        Ok(Arc::new(SOPGraph {
            graph,
            start_node: node_indices.first().cloned().ok_or_else(|| {
                TypesError::GraphError("Graph config must contain at least one node".into())
            })?,
            is_validated: AtomicBool::new(false),
        }))
    }

    pub fn add_node(&mut self, node: Arc<dyn SOPNode>) -> NodeIndex {
        self.graph.add_node(node)
    }

    pub fn add_edge(&mut self, from: NodeIndex, to: NodeIndex) {
        self.graph.add_edge(from, to, ());
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SopEvaluation {
    pub score: f32,
    pub metrics: HashMap<String, f32>,
    pub feedback: String,
    pub is_acceptable: bool,
}

#[async_trait::async_trait]
pub trait SopEvaluator: Send + Sync {
    async fn evaluate(&self, goal: &str, config: &SOPGraphConfig) -> Result<SopEvaluation, TypesError>;
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum MicroReflectionDecision {
    Continue,
    Adjust { updates: HashMap<String, Value> },
    Pivot { reason: String },
}

#[async_trait::async_trait]
pub trait MicroReflector: Send + Sync {
    async fn predict(
        &self,
        node_name: &str,
        ctx: &ExecutionContext,
    ) -> Result<String, TypesError>;

    async fn reflect(
        &self,
        node_name: &str,
        prediction: &str,
        actual: &str,
        ctx: &ExecutionContext,
    ) -> Result<MicroReflectionDecision, TypesError>;
}
