# sisyphus-sandbox — WASM Runtime

Provides a sandboxed WASM runtime for executing tool code safely using [Wasmtime](https://wasmtime.dev/).

## Key Type: `WasmToolExecutor`

```rust
pub struct WasmToolExecutor {
    engine: Engine,
    tools_path: PathBuf,
    module_cache: Arc<DashMap<String, Module>>,
}
```

### Construction

```rust
// Default — looks for tools/ relative to CWD
let exec = WasmToolExecutor::new()?;

// Custom path and memory limit
let exec = WasmToolExecutor::with_config(
    Some(PathBuf::from("/opt/tools")),
    Some(20),  // pages (20 × 64 KiB = 1.25 MiB)
)?;
```

Creates a Wasmtime `Engine` with:

- **Pooling instance allocation**: Pre-allocated memory pools for fast instantiation
- **Memory limits**: Configurable max memory per instance (default: 10 pages = 640 KiB)
- **Fuel metering**: Configurable fuel limits to prevent runaway execution (default: 10,000)
- **Stack limit**: 1,000,000 max WASM stack frames

### Execution Flow

1. **Load**: Read `.wasm` binary from `{tools_path}/{tool_id}.wasm`
2. **Compile**: Compile into a Wasmtime `Module` (cached in `DashMap` to avoid recompilation)
3. **Instantiate**: Create an instance with per-call fuel allocation
4. **Call**: Invoke the exported `run` function, passing input bytes
5. **Return**: Return output bytes, or a fuel/memory exhaustion error

```rust
#[async_trait]
impl ToolExecutor for WasmToolExecutor {
    async fn execute(&self, tool_id: &str, args: Vec<u8>, fuel_limit: u64) -> Result<Vec<u8>, String> {
        // ...
    }
}
```

### Error Handling

| Error | Meaning | Cognitive Loop Response |
|-------|---------|------------------------|
| `ResourceExhausted: fuel` | Ran out of WASM instructions | Retry with 2× fuel |
| `ResourceExhausted: memory` | Exceeded linear memory limit | Fail — redesign node |
| `Unknown export: run` | WASM module has no `run` export | Fail — invalid tool |
| I/O error | Tool file not found | Fail — missing tool |

### Fuel System

Wasmtime's fuel metering tracks WASM instruction consumption. When fuel runs out, execution is immediately aborted. The cognitive loop interprets this as a signal to retry with increased fuel via `AdaptiveFuelPricer`.

### Console Output Capture

WASM module stdout/stderr is captured via `WasmtimeConsole` and routed through `tracing`.
