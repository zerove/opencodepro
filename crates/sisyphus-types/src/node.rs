use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

use crate::errors::TypesError;
use crate::execution::{ExecutionContext, NodeOutput};
use crate::graph::NodeKind;

#[async_trait]
pub trait SOPNode: Send + Sync {
    async fn execute(&self, ctx: &ExecutionContext) -> Result<NodeOutput, TypesError>;
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
    async fn execute(&self, _ctx: &ExecutionContext) -> Result<NodeOutput, TypesError> {
        let args = Vec::new();
        let result = self
            .executor
            .execute(&self.tool_id, args, self.fuel_limit)
            .await?;
        Ok(NodeOutput(Value::String(
            String::from_utf8_lossy(&result).to_string(),
        )))
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
    async fn execute(&self, ctx: &ExecutionContext) -> Result<NodeOutput, TypesError> {
        let mut last_err = String::new();
        for _i in 0..=self.max_retries {
            match self.child.execute(ctx).await {
                Ok(out) => return Ok(out),
                Err(e) => {
                    last_err = e.to_string();
                }
            }
        }
        Err(TypesError::NodeExecutionError(format!(
            "RetrySOPNode failed after {} retries. Last error: {}",
            self.max_retries, last_err
        )))
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
    ) -> Result<Vec<u8>, TypesError>;
}

/// Routes tool calls to registered sub-executors based on the tool_id prefix.
///
/// The tool_id is split on the first `:` separator. If the prefix matches a
/// registered executor, it is stripped and the remainder is forwarded. If no
/// prefix matches or there is no `:`, the `default` executor is used.
///
/// # Example
///
/// ```ignore
/// let mut router = ExecutorRouter::new(builtin_executor);
/// router.register("wasm", wasm_executor);
/// router.register("mcp", mcp_executor);
///
/// // dispatches to builtin_executor.execute("file_read", ...)
/// router.execute("file_read", args, 0).await;
///
/// // dispatches to mcp_executor.execute("echo", ...)
/// router.execute("mcp:echo", args, 0).await;
/// ```
pub struct ExecutorRouter {
    routers: std::collections::HashMap<String, Arc<dyn ToolExecutor>>,
    default: Arc<dyn ToolExecutor>,
}

impl ExecutorRouter {
    /// Create a router with the given default executor.
    ///
    /// Tools with no matching prefix (or no `:` separator) are dispatched
    /// to `default`.
    pub fn new(default: Arc<dyn ToolExecutor>) -> Self {
        Self {
            routers: std::collections::HashMap::new(),
            default,
        }
    }

    /// Register a sub-executor under a prefix.
    ///
    /// After registration, tool IDs of the form `"{prefix}:{name}"` will be
    /// dispatched to this executor with `{name}` as the tool_id.
    pub fn register(&mut self, prefix: &str, executor: Arc<dyn ToolExecutor>) {
        self.routers.insert(prefix.to_string(), executor);
    }
}

#[async_trait]
impl ToolExecutor for ExecutorRouter {
    async fn execute(
        &self,
        tool_id: &str,
        args: Vec<u8>,
        fuel_limit: u64,
    ) -> Result<Vec<u8>, TypesError> {
        if let Some(pos) = tool_id.find(':') {
            let prefix = &tool_id[..pos];
            let remainder = &tool_id[pos + 1..];
            if let Some(executor) = self.routers.get(prefix) {
                return executor.execute(remainder, args, fuel_limit).await;
            }
        }
        // No prefix match or no ':' — use default
        self.default.execute(tool_id, args, fuel_limit).await
    }
}
