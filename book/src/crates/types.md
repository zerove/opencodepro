# sisyphus-types — 核心数据结构

其他所有 crate 共享的类型、traits 与配置。

## 关键类型

### `SOPGraph`

使用 `petgraph::graph::DiGraph<Arc<dyn SOPNode>, f64>` 的有向无环图（DAG）：

```rust
pub struct SOPGraph {
    pub graph: petgraph::graph::DiGraph<Arc<dyn SOPNode>, f64>,
    pub metadata: SOPGraphConfig,
}
```

每个节点实现 `SOPNode` trait：

```rust
pub trait SOPNode: Send + Sync {
    fn name(&self) -> &str;
    fn execute(&self, ctx: &ExecutionContext) -> impl Future<Output = Result<NodeOutput, String>>;
}
```

### `WasmSOPNode`

由 WASM 工具支持的具象节点实现：

```rust
pub struct WasmSOPNode {
    pub name: String,
    pub tool_id: String,
    pub executor: Arc<dyn ToolExecutor>,
    pub fuel_limit: u64,
}
```

### `ExecutionContext`

线程安全的共享上下文，用于在 DAG 中传播值。内部为 `Arc<RwLock<HashMap<String, Value>>>`。

```rust
let ctx = ExecutionContext::new();
ctx.set("key", serde_json::json!("value")).await;
let val = ctx.get_result("key").await;
```

### `ToolExecutor` Trait

将 `sisyphus-core` 与 `sisyphus-sandbox` 解耦的抽象：

```rust
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(&self, tool_id: &str, args: Vec<u8>, fuel_limit: u64) -> Result<Vec<u8>, String>;
}
```

### `StateStore` Trait

将 `sisyphus-core` 与 `sisyphus-mem` 解耦的存储抽象：

```rust
#[async_trait]
pub trait StateStore: Send + Sync {
    async fn get_state_json(&self, key: &str) -> Result<Option<serde_json::Value>, String>;
    async fn set_state_json(&self, key: &str, value: serde_json::Value) -> Result<(), String>;
}
```

### `Config`

所有子系统均可反序列化 TOML 的配置，支持环境变量覆盖：

```rust
let config = Config::from_file("config.toml")?;
config.apply_env_overrides();
// SISYPHUS_METRICS__PROMETHEUS_PORT=9090 覆盖 metrics.prometheus_port
```

### 其他关键类型

| 类型 | 说明 |
|------|-------------|
| `CompositeGraph` | 多个 `SOPGraph` 及跨图依赖映射 |
| `DataMapping` | 定义汇节点输出如何馈入下游图的输入 |
| `GraphDependency` | 两个 SOP 图之间的依赖边 |
| `ExecutionHistory` | 每个 SOP 的执行记录，含时间戳与偏差分数 |
| `CognitiveReport` | 认知循环的完整迭代报告 |
| `NodeOutput` | 节点执行结果的类型化枚举 |