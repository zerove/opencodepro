//! Sisyphus Types — 共享类型、配置与核心 trait 定义。
//!
//! 定义 Config、ExecutionContext、SOPGraph、ToolExecutor 等核心类型，
//! 以及错误类型与跨 crate 通用 trait。

#![allow(clippy::cast_precision_loss)]
pub mod asset;
pub mod errors;
pub mod experience;
pub mod roadmap;
pub mod tools;

pub mod config;
pub mod execution;
pub mod graph;
pub mod node;
pub mod skill;
pub mod types;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoldenSop {
    pub goal: String,
    pub sop_config: SOPGraphConfig,
    pub success_metric: f64,
    pub timestamp: u64,
}

#[async_trait]
pub trait GoldenDataset: Send + Sync {
    async fn enroll(
        &self,
        goal: &str,
        config: SOPGraphConfig,
        metric: f64,
    ) -> Result<(), TypesError>;
    async fn get_golden_sop(&self, goal: &str) -> Result<Option<GoldenSop>, TypesError>;
    async fn verify_no_regression(
        &self,
        goal: &str,
        candidate: &SOPGraphConfig,
        current_metric: f64,
    ) -> Result<bool, TypesError>;
}

pub use config::*;
pub use errors::*;
pub use execution::*;
pub use graph::*;
pub use node::*;
pub use roadmap::*;
pub use types::*;

