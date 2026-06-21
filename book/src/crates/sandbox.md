# sisyphus-sandbox — WASM Runtime

Provides a sandboxed WASM runtime for executing tool code safely within the cognitive loop.

## Key Component: `WasmToolExecutor`

```rust
pub struct WasmToolExecutor {
    engine: Engine,
    module_cache: Arc<Mutex<HashMap<String, Module>>>,
}
```

### Construction

`WasmToolExecutor::new()` creates a Wasmtime `Engine` with:

- **Pooling instance allocation**: Pre-allocated memory pools for fast instantiation.
- **Memory limits**: Max 1 MB memory per instance (`max_memory_size(10 * 65536)`).
- **Fuel metering**: Configurable fuel limits to prevent runaway execution.

### Execution Flow

1. **Load**: Read WASM binary from the filesystem (path: `wasm_tools/{tool_id}.wasm`).
2. **Compile**: Compile into a Wasmtime `Module` (with caching to avoid recompilation).
3. **Instantiate**: Create an instance with fuel and memory limits.
4. **Call**: Invoke the `run` export function with input bytes.
5. **Return**: Return the result bytes or an error (fuel exhaustion, memory exhaustion, etc.).

### Fuel System

Wasmtime's fuel metering tracks WASM instruction consumption. When fuel runs out, execution is immediately aborted with a `ResourceExhausted` error, which the cognitive loop interprets as a signal to retry with more fuel.

## Console Output Capture

The executor captures WASM module stdout/stderr via `WasmtimeConsole` and routes it through `tracing`.
