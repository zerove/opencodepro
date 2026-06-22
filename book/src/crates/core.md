# sisyphus-core — Kernel & Orchestration

The central crate that orchestrates the entire cognitive loop. Depends on all other crates.

## Key Types

### `SisyphusKernel`

Main entry point for the cognitive loop. Holds references to all subsystems:

```rust
pub struct SisyphusKernel {
    pub state_store: Arc<dyn StateStore>,
    pub tool_executor: Arc<dyn ToolExecutor>,
    pub intel: Arc<SisyphusIntel>,
    pub synthesizer: Arc<dyn FlowSynthesizer>,
}
```

#### Core Methods

| Method | Description |
|--------|-------------|
| `execute_goal(goal, sop_id)` | Full pipeline: synthesize → execute → cognize → evolve |
| `execute_goals(goals)` | Execute multiple goals concurrently, return composite output |
| `execute_with_cognition(graph, start, sop_id, ctx)` | Multi-iteration loop: predict → execute → reflect → evolve |
| `load_sop(sop_id)` | Load a previously saved SOP graph from the state store |
| `save_sop(sop_id, graph)` | Persist an SOP graph to the state store |

### `Executor`

The DAG runner that executes SOP graphs with topological parallelism:

```rust
let ctx = Executor::new()
    .execute_with_context(&sop_graph, start_node, None)
    .await?;
```

### `CognitiveReport`

Returned from `execute_with_cognition`:

```rust
pub struct CognitiveReport {
    pub deviation_score: f64,
    pub iterations: usize,
    pub trace: String,
    pub improvements: Vec<String>,
}
```

### `DependencyResolver`

Resolves dependency mappings across composite graphs:

```rust
pub fn resolve(composite: &CompositeGraph) -> Result<Vec<String>, String>;
```

## Execution Flow

1. **Synthesize**: `FlowSynthesizer::synthesize()` converts a natural-language goal into an `SOPGraph`
2. **Execute**: `Executor::execute_with_context()` runs the DAG with topological ordering — independent nodes run in parallel via `tokio::task::JoinSet`
3. **Simulate**: `MonteCarloSimulator` estimates success probability and cost via random path sampling
4. **Reflect**: `StandardReflector` compares prediction vs actual trace, producing a deviation score
5. **Evolve**: If deviation > threshold, `StandardEvolver` mutates the SOP (fuel adjustment, retry wrappers)
6. **Loop**: Repeat until convergence or max iterations

## Benchmark Binary

`cargo run --release -p sisyphus-core --bin integration_stress` runs an end-to-end in-process stress test.

## Example

```rust
use sisyphus_core::{SisyphusKernel, Executor, SOPGraph, WasmSOPNode};
use sisyphus_sandbox::WasmToolExecutor;
use std::sync::Arc;

// Build a single-node SOP
let executor = Arc::new(WasmToolExecutor::new()?);
let tool = WasmSOPNode {
    name: "greet".into(),
    tool_id: "hello".into(),
    executor: executor.clone(),
    fuel_limit: 1000,
};
let mut graph = SOPGraph::new();
let n = graph.add_node(Arc::new(tool));
let ctx = Executor::new().execute_with_context(&graph, n, None).await?;
```
