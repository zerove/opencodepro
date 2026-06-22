use async_trait::async_trait;
use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub type NodeOutput = Value;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ExecutionEvent {
    NodeStarted { node_name: String },
    NodeCompleted { node_name: String, output: NodeOutput },
    NodeFailed { node_name: String, error: String },
    ExecutionComplete { result: NodeOutput },
}

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
        let mut results = self
            .results
            .write()
            .expect("ExecutionContext RwLock poisoned");
        results.insert(node_name, output);
    }

    pub fn get_result(&self, node_name: &str) -> Option<NodeOutput> {
        let results = self
            .results
            .read()
            .expect("ExecutionContext RwLock poisoned");
        results.get(node_name).cloned()
    }

    pub fn interpolate(&self, template: &str) -> String {
        let results = self
            .results
            .read()
            .expect("ExecutionContext RwLock poisoned");
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

// ── Configuration ────────────────────────────────────────────────────

/// Deserializable TOML configuration for all Sisyphus components.
///
/// Use `Config::from_file(path)` or `Config::default()` and override
/// individual fields via environment variables.
#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct Config {
    /// Cognitive loop parameters.
    #[serde(default = "default_cognitive_loop")]
    pub cognitive_loop: CognitiveLoopConfig,

    /// DAG executor parameters.
    #[serde(default = "default_executor")]
    pub executor: ExecutorConfig,

    /// Two-level cache (L1 DashMap + L2 SQLite) parameters.
    #[serde(default = "default_cache")]
    pub cache: CacheConfig,

    /// Prometheus metrics exporter.
    #[serde(default = "default_metrics")]
    pub metrics: MetricsConfig,

    /// WASM sandbox parameters.
    #[serde(default = "default_sandbox")]
    pub sandbox: SandboxConfig,
}

