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
    async fn execute(
        &self,
        _ctx: &sisyphus_core::ExecutionContext,
    ) -> Result<NodeOutput, sisyphus_types::TypesError> {
        if self.should_fail {
            Err(sisyphus_types::TypesError::NodeExecutionError(format!(
                "ResourceExhausted: {} failed",
                self.name
            )))
        } else {
            Ok(NodeOutput::from(Value::String(format!(
                "result_{}",
                self.name
            ))))
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
            args: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Mock ToolExecutor
// ---------------------------------------------------------------------------
struct MockToolExecutor;

#[async_trait]
impl sisyphus_types::ToolExecutor for MockToolExecutor {
    async fn execute(
        &self,
        _tool_id: &str,
        _args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
        Ok(b"mock".to_vec())
    }
}

fn make_executor() -> Executor {
    Executor::new(
        Arc::new(sisyphus_tools_executor::executor::test_cmds::TestExecutor::new()),
        Arc::new(sisyphus_core::intervention_manager::InterventionManager::new()),
        Arc::new(
            sisyphus_core::internalization_manager::InternalizationManager::new(
                Arc::new(sisyphus_tools_llm::metis::DummyLlmClient),
                Arc::new(sisyphus_knowledge::AssetManager::new(
                    ".sisyphus/knowledge".into(),
                )),
            ),
        ),
        None,
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_single_node() {
    let mut graph = SOPGraph::new();
    let node = Arc::new(MockNode::new("alpha"));
    let idx = graph.add_node(node);
    let executor = make_executor();

    let ctx = executor.execute(Arc::new(graph), idx, Arc::new(MockToolExecutor)).await.unwrap();
    let out = ctx.get_result("alpha").unwrap();
    assert_eq!(out, Value::String("result_alpha".into()));
}

#[tokio::test]
async fn test_linear_dag() {
    let mut graph = SOPGraph::new();
    let n1 = graph.add_node(Arc::new(MockNode::new("step1")));
    let n2 = graph.add_node(Arc::new(MockNode::new("step2")));
    graph.add_edge(n1, n2);

    let executor = make_executor();
    let ctx = executor.execute(Arc::new(graph), n1, Arc::new(MockToolExecutor)).await.unwrap();

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

    let executor = make_executor();
    let result = executor.execute(Arc::new(graph), n1, Arc::new(MockToolExecutor)).await;
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

    let executor = make_executor();
    let ctx = executor.execute(Arc::new(graph), n1, Arc::new(MockToolExecutor)).await.unwrap();

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
    let executor = make_executor();

    // 当前契约：节点失败中止整个执行并**向上传播错误**，而不是把错误塞进
    // 上下文当作正常结果——「错误即成功」会让上层把全失败误报为 SUCCESS。
    // 覆盖 streaming / 重试 / 恢复分支的完整矩阵见
    // executor_failure_semantics.rs。
    let err = executor
        .execute(Arc::new(graph), n1, Arc::new(MockToolExecutor))
        .await
        .expect_err("a failing node must abort the run");
    match err {
        sisyphus_core::CoreError::NodeExecutionFailed { node_name, source } => {
            assert_eq!(node_name, "crash");
            assert!(
                source.to_string().contains("ResourceExhausted"),
                "propagated error must keep its cause, got: {source}"
            );
        },
        other => panic!("unexpected error variant: {other:?}"),
    }
}
