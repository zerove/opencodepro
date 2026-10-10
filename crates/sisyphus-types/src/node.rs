use async_trait::async_trait;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

use crate::errors::TypesError;
use crate::execution::{ExecutionContext, NodeOutput};
use crate::graph::NodeKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
/// Reflection Priority.
pub enum ReflectionPriority {
    None, // Skip reflection completely
    Low,  // Use simple heuristic or skip if output seems okay
    High, // Always use LLM reflect
}

#[async_trait]
/// Node.
pub trait SOPNode: Send + Sync {
    async fn execute(&self, ctx: &ExecutionContext) -> Result<NodeOutput, TypesError>;
    fn name(&self) -> &str;
    fn as_any(&self) -> &dyn std::any::Any;
    fn to_kind(&self) -> NodeKind;
    fn reflection_priority(&self) -> ReflectionPriority {
        ReflectionPriority::Low
    }
}

/// Wasm Node.
pub struct WasmSOPNode {
    pub name: String,
    pub tool_id: String,
    pub fuel_limit: u64,
    pub executor: Arc<dyn ToolExecutor>,
    /// 传给底层工具的原始参数字节（JSON）。
    ///
    /// **历史上这里是写死的 `Vec::new()`**，因此任何需要入参的 builtin
    /// 工具（`web_fetch` 要 `url`、`bash_exec` 要 `command`）在
    /// `GoalDrivenSynthesizer` 合成的计划里必然失败。实测症状：
    /// `AllIterationsPivoted(1, "Node fetch produced output with deviation
    /// score 0.81")`，底层错误为
    /// `Tool execution error for 'web_fetch': missing or non-string arg 'url'`。
    ///
    /// 保持默认空值是为了让大量只关心 `tool_id` 的 mock/测试构造点
    /// （全仓 60+ 处）继续编译，而真正需要参数的规划器显式赋值。
    pub args: Vec<u8>,
}