// ── Cognitive Map Types ────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CogNode {
    pub id: String,
    pub label: String,
    pub status: String, // "pending", "running", "ok", "error"
    pub deviation: f64,
    pub position: (f32, f32), // For frontend rendering
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CogEdge {
    pub source: String,
    pub target: String,
    pub label: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CogMap {
    pub sop_id: String,
    pub goal_text: String,
    pub nodes: Vec<CogNode>,
    pub edges: Vec<CogEdge>,
    pub active_node: Option<String>,
    pub iteration: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use petgraph::graph::NodeIndex;
    use proptest::prelude::*;
    use serde_json::json;
    use serde_json::Value;
    use std::sync::Arc;

    struct MockToolExecutor;
    #[async_trait]
    impl ToolExecutor for MockToolExecutor {
        async fn execute(
            &self,
            _tool_id: &str,
            _args: Vec<u8>,
            _fuel_limit: u64,
        ) -> Result<Vec<u8>, TypesError> {
            Ok(b"mock_output".to_vec())
        }
    }

    struct MockTestExecutor;
    #[async_trait]
    impl TestExecutor for MockTestExecutor {
        async fn run_test(
            &self,
            _command: &str,
            _workdir: Option<&str>,
        ) -> Result<TestResult, TypesError> {
            Ok(TestResult {
                success: true,
                stdout: "mock test output".to_string(),
                stderr: String::new(),
                exit_code: 0,
                duration_ms: 0,
                error_summary: None,
            })
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
        proptest::collection::vec(arb_sop_node_config(), 1..8).prop_flat_map(|nodes| {
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
        ctx.set_result("node1".to_string(), NodeOutput::from(val.clone()));
        let result = ctx.get_result("node1");
        assert_eq!(result, Some(NodeOutput::from(val)));
    }

    #[test]
    fn test_get_missing() {
        let ctx = ExecutionContext::new();
        assert_eq!(ctx.get_result("missing"), None);
    }

    #[test]
    fn test_interpolate() {
        let ctx = ExecutionContext::new();
        ctx.set_result("node1".to_string(), NodeOutput::from(json!("hello")));
        ctx.set_result("node2".to_string(), NodeOutput::from(json!(123)));

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

        let config = SOPGraphConfig::from_graph(&graph).unwrap();
        assert_eq!(config.nodes.len(), 2);
        assert_eq!(config.edges.len(), 1);

        let test_executor = Arc::new(MockTestExecutor);
        let rebuilt_graph = SOPGraph::from_config(&config, executor, test_executor).unwrap();
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
            let test_executor = Arc::new(MockTestExecutor);
            let graph = SOPGraph::from_config(&config, executor.clone(), test_executor.clone()).unwrap();
            prop_assert_eq!(graph.graph.node_count(), config.nodes.len());

            let roundtrip = SOPGraphConfig::from_graph(&graph).unwrap();
            prop_assert_eq!(roundtrip.nodes.len(), config.nodes.len());
            prop_assert!(roundtrip.edges.len() <= config.edges.len());
            if config.edges.is_empty() {
                prop_assert_eq!(roundtrip.edges.len(), 0);
            }

            let rebuilt = SOPGraph::from_config(&roundtrip, executor.clone(), test_executor.clone()).unwrap();
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
            ctx.set_result(key.clone(), NodeOutput::from(value.clone()));
            let retrieved = ctx.get_result(&key);
            prop_assert_eq!(retrieved, Some(NodeOutput::from(value)));
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
            ctx.set_result(key.clone(), NodeOutput::from(value));

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

    // ── TypesError tests ───────────────────────────────────────────────

    #[test]
    fn test_types_error_config_read_display() {
        let source = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = TypesError::ConfigReadError {
            path: "config.toml".into(),
            source,
        };
        let msg = err.to_string();
        assert!(msg.contains("config.toml"), "should contain path");
        assert!(
            msg.contains("file not found"),
            "should contain source error"
        );
    }

    #[test]
    fn test_types_error_config_parse_display() {
        let source = toml::from_str::<serde_json::Value>("invalid = [toml").unwrap_err();
        let err = TypesError::ConfigParseError {
            path: "config.toml".into(),
            source,
        };
        let msg = err.to_string();
        assert!(msg.contains("config.toml"), "should contain path");
        assert!(!msg.is_empty(), "should contain source error");
    }

    #[test]
    fn test_types_error_state_store_display() {
        let err = TypesError::StateStoreError("db connection failed".into());
        let msg = err.to_string();
        assert!(msg.contains("State store error"), "should contain prefix");
        assert!(
            msg.contains("db connection failed"),
            "should contain message"
        );
    }

    #[test]
    fn test_types_error_node_execution_display() {
        let err = TypesError::NodeExecutionError("node timeout".into());
        let msg = err.to_string();
        assert!(
            msg.contains("Node execution error"),
            "should contain prefix"
        );
        assert!(msg.contains("node timeout"), "should contain message");
    }

    #[test]
    fn test_types_error_tool_execution_display() {
        let err = TypesError::ToolExecutionError {
            tool_id: "test_tool".into(),
            details: "something went wrong".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("test_tool"), "should contain tool_id");
        assert!(
            msg.contains("something went wrong"),
            "should contain details"
        );
    }

    #[test]
    fn test_types_error_string_conversion() {
        let variants = vec![
            TypesError::StateStoreError("err1".into()),
            TypesError::NodeExecutionError("err2".into()),
            TypesError::ToolExecutionError {
                tool_id: "t1".into(),
                details: "err3".into(),
            },
        ];
        for err in variants {
            let s = err.to_string();
            assert!(!s.is_empty(), "conversion to string should not be empty");
        }
    }

    #[test]
    fn test_types_error_propagation() {
        fn propagate() -> Result<(), TypesError> {
            Err(TypesError::StateStoreError("propagate".into()))
        }
        let result = propagate();
        assert!(result.is_err(), "should return an error");
        if let Err(TypesError::StateStoreError(msg)) = result {
            assert_eq!(msg, "propagate");
        } else {
            panic!("Expected StateStoreError");
        }
    }

    // ── ExecutorRouter tests ─────────────────────────────────────────

    struct PrefixExecutor(&'static str);
    #[async_trait]
    impl ToolExecutor for PrefixExecutor {
        async fn execute(
            &self,
            tool_id: &str,
            _args: Vec<u8>,
            _fuel_limit: u64,
        ) -> Result<Vec<u8>, TypesError> {
            let response = format!("{}:{}", self.0, tool_id);
            Ok(response.into_bytes())
        }
    }

    #[tokio::test]
    async fn test_router_default_only() {
        let default = Arc::new(PrefixExecutor("default"));
        let router = ExecutorRouter::new(default);
        let result = router.execute("some_tool", vec![], 0).await.unwrap();
        assert_eq!(String::from_utf8(result).unwrap(), "default:some_tool");
    }

    #[tokio::test]
    async fn test_router_prefix_dispatch() {
        let builtin = Arc::new(PrefixExecutor("builtin"));
        let wasm = Arc::new(PrefixExecutor("wasm"));
        let router = ExecutorRouter::new(builtin);
        router.register("wasm", wasm);

        // With prefix: routes to wasm executor
        let result = router.execute("wasm:my_tool", vec![], 0).await.unwrap();
        assert_eq!(String::from_utf8(result).unwrap(), "wasm:my_tool");

        // Without prefix: falls back to default (builtin)
        let result = router.execute("file_read", vec![], 0).await.unwrap();
        assert_eq!(String::from_utf8(result).unwrap(), "builtin:file_read");
    }

    #[tokio::test]
    async fn test_router_unknown_prefix_falls_back() {
        let default = Arc::new(PrefixExecutor("default"));
        let router = ExecutorRouter::new(default);
        router.register("wasm", Arc::new(PrefixExecutor("wasm")));

        // Unknown prefix "mcp:" should fall back to default, including the prefix
        let result = router.execute("mcp:tool", vec![], 0).await.unwrap();
        assert_eq!(String::from_utf8(result).unwrap(), "default:mcp:tool");
    }

    #[tokio::test]
    async fn test_router_empty_tool_id() {
        let default = Arc::new(PrefixExecutor("default"));
        let router = ExecutorRouter::new(default);
        let result = router.execute("", vec![], 0).await.unwrap();
        assert_eq!(String::from_utf8(result).unwrap(), "default:");
    }

    #[tokio::test]
    async fn test_router_colon_only() {
        let default = Arc::new(PrefixExecutor("default"));
        let router = ExecutorRouter::new(default);
        let result = router.execute(":", vec![], 0).await.unwrap();
        assert_eq!(String::from_utf8(result).unwrap(), "default::");
    }

    #[tokio::test]
    async fn test_router_multiple_prefixes() {
        let default = Arc::new(PrefixExecutor("default"));
        let wasm = Arc::new(PrefixExecutor("wasm"));
        let builtin = Arc::new(PrefixExecutor("builtin"));
        let mcp = Arc::new(PrefixExecutor("mcp"));

        let router = ExecutorRouter::new(default);
        router.register("wasm", wasm);
        router.register("builtin", builtin);
        router.register("mcp", mcp);

        let cases = vec![
            ("wasm:add", "wasm:add"),
            ("builtin:file_read", "builtin:file_read"),
            ("mcp:echo", "mcp:echo"),
            ("no_prefix", "default:no_prefix"),
        ];
        for (input, expected) in cases {
            let result = router.execute(input, vec![], 0).await.unwrap();
            assert_eq!(
                String::from_utf8(result).unwrap(),
                expected,
                "failed for input '{input}'"
            );
        }
    }
}
