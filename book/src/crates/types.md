# sisyphus-types — Core Data Structures

Shared types, traits, and configuration used by all other crates.

## Key Types

### `SOPGraph`

A directed acyclic graph (DAG) using `petgraph::graph::DiGraph<Arc<dyn SOPNode>, f64>`:

```rust
pub struct SOPGraph {
    pub graph: petgraph::graph::DiGraph<Arc<dyn SOPNode>, f64>,
    pub metadata: SOPGraphConfig,
}
```

Each node implements the `SOPNode` trait:

```rust
pub trait SOPNode: Send + Sync {
    fn name(&self) -> &str;
    fn execute(&self, ctx: &ExecutionContext) -> impl Future<Output = Result<NodeOutput, String>>;
}
```

### `WasmSOPNode`

Concrete node implementation backed by a WASM tool:

```rust
pub struct WasmSOPNode {
    pub name: String,
    pub tool_id: String,
    pub executor: Arc<dyn ToolExecutor>,
    pub fuel_limit: u64,
}
```

### `ExecutionContext`

Thread-safe shared context for propagating values through the DAG. Internally an `Arc<RwLock<HashMap<String, Value>>>`.

```rust
let ctx = ExecutionContext::new();
ctx.set("key", serde_json::json!("value")).await;
let val = ctx.get_result("key").await;
```

### `ToolExecutor` Trait

The abstraction that decouples `sisyphus-core` from `sisyphus-sandbox`:

```rust
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(&self, tool_id: &str, args: Vec<u8>, fuel_limit: u64) -> Result<Vec<u8>, String>;
}
```

### `StateStore` Trait

Storage abstraction decoupling `sisyphus-core` from `sisyphus-mem`:

```rust
#[async_trait]
pub trait StateStore: Send + Sync {
    async fn get_state_json(&self, key: &str) -> Result<Option<serde_json::Value>, String>;
    async fn set_state_json(&self, key: &str, value: serde_json::Value) -> Result<(), String>;
}
```

### `Config`

TOML-deserializable configuration for all subsystems. Supports environment variable overrides:

```rust
let config = Config::from_file("config.toml")?;
config.apply_env_overrides();
// SISYPHUS_METRICS__PROMETHEUS_PORT=9090 overrides metrics.prometheus_port
```

### Other Key Types

| Type | Description |
|------|-------------|
| `CompositeGraph` | Multiple `SOPGraph`s with cross-graph dependency mappings |
| `DataMapping` | Defines how a sink node's output feeds a downstream graph's input |
| `GraphDependency` | Dependency edge between two SOP graphs |
| `ExecutionHistory` | Per-SOP execution records with timestamps and deviation scores |
| `CognitiveReport` | Full iteration report from the cognitive loop |
| `NodeOutput` | Typed enum for node execution results |
