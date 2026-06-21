# sisyphus-core — Kernel & Orchestration

The central crate that orchestrates the entire cognitive loop. It depends on all other crates.

## Key Components

### `SisyphusKernel`

The main entry point for the cognitive loop. Holds references to the SOP synthesizer, simulator, reflector, evolver, memory store, and WASM tool executor.

```rust
pub struct SisyphusKernel {
    pub intel: Arc<SisyphusIntel>,
    pub mem: Arc<dyn MemoryStore>,
    pub executor: Arc<WasmToolExecutor>,
}
```

### `CognitiveLoop::execute_goal()`

The primary cognitive cycle:

1. **Synthesize**: Convert natural-language goal into SOP graph via `SisyphusIntel::synthesize_sop()`.
2. **Execute**: Run the SOP graph using the DAG executor, respecting node dependencies.
3. **Simulate**: Run Monte Carlo simulation over execution traces via `StandardReflector::simulate_paths()`.
4. **Reflect**: Compare predicted vs. actual outcomes via `StandardReflector::reflect()`.
5. **Evolve**: If deviation > threshold, mutate the SOP via `StandardReflector::evolve()`.
6. **Loop**: Repeat steps 2–5 until convergence or max iterations.

### `DagExecutor`

Executes a `SOPGraph` with topological ordering. Nodes with zero in-degree execute in parallel via `tokio::task::JoinSet`. Results propagate through the graph via `Arc<ExecutionContext>` sharing.

## CLI

The crate includes a binary `integration_stress` that runs an end-to-end in-process integration test.
