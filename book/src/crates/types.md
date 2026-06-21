# sisyphus-types — Core Data Structures

Standalone crate (zero dependencies) providing the data model used by all other crates.

## Key Types

### `SOPGraph`

A directed acyclic graph (DAG) of nodes, using `petgraph::graph::DiGraph`. Each node is a `SOPNode` containing metadata like name, description, tool type, and required fuel.

```rust
pub struct SOPGraph {
    pub graph: DiGraph<SOPNode, f64>,
    pub metadata: SOPMetadata,
}
```

### `SOPNode`

A single node in the SOP graph.

```rust
pub struct SOPNode {
    name: String,
    description: String,
    tool_type: String,
    fuel_budget: u64,
    max_retries: u32,
    retry_count: u32,
    timeout_ms: u64,
}
```

### `ExecutionContext`

Thread-safe shared context for storing named values (`String` → `serde_json::Value`). Passed as `Arc<ExecutionContext>` through the DAG, enabling parallel node access.

### `ExecutionResult`

Result of a single node execution, containing the output value, execution duration, and fuel consumed.

### `CognitiveReport`

Full report from a cognitive loop iteration, including:
- Channel trace with step-by-step reasoning
- Reflection report with deviation scores
- Simulation result with predicted success probability

### `Config`

TOML-deserializable configuration struct for all Sisyphus components (fuel limits, pool sizes, paths, etc.).
