use async_trait::async_trait;
use serde_json::Value;
use sisyphus_core::{Executor, NodeOutput, SOPGraph};
use sisyphus_types::SOPNode;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Mock node that returns a fixed name-based result.
// ---------------------------------------------------------------------------
struct MockNode {
    name: String,
    should_fail: bool,
}

impl MockNode {
    fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            should_fail: false,
        }
    }
    fn failing(name: &str) -> Self {
        Self {
            name: name.into(),
            should_fail: true,
        }
    }
}

#[async_trait]
impl SOPNode for MockNode {
    async fn execute(&self, _ctx: &sisyphus_core::ExecutionContext) -> Result<NodeOutput, sisyphus_types::TypesError> {
        if self.should_fail {
            Err(sisyphus_types::TypesError::NodeExecutionError(format!("ResourceExhausted: {} failed", self.name)))
        } else {
            Ok(NodeOutput(Value::String(format!("result_{}", self.name))))
        }
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn to_kind(&self) -> sisyphus_types::NodeKind {
        sisyphus_types::NodeKind::Wasm {
            tool_id: "mock".into(),
            fuel_limit: 100,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_single_node() {
    let mut graph = SOPGraph::new();
    let node = Arc::new(MockNode::new("alpha"));
    let idx = graph.add_node(node);
    let executor = Executor::new();

    let ctx = executor.execute(&graph, idx).await.unwrap();
    let out = ctx.get_result("alpha").unwrap();
    assert_eq!(out, Value::String("result_alpha".into()));
}

#[tokio::test]
async fn test_linear_dag() {
    let mut graph = SOPGraph::new();
    let n1 = graph.add_node(Arc::new(MockNode::new("step1")));
    let n2 = graph.add_node(Arc::new(MockNode::new("step2")));
    graph.add_edge(n1, n2);

    let executor = Executor::new();
    let ctx = executor.execute(&graph, n1).await.unwrap();

    assert_eq!(
        ctx.get_result("step1").unwrap(),
        Value::String("result_step1".into())
    );
    assert_eq!(
        ctx.get_result("step2").unwrap(),
        Value::String("result_step2".into())
    );
}

#[tokio::test]
async fn test_cycle_detection() {
    let mut graph = SOPGraph::new();
    let n1 = graph.add_node(Arc::new(MockNode::new("a")));
    let n2 = graph.add_node(Arc::new(MockNode::new("b")));
    graph.add_edge(n1, n2);
    graph.add_edge(n2, n1); // cycle

    let executor = Executor::new();
    let result = executor.execute(&graph, n1).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cycle"));
}

#[tokio::test]
async fn test_parallel_execution() {
    let mut graph = SOPGraph::new();
    let n1 = graph.add_node(Arc::new(MockNode::new("task1")));
    let n2 = graph.add_node(Arc::new(MockNode::new("task2")));
    // Both feed into n3
    let n3 = graph.add_node(Arc::new(MockNode::new("task3")));
    graph.add_edge(n1, n3);
    graph.add_edge(n2, n3);

    let executor = Executor::new();
    let ctx = executor.execute(&graph, n1).await.unwrap();

    assert_eq!(
        ctx.get_result("task1").unwrap(),
        Value::String("result_task1".into())
    );
    assert_eq!(
        ctx.get_result("task2").unwrap(),
        Value::String("result_task2".into())
    );
    assert_eq!(
        ctx.get_result("task3").unwrap(),
        Value::String("result_task3".into())
    );
}

#[tokio::test]
async fn test_node_error_propagated() {
    let mut graph = SOPGraph::new();
    let n1 = graph.add_node(Arc::new(MockNode::failing("crash")));
    let executor = Executor::new();

    let ctx = executor.execute(&graph, n1).await.unwrap();
    let out = ctx.get_result("crash").unwrap();
    // Error is stored as a Value::String containing the error message
    assert!(out.to_string().contains("ResourceExhausted"));
}
