# Sisyphus Sandbox Agent Knowledge Base

## OVERVIEW
Wasmtime-based sandbox execution with fuel metering, memory limits, and module caching.

## WHERE TO LOOK
- General execution: `WasmToolExecutor` in `src/lib.rs` (bytecode load, instantiate, run)
- Schema-validated execution: `CustomToolExecutor` in `src/custom_executor.rs` (JSON Schema + WASM)
- Module cache: `module_cache` (DashMap) in both executors for hot module reuse
- Integration tests: `tests/wasm_test.rs` (end-to-end wasm execution and trap verification)

## CONVENTIONS
- Wasmtime version pinned to v45 in `Cargo.toml`
- Fuel metering enabled by default to prevent runaway execution
- Memory limits enforced via `PoolingAllocationConfig` (default 10 pages)
- All WASM tools must export an `execute` function and a `memory` export for result reading

## COMMANDS
```bash
cargo test -p sisyphus-sandbox        # Run all sandbox tests (requires wasmtime)
cargo check -p sisyphus-sandbox        # Check sandbox crate (heavy cranelift dependency)
```