#[async_trait]
impl SOPNode for WasmSOPNode {
    async fn execute(&self, ctx: &ExecutionContext) -> Result<NodeOutput, TypesError> {
        let args = self.args.clone();
        // P2-9 燃料真相管道：优先走带测量的路径。
        let (result, measurement) =
            match self.executor.execute_measured(&self.tool_id, args, self.fuel_limit).await {
                Ok((data, Some(m))) => (data, Some(m)),
                Ok((data, None)) => (data, None),
                // 测量路径失败时回退普通执行，保持行为兼容。
                Err(_) => {
                    let data =
                        self.executor.execute(&self.tool_id, Vec::new(), self.fuel_limit).await?;
                    (data, None)
                },
            };
        if let Some(m) = measurement {
            ctx.record_measurement(&self.name, m);
        }
        Ok(NodeOutput::from(Value::String(
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
            args: self.args.clone(),
        }
    }
}

/// Retry Node.
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
                },
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

/// Branch Node.
pub struct BranchSOPNode {
    pub name: String,
    /// A map of "condition_result" -> "next_node_index".
    pub branches: std::collections::HashMap<String, usize>,
    /// The default node index to follow if no condition matches.
    pub default_next: usize,
}

#[async_trait]
impl SOPNode for BranchSOPNode {
    async fn execute(&self, ctx: &ExecutionContext) -> Result<NodeOutput, TypesError> {
        let last_output = ctx.last_output().ok_or_else(|| {
            TypesError::NodeExecutionError("BranchNode requires a previous output to decide".into())
        })?;
        let output_str = last_output.as_str().unwrap_or("");
        let next_node = self.branches.get(output_str).unwrap_or(&self.default_next);
        Ok(NodeOutput::from(Value::Number(serde_json::Number::from(
            *next_node,
        ))))
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn to_kind(&self) -> NodeKind {
        NodeKind::Branch {
            branches: self.branches.clone(),
            default_next: self.default_next,
        }
    }
}

/// Sub Graph Node.
pub struct SubGraphSOPNode {
    pub name: String,
    pub config: crate::graph::SOPGraphConfig,
}

#[async_trait]
impl SOPNode for SubGraphSOPNode {
    async fn execute(&self, _ctx: &ExecutionContext) -> Result<NodeOutput, TypesError> {
        Err(TypesError::NodeExecutionError(
            "SubGraphSOPNode must be executed by the Executor via recursive graph dispatch, not by direct call".into(),
        ))
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn to_kind(&self) -> NodeKind {
        NodeKind::SubGraph {
            config: Box::new(self.config.clone()),
        }
    }
}

#[async_trait]
/// Test Executor.
pub trait TestExecutor: Send + Sync {
    async fn run_test(
        &self,
        command: &str,
        workdir: Option<&str>,
    ) -> Result<TestResult, TypesError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Test Result.
pub struct TestResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u128,
    pub error_summary: Option<String>,
}

/// Test Node.
pub struct TestSOPNode {
    pub name: String,
    pub test_command: String,
    pub workdir: Option<String>,
    pub test_executor: Arc<dyn TestExecutor>,
}

#[async_trait]
impl SOPNode for TestSOPNode {
    async fn execute(&self, _ctx: &ExecutionContext) -> Result<NodeOutput, TypesError> {
        let result =
            self.test_executor.run_test(&self.test_command, self.workdir.as_deref()).await?;

        let output_str = if result.success {
            format!(
                "Test Passed: {}\nStdout: {}",
                self.test_command, result.stdout
            )
        } else {
            format!(
                "Test Failed: {}\nError: {}\nStderr: {}",
                self.test_command,
                result.error_summary.as_deref().unwrap_or("Unknown error"),
                result.stderr
            )
        };

        Ok(NodeOutput::from(Value::String(output_str)))
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn to_kind(&self) -> NodeKind {
        NodeKind::Test {
            command: self.test_command.clone(),
            workdir: self.workdir.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Tool Metadata.
pub struct ToolMetadata {
    pub id: String,
    pub description: String,
}

/// 单次 WASM 执行的墙钟上限（毫秒），同时用作 epoch 推进间隔。
///
/// Fuel 是首要限制，因为它确定性强、可复现；但它是**协作式**的——只在
/// guest 仍在执行 wasm 指令时才拦得住它。一旦计账本身失效（例如
/// `call_ref` / 异常路径漏记燃料，见 RUSTSEC-2026-0315），无限循环就永远
/// 不会被中断，调用会挂死。因此需要一道与计账无关的墙钟兜底。
///
/// 5s 的依据（本仓库实测，非拍脑袋）：正常工具冷编译约 114ms、命中模块
/// 缓存后约 0.9ms，相对 5s 有约 44 倍余量，不会误伤；而无兜底时的挂死需
/// 人工 `kill -9`，即运行器全局超时（实测 10+ 分钟），代价高得多。
///
/// 余量并非无限：`tools/` 下目前只有测试用 fixture，没有真实生产负载，
/// 因此这个值是**按现有证据选的**，尚未按生产流量标定。若日后出现耗时
/// 超过 5s 的合法工具，应先确认它是否需要更大上限，而不是直接调高。
///
/// 定义在此处而非任一沙箱 crate：`sisyphus-sandbox` 与 `sisyphus-sandboxd`
/// 各自持有独立的执行引擎与复制粘贴的执行路径，两边必须取同一个值，
/// 否则守护进程会与其客户端行为不一致。
pub const EXECUTION_TIMEOUT_MS: u64 = 5_000;

/// 单次工具执行的真实测量数据（P2-9：燃料真值打通）。
///
/// 此前事件流的 `fuel_consumed` 填的是配置上限而非真实消耗，
/// AnomalyDetector 学到的是恒定基线、燃料维度异常检测失效。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ExecutionMeasurement {
    /// WASM 实际消耗的燃料（fuel_limit - store.get_fuel()）。
    pub fuel_consumed: u64,
    /// 工具调用总耗时（毫秒，含编译/实例化）。
    pub duration_ms: u64,
    /// guest 线性内存的实际占用字节数（`Memory::data_size`）。
    ///
    /// 与 `fuel_consumed` 互补：燃料衡量指令数，这个衡量数据段体积。
    /// 缺失时为 `None`（非 WASM 执行器、或未暴露 memory 导出），
    /// 而不是 0——0 会被误读成“未占用内存”。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_memory_bytes: Option<u64>,
}

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(
        &self,
        tool_id: &str,
        args: Vec<u8>,
        fuel_limit: u64,
    ) -> Result<Vec<u8>, TypesError>;

    /// 带测量执行的默认实现：委托 `execute` 并报告无测量数据。
    ///
    /// 能拿到真实燃料计量的执行器（wasmtime store.get_fuel()）
    /// 应覆写本方法。默认实现的 measurement 为 None 时，
    /// 消费方回退到旧的上限值语义（行为兼容）。
    async fn execute_measured(
        &self,
        tool_id: &str,
        args: Vec<u8>,
        fuel_limit: u64,
    ) -> Result<(Vec<u8>, Option<ExecutionMeasurement>), TypesError> {
        let data = self.execute(tool_id, args, fuel_limit).await?;
        Ok((data, None))
    }

    /// List all tools provided by this executor.
    async fn list_tools(&self) -> Result<Vec<ToolMetadata>, TypesError> {
        Ok(Vec::new()) // Default implementation returns no tools
    }

    /// Returns a reference to the cache invalidator if this executor supports it.
    fn as_cache_invalidatable(&self) -> Option<&dyn CacheInvalidatable> {
        None
    }
}

/// Cache Invalidatable.
pub trait CacheInvalidatable: Send + Sync {
    fn invalidate_cache(&self, tool_id: &str);
}

/// Routes tool calls to registered sub-executors based on the tool_id prefix.
///
/// The tool_id is split on the first `:` separator. If the prefix matches a
/// registered executor, it is stripped and the remainder is forwarded. If no
/// prefix matches or there is no `:`, the `default` executor is used.
///
/// # Example
///
/// ```no_run
/// use async_trait::async_trait;
/// use sisyphus_types::{ExecutorRouter, ToolExecutor, TypesError};
/// use std::sync::Arc;
///
/// struct Noop;
/// #[async_trait]
/// impl ToolExecutor for Noop {
///     async fn execute(&self, _id: &str, _args: Vec<u8>, _fuel: u64)
///         -> Result<Vec<u8>, TypesError> { Ok(b"ok".to_vec()) }
/// }
///
/// # async fn demo(
/// #     builtin: Arc<dyn ToolExecutor>,
/// #     wasm: Arc<dyn ToolExecutor>,
/// #     mcp: Arc<dyn ToolExecutor>,
/// # ) -> Result<(), TypesError> {
/// let router = ExecutorRouter::new(builtin);
/// router.register("wasm", wasm);
/// router.register("mcp", mcp);
///
/// let args: Vec<u8> = Vec::new();
/// // 无前缀 → 走 default
/// router.execute("file_read", args.clone(), 0).await?;
/// // 前缀命中 → 剥离前缀后转发给 mcp
/// router.execute("mcp:echo", args, 0).await?;
/// # Ok::<(), TypesError>(())
/// # }
/// # fn main() {}
/// ```
/// 此前该示例标记为 ignore（不参与编译），因此 `mut`（`register` 取
/// `&self`，并不需要）与未定义的 `args` 都无人发现。
pub struct ExecutorRouter {
    routers: DashMap<String, Arc<dyn ToolExecutor>>,
    pub default: Arc<dyn ToolExecutor>,
}

impl ExecutorRouter {
    /// Create a router with the given default executor.
    ///
    /// Tools with no matching prefix (or no `:` separator) are dispatched
    /// to `default`.
    pub fn new(default: Arc<dyn ToolExecutor>) -> Self {
        Self {
            routers: DashMap::new(),
            default,
        }
    }

    /// Register a sub-executor under a prefix.
    ///
    /// After registration, tool IDs of the form `"{prefix}:{name}"` will be
    /// dispatched to this executor with `{name}` as the tool_id.
    pub fn register(&self, prefix: &str, executor: Arc<dyn ToolExecutor>) {
        self.routers.insert(prefix.to_string(), executor);
    }

    /// Checks if a tool is available in any of the registered executors.
    pub async fn has_tool(&self, tool_id: &str) -> Result<bool, TypesError> {
        if let Some(pos) = tool_id.find(':') {
            let prefix = &tool_id[..pos];
            let remainder = &tool_id[pos + 1..];
            if let Some(executor) = self.routers.get(prefix) {
                let tools = executor.list_tools().await?;
                return Ok(tools.iter().any(|t| t.id == remainder));
            }
        }
        let tools = self.default.list_tools().await?;
        Ok(tools.iter().any(|t| t.id == tool_id))
    }

    /// Aggregates all available tools from all registered executors.
    pub async fn list_tools(&self) -> Result<Vec<ToolMetadata>, TypesError> {
        let mut all_tools = Vec::new();

        // 1. Get tools from default executor
        let mut default_tools = self.default.list_tools().await?;
        all_tools.append(&mut default_tools);

        // 2. Get tools from prefixed routers
        for entry in self.routers.iter() {
            let prefix = entry.key();
            let executor = entry.value();
            let tools = executor.list_tools().await?;
            for mut tool in tools {
                tool.id = format!("{}:{}", prefix, tool.id);
                all_tools.push(tool);
            }
        }

        Ok(all_tools)
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

/// Visual Node.
pub struct VisualSOPNode {
    pub name: String,
    pub kind: NodeKind,
}

impl VisualSOPNode {
    /// Create a new node stub with `name` and `kind`, leaving inputs/outputs empty.
    ///
    /// Most callers should use [`NodeBuilder`] for richer construction.
    pub fn new(name: String, kind: NodeKind) -> Self {
        Self { name, kind }
    }
}

#[async_trait]
impl SOPNode for VisualSOPNode {
    async fn execute(&self, _ctx: &ExecutionContext) -> Result<NodeOutput, TypesError> {
        // Visual nodes are handled by the ExecutionManager's visual perception bridge.
        // We return a special output that signals the executor to perform visual analysis.
        Ok(NodeOutput::from(Value::String(format!(
            "VISUAL_EXEC_REQUIRED:{}",
            self.name
        ))))
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn to_kind(&self) -> NodeKind {
        self.kind.clone()
    }
}

/// Human Intervention Node.
pub struct HumanInterventionSOPNode {
    pub name: String,
    pub prompt: String,
    pub timeout_secs: u64,
}

#[async_trait]
impl SOPNode for HumanInterventionSOPNode {
    async fn execute(&self, _ctx: &ExecutionContext) -> Result<NodeOutput, TypesError> {
        // Intervention nodes are handled by the Executor's InteractionManager.
        // We return a special signal to pause execution.
        Ok(NodeOutput::from(Value::String(format!(
            "HUMAN_INTERVENTION_REQUIRED:{}",
            self.name
        ))))
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn to_kind(&self) -> NodeKind {
        NodeKind::HumanIntervention {
            prompt: self.prompt.clone(),
            timeout_secs: self.timeout_secs,
        }
    }
}

#[cfg(test)]
mod fuel_truth_tests {
    use super::*;
    use crate::execution::ExecutionContext;
    use async_trait::async_trait;
    use std::sync::Arc;

    /// 提供真实测量的执行器（模拟 sandboxd / in-process WASM 路径）。
    struct MeasuringExecutor {
        fuel: u64,
        fail_measured: bool,
    }
    #[async_trait]
    impl ToolExecutor for MeasuringExecutor {
        async fn execute(
            &self,
            _tool_id: &str,
            _args: Vec<u8>,
            _fuel_limit: u64,
        ) -> Result<Vec<u8>, TypesError> {
            Ok(b"plain".to_vec())
        }
        async fn execute_measured(
            &self,
            _tool_id: &str,
            _args: Vec<u8>,
            _fuel_limit: u64,
        ) -> Result<(Vec<u8>, Option<ExecutionMeasurement>), TypesError> {
            if self.fail_measured {
                Err(TypesError::ToolExecutionError {
                    tool_id: "t".into(),
                    details: "measured path down".into(),
                })
            } else {
                Ok((
                    b"measured".to_vec(),
                    Some(ExecutionMeasurement {
                        fuel_consumed: self.fuel,
                        duration_ms: 7,
                        peak_memory_bytes: None,
                    }),
                ))
            }
        }
    }

    fn wasm_node(executor: Arc<dyn ToolExecutor>) -> WasmSOPNode {
        WasmSOPNode {
            name: "burn_node".into(),
            tool_id: "t".into(),
            fuel_limit: 100_000,
            executor,
            args: Vec::new(),
        }
    }

    /// 有测量能力时必须写入 ctx，且输出走测量路径。
    #[tokio::test]
    async fn test_measured_path_records_measurement() {
        let node = wasm_node(Arc::new(MeasuringExecutor {
            fuel: 4321,
            fail_measured: false,
        }));
        let ctx = ExecutionContext::new();
        let out = node.execute(&ctx).await.expect("execute must succeed");
        assert_eq!(out.0.as_str(), Some("measured"));
        let m = ctx.take_measurement("burn_node").expect("measurement must be recorded");
        assert_eq!(m.fuel_consumed, 4321);
        // take 语义：消费后不残留
        assert!(ctx.take_measurement("burn_node").is_none());
    }

    /// 无测量能力的执行器（只用默认 execute_measured，返回 None）。
    struct PlainExecutor;
    #[async_trait]
    impl ToolExecutor for PlainExecutor {
        async fn execute(
            &self,
            _tool_id: &str,
            _args: Vec<u8>,
            _fuel_limit: u64,
        ) -> Result<Vec<u8>, TypesError> {
            Ok(b"plain".to_vec())
        }
        // execute_measured 不覆写：走 trait 默认实现（None 测量）
    }

    /// 无测量能力（trait 默认实现返回 None）时不得写入 ctx，行为与旧版一致。
    #[tokio::test]
    async fn test_unmeasured_executor_leaves_ctx_clean() {
        let node = wasm_node(Arc::new(PlainExecutor));
        let ctx = ExecutionContext::new();
        let _ = node.execute(&ctx).await;
        assert!(
            ctx.get_measurement("burn_node").is_none(),
            "默认 execute_measured 返回 None 时不得产生测量"
        );
    }

    /// 测量路径失败时回退普通执行，保证兼容。
    #[tokio::test]
    async fn test_fallback_on_measured_failure() {
        let node = wasm_node(Arc::new(MeasuringExecutor {
            fuel: 1,
            fail_measured: true,
        }));
        let ctx = ExecutionContext::new();
        let out = node.execute(&ctx).await.expect("fallback must succeed");
        assert_eq!(out.0.as_str(), Some("plain"));
        assert!(ctx.get_measurement("burn_node").is_none());
    }
}
