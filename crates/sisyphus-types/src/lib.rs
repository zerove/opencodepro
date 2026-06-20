use async_trait::async_trait;
use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub type NodeOutput = Value;

// ---------------------------------------------------------------------------
// Cross-graph dependency types
// ---------------------------------------------------------------------------

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

#[async_trait]
pub trait StateStore: Send + Sync {
    async fn get_state_json(&self, key: &str) -> Result<Option<serde_json::Value>, String>;
    async fn set_state_json(&self, key: &str, value: serde_json::Value) -> Result<(), String>;
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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HistoryRecord {
    pub timestamp: u64,
    pub trace: String,
    pub deviation_score: f64,
    pub success: bool,
    pub cost: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ExecutionHistory {
    pub sop_id: String,
    pub records: Vec<HistoryRecord>,
}

impl SOPGraphConfig {
    pub fn from_graph(graph: &SOPGraph) -> Self {
        let mut nodes = Vec::new();
        let mut node_indices = HashMap::new();

        for idx in graph.graph.node_indices() {
            let node = graph.graph.node_weight(idx).unwrap();
            nodes.push(SOPNodeConfig {
                name: node.name().to_string(),
                kind: node.to_kind(),
            });
            node_indices.insert(idx, nodes.len() - 1);
        }

        let mut edges = Vec::new();
        for edge in graph.graph.edge_indices() {
            let (source, target) = graph.graph.edge_endpoints(edge).unwrap();
            if let (Some(&s), Some(&t)) = (node_indices.get(&source), node_indices.get(&target)) {
                edges.push((s, t));
            }
        }

        Self { nodes, edges }
    }
}

#[derive(Debug)]
pub struct ExecutionContext {
    pub results: Arc<RwLock<HashMap<String, NodeOutput>>>,
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionContext {
    pub fn new() -> Self {
        Self {
            results: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn set_result(&self, node_name: String, output: NodeOutput) {
        let mut results = self.results.write().unwrap();
        results.insert(node_name, output);
    }

    pub fn get_result(&self, node_name: &str) -> Option<NodeOutput> {
        let results = self.results.read().unwrap();
        results.get(node_name).cloned()
    }

    pub fn interpolate(&self, template: &str) -> String {
        let results = self.results.read().unwrap();
        let mut output = template.to_string();
        for (name, value) in results.iter() {
            let placeholder = format!("{{{{{}.output}}}}", name);
            let val_str = match value {
                Value::String(s) => s.clone(),
                _ => value.to_string(),
            };
            output = output.replace(&placeholder, &val_str);
        }
        output
    }
}

#[async_trait]
pub trait SOPNode: Send + Sync {
    async fn execute(&self, ctx: &ExecutionContext) -> Result<NodeOutput, String>;
    fn name(&self) -> &str;
    fn as_any(&self) -> &dyn std::any::Any;
    fn to_kind(&self) -> NodeKind;
}

pub struct WasmSOPNode {
    pub name: String,
    pub tool_id: String,
    pub fuel_limit: u64,
    pub executor: Arc<dyn ToolExecutor>,
}

#[async_trait]
impl SOPNode for WasmSOPNode {
    async fn execute(&self, _ctx: &ExecutionContext) -> Result<NodeOutput, String> {
        let args = Vec::new();
        let result = self
            .executor
            .execute(&self.tool_id, args, self.fuel_limit)
            .await?;
        Ok(Value::String(String::from_utf8_lossy(&result).to_string()))
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn to_kind(&self) -> NodeKind {
        NodeKind::Wasm {
            tool_id: self.tool_id.clone(),
            fuel_limit: self.fuel_limit,
        }
    }
}

pub struct RetrySOPNode {
    pub name: String,
    pub child: Arc<dyn SOPNode>,
    pub max_retries: u32,
}

#[async_trait]
impl SOPNode for RetrySOPNode {
    async fn execute(&self, ctx: &ExecutionContext) -> Result<NodeOutput, String> {
        let mut last_err = String::new();
        for _i in 0..=self.max_retries {
            match self.child.execute(ctx).await {
                Ok(out) => return Ok(out),
                Err(e) => {
                    last_err = e;
                }
            }
        }
        Err(format!(
            "RetrySOPNode failed after {} retries. Last error: {}",
            self.max_retries, last_err
        ))
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn to_kind(&self) -> NodeKind {
        NodeKind::Retry {
            child: Box::new(self.child.to_kind()),
            max_retries: self.max_retries,
        }
    }
}

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(
        &self,
        tool_id: &str,
        args: Vec<u8>,
        fuel_limit: u64,
    ) -> Result<Vec<u8>, String>;
}

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

    pub fn from_config(config: &SOPGraphConfig, executor: Arc<dyn ToolExecutor>) -> Self {
        let mut graph = SOPGraph::new();
        let mut node_indices = Vec::new();

        fn build_node(
            kind: &NodeKind,
            name: &str,
            executor: Arc<dyn ToolExecutor>,
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

        for &(source, target) in &config.edges {
            graph.add_edge(node_indices[source], node_indices[target]);
        }

        graph
    }

    pub fn add_node(&mut self, node: Arc<dyn SOPNode>) -> NodeIndex {
        self.graph.add_node(node)
    }

    pub fn add_edge(&mut self, from: NodeIndex, to: NodeIndex) {
        self.graph.add_edge(from, to, ());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct MockToolExecutor;
    #[async_trait]
    impl ToolExecutor for MockToolExecutor {
        async fn execute(
            &self,
            _tool_id: &str,
            _args: Vec<u8>,
            _fuel_limit: u64,
        ) -> Result<Vec<u8>, String> {
            Ok(b"mock_output".to_vec())
        }
    }

    #[test]
    fn test_empty_graph() {
        let graph = SOPGraph::new();
        assert_eq!(graph.graph.node_count(), 0);
    }

    #[test]
    fn test_add_node_edge() {
        let mut graph = SOPGraph::new();
        let executor = Arc::new(MockToolExecutor);

        let node1 = Arc::new(WasmSOPNode {
            name: "node1".to_string(),
            tool_id: "tool1".to_string(),
            fuel_limit: 100,
            executor: executor.clone(),
        });
        let node2 = Arc::new(WasmSOPNode {
            name: "node2".to_string(),
            tool_id: "tool2".to_string(),
            fuel_limit: 100,
            executor: executor.clone(),
        });

        let idx1 = graph.add_node(node1);
        let idx2 = graph.add_node(node2);

        assert_eq!(graph.graph.node_count(), 2);

        graph.add_edge(idx1, idx2);
        assert_eq!(graph.graph.edge_count(), 1);
    }

    #[test]
    fn test_set_get_result() {
        let ctx = ExecutionContext::new();
        let val = json!({"status": "ok"});
        ctx.set_result("node1".to_string(), val.clone());
        assert_eq!(ctx.get_result("node1"), Some(val));
    }

    #[test]
    fn test_get_missing() {
        let ctx = ExecutionContext::new();
        assert_eq!(ctx.get_result("missing"), None);
    }

    #[test]
    fn test_interpolate() {
        let ctx = ExecutionContext::new();
        ctx.set_result("node1".to_string(), json!("hello"));
        ctx.set_result("node2".to_string(), json!(123));

        let template = "Greeting: {{node1.output}}, ID: {{node2.output}}";
        let result = ctx.interpolate(template);
        assert_eq!(result, "Greeting: hello, ID: 123");
    }

    #[test]
    fn test_config_roundtrip() {
        let mut graph = SOPGraph::new();
        let executor = Arc::new(MockToolExecutor);

        let node1 = Arc::new(WasmSOPNode {
            name: "node1".to_string(),
            tool_id: "tool1".to_string(),
            fuel_limit: 100,
            executor: executor.clone(),
        });
        let node2 = Arc::new(WasmSOPNode {
            name: "node2".to_string(),
            tool_id: "tool2".to_string(),
            fuel_limit: 100,
            executor: executor.clone(),
        });

        let idx1 = graph.add_node(node1);
        let idx2 = graph.add_node(node2);
        graph.add_edge(idx1, idx2);

        let config = SOPGraphConfig::from_graph(&graph);
        assert_eq!(config.nodes.len(), 2);
        assert_eq!(config.edges.len(), 1);

        let rebuilt_graph = SOPGraph::from_config(&config, executor);
        assert_eq!(rebuilt_graph.graph.node_count(), 2);
        assert_eq!(rebuilt_graph.graph.edge_count(), 1);
    }
}
