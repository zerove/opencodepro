# Configuration

Sisyphus is configured via TOML files deserialized into the `sisyphus_types::Config` struct.

## Default Configuration

```toml
[cognitive_loop]
max_iterations = 10
convergence_threshold = 0.15

[executor]
max_concurrent_tasks = 16
default_fuel = 100000

[cache]
l1_capacity = 10000
sqlite_path = "data/sisyphus.db"

[metrics]
prometheus_port = 9100

[sandbox]
max_memory_pages = 10
default_fuel = 100000
wasm_tools_dir = "wasm_tools"
```

## Override Order

1. **Default values** (hardcoded in `Config::default()`)
2. **Config file** (path set via `--config` flag or `SISYPHUS_CONFIG` env var)
3. **Environment variables** (prefixed with `SISYPHUS_`, e.g. `SISYPHUS_COGNITIVE_LOOP_MAX_ITERATIONS=20`)

## Environment Variables

| Variable | Overrides | Default |
|----------|-----------|---------|
| `SISYPHUS_CONFIG` | Config file path | — |
| `SISYPHUS_COGNITIVE_LOOP_MAX_ITERATIONS` | `cognitive_loop.max_iterations` | 10 |
| `SISYPHUS_EXECUTOR_DEFAULT_FUEL` | `executor.default_fuel` | 100000 |
| `SISYPHUS_SANDBOX_WASM_TOOLS_DIR` | `sandbox.wasm_tools_dir` | `wasm_tools` |