impl Config {
    /// Load configuration from a TOML file path.
    /// Returns `Err` if the file cannot be read or parsed.
    pub fn from_file(path: &str) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file '{}': {}", path, e))?;
        toml::from_str(&content)
            .map_err(|e| format!("Failed to parse config file '{}': {}", path, e))
    }

    /// Load configuration from environment variables, overriding file/default values.
    /// Prefix: `SISYPHUS_` — e.g. `SISYPHUS_SANDBOX_DEFAULT_FUEL=500000`.
    pub fn apply_env_overrides(&mut self) {
        fn env_u64(key: &str) -> Option<u64> {
            std::env::var(key).ok().and_then(|v| v.parse().ok())
        }
        fn env_usize(key: &str) -> Option<usize> {
            std::env::var(key).ok().and_then(|v| v.parse().ok())
        }
        fn env_u32(key: &str) -> Option<u32> {
            std::env::var(key).ok().and_then(|v| v.parse().ok())
        }
        fn env_f64(key: &str) -> Option<f64> {
            std::env::var(key).ok().and_then(|v| v.parse().ok())
        }
        fn env_u16(key: &str) -> Option<u16> {
            std::env::var(key).ok().and_then(|v| v.parse().ok())
        }
        fn env_string(key: &str) -> Option<String> {
            std::env::var(key).ok()
        }

        if let Some(v) = env_u32("SISYPHUS_COGNITIVE_LOOP_MAX_ITERATIONS") {
            self.cognitive_loop.max_iterations = v;
        }
        if let Some(v) = env_f64("SISYPHUS_COGNITIVE_LOOP_CONVERGENCE_THRESHOLD") {
            self.cognitive_loop.convergence_threshold = v;
        }
        if let Some(v) = env_usize("SISYPHUS_EXECUTOR_MAX_CONCURRENT_TASKS") {
            self.executor.max_concurrent_tasks = v;
        }
        if let Some(v) = env_u64("SISYPHUS_EXECUTOR_DEFAULT_FUEL") {
            self.executor.default_fuel = v;
        }
        if let Some(v) = env_usize("SISYPHUS_CACHE_L1_CAPACITY") {
            self.cache.l1_capacity = v;
        }
        if let Some(v) = env_string("SISYPHUS_CACHE_SQLITE_PATH") {
            self.cache.sqlite_path = v;
        }
        if let Some(v) = env_u16("SISYPHUS_METRICS_PROMETHEUS_PORT") {
            self.metrics.prometheus_port = v;
        }
        if let Some(v) = env_u32("SISYPHUS_SANDBOX_MAX_MEMORY_PAGES") {
            self.sandbox.max_memory_pages = v;
        }
        if let Some(v) = env_u64("SISYPHUS_SANDBOX_DEFAULT_FUEL") {
            self.sandbox.default_fuel = v;
        }
        if let Some(v) = env_string("SISYPHUS_SANDBOX_WASM_TOOLS_DIR") {
            self.sandbox.wasm_tools_dir = v;
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct CognitiveLoopConfig {
    pub max_iterations: u32,
    pub convergence_threshold: f64,
}

fn default_cognitive_loop() -> CognitiveLoopConfig {
    CognitiveLoopConfig {
        max_iterations: 10,
        convergence_threshold: 0.15,
    }
}

impl Default for CognitiveLoopConfig {
    fn default() -> Self {
        default_cognitive_loop()
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct ExecutorConfig {
    pub max_concurrent_tasks: usize,
    pub default_fuel: u64,
}

fn default_executor() -> ExecutorConfig {
    ExecutorConfig {
        max_concurrent_tasks: 16,
        default_fuel: 1_000_000,
    }
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        default_executor()
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct CacheConfig {
    pub l1_capacity: usize,
    pub sqlite_path: String,
}

fn default_cache() -> CacheConfig {
    CacheConfig {
        l1_capacity: 10_000,
        sqlite_path: "data/sisyphus.db".to_string(),
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        default_cache()
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct MetricsConfig {
    pub prometheus_port: u16,
}

fn default_metrics() -> MetricsConfig {
    MetricsConfig { prometheus_port: 9100 }
}

impl Default for MetricsConfig {
    fn default() -> Self {
        default_metrics()
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct SandboxConfig {
    pub max_memory_pages: u32,
    pub default_fuel: u64,
    pub wasm_tools_dir: String,
}

fn default_sandbox() -> SandboxConfig {
    SandboxConfig {
        max_memory_pages: 10,
        default_fuel: 1_000_000,
        wasm_tools_dir: "tools".to_string(),
    }
}

impl Default for SandboxConfig {
    fn default() -> Self {
        default_sandbox()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
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

    // ── proptest strategies ──────────────────────────────────────────

    fn arb_node_name() -> impl Strategy<Value = String> {
        "[a-zA-Z_][a-zA-Z0-9_]{0,15}"
    }

    fn arb_tool_id() -> impl Strategy<Value = String> {
        "[a-z][a-z0-9_]{0,31}"
    }

    fn arb_node_kind_depth(depth: u32) -> BoxedStrategy<NodeKind> {
        if depth == 0 {
            (arb_tool_id(), 0u64..10_000_000u64)
                .prop_map(|(tool_id, fuel)| NodeKind::Wasm {
                    tool_id,
                    fuel_limit: fuel,
                })
                .boxed()
        } else {
            prop_oneof![
                (arb_tool_id(), 0u64..10_000_000u64).prop_map(|(tool_id, fuel)| NodeKind::Wasm {
                    tool_id,
                    fuel_limit: fuel
                }),
                (arb_node_kind_depth(depth - 1), 0u32..5u32).prop_map(|(child, max_retries)| {
                    NodeKind::Retry {
                        child: Box::new(child),
                        max_retries,
                    }
                }),
            ]
            .boxed()
        }
    }

    fn arb_node_kind() -> BoxedStrategy<NodeKind> {
        arb_node_kind_depth(3)
    }

    fn arb_sop_node_config() -> impl Strategy<Value = SOPNodeConfig> {
        (arb_node_name(), arb_node_kind()).prop_map(|(name, kind)| SOPNodeConfig { name, kind })
    }

    fn arb_sop_graph_config() -> impl Strategy<Value = SOPGraphConfig> {
        proptest::collection::vec(arb_sop_node_config(), 0..8).prop_flat_map(|nodes| {
            let node_count = nodes.len();
            let max_edges = if node_count < 2 {
                0
            } else {
                (node_count * (node_count - 1)) / 2
            };
            let edge_strategy = proptest::collection::vec(
                (0..node_count, 0..node_count).prop_filter(
                    "edges must go from lower to higher index (DAG)",
                    |(s, t)| s < t,
                ),
                0..max_edges.saturating_add(1),
            );
            (Just(nodes), edge_strategy).prop_map(|(nodes, raw_edges)| {
                let mut seen = std::collections::HashSet::new();
                let edges: Vec<(usize, usize)> =
                    raw_edges.into_iter().filter(|e| seen.insert(*e)).collect();
                SOPGraphConfig { nodes, edges }
            })
        })
    }

    fn arb_json_value() -> impl Strategy<Value = Value> {
        prop_oneof![
            proptest::string::string_regex(".{0,64}")
                .unwrap()
                .prop_map(Value::String),
            any::<i64>().prop_map(|n| json!(n)),
            any::<f64>().prop_map(|n| json!(n)),
            proptest::bool::ANY.prop_map(Value::Bool),
            (
                0usize..10,
                proptest::string::string_regex(".{0,16}").unwrap()
            )
                .prop_map(|(n, s)| json!({ "key": s, "index": n })),
        ]
    }

    fn arb_context_key() -> impl Strategy<Value = String> {
        "[a-zA-Z0-9_]{1,16}"
    }

    // ── existing unit tests ──────────────────────────────────────────

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

    // ── proptest property-based tests ────────────────────────────────

    proptest! {
        #[test]
        fn proptest_sop_graph_config_roundtrip(
            config in arb_sop_graph_config(),
        ) {
            let executor = Arc::new(MockToolExecutor);
            let graph = SOPGraph::from_config(&config, executor.clone());
            prop_assert_eq!(graph.graph.node_count(), config.nodes.len());

            let roundtrip = SOPGraphConfig::from_graph(&graph);
            prop_assert_eq!(roundtrip.nodes.len(), config.nodes.len());
            prop_assert!(roundtrip.edges.len() <= config.edges.len());
            if config.edges.is_empty() {
                prop_assert_eq!(roundtrip.edges.len(), 0);
            }

            let rebuilt = SOPGraph::from_config(&roundtrip, executor.clone());
            prop_assert_eq!(rebuilt.graph.node_count(), roundtrip.nodes.len());
            prop_assert_eq!(rebuilt.graph.edge_count(), roundtrip.edges.len());

            for (i, node_cfg) in config.nodes.iter().enumerate() {
                let node = graph.graph.node_weight(NodeIndex::new(i)).unwrap();
                prop_assert_eq!(node.name(), &node_cfg.name);
            }
        }

        #[test]
        fn proptest_context_set_get(
            key in arb_context_key(),
            value in arb_json_value(),
        ) {
            let ctx = ExecutionContext::new();
            ctx.set_result(key.clone(), value.clone());
            let retrieved = ctx.get_result(&key);
            prop_assert_eq!(retrieved, Some(value));
        }

        #[test]
        fn proptest_context_interpolate(
            key in arb_context_key(),
            value_str in proptest::string::string_regex("[a-zA-Z0-9 ]{0,32}").unwrap(),
            prefix in proptest::string::string_regex("[a-zA-Z0-9 ]{0,16}").unwrap(),
            suffix in proptest::string::string_regex("[a-zA-Z0-9 ]{0,16}").unwrap(),
        ) {
            let ctx = ExecutionContext::new();
            let value = Value::String(value_str.clone());
            ctx.set_result(key.clone(), value);

            let template = [prefix.as_str(), "{{", key.as_str(), ".output}}", suffix.as_str()].concat();
            let result = ctx.interpolate(&template);
            let expected = format!("{}{}{}", prefix, value_str, suffix);
            prop_assert_eq!(result, expected);
        }

        #[test]
        fn proptest_context_missing_key_returns_none(
            key in arb_context_key(),
        ) {
            let ctx = ExecutionContext::new();
            prop_assert_eq!(ctx.get_result(&key), None);
        }

        #[test]
        fn proptest_composite_graph_add_operations(
            graph_ids in proptest::collection::hash_set(arb_node_name(), 0..8),
        ) {
            let mut composite = CompositeGraph::new();
            prop_assert!(composite.graphs.is_empty());
            prop_assert!(composite.dependencies.is_empty());

            let expected_count = graph_ids.len();
            for gid in &graph_ids {
                let config = SOPGraphConfig {
                    nodes: vec![SOPNodeConfig {
                        name: gid.clone(),
                        kind: NodeKind::Wasm {
                            tool_id: gid.clone(),
                            fuel_limit: 100,
                        },
                    }],
                    edges: vec![],
                };
                composite.add_graph(gid, config);
            }

            prop_assert_eq!(composite.graphs.len(), expected_count);
        }
    }
}
