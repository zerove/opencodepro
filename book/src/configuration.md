# Configuration

Sisyphus is configured via TOML files deserialized into the `sisyphus_types::Config` struct.

An example configuration file is provided at `config.example.toml` in the project root.

## Default Configuration

```toml
[cognitive_loop]
max_iterations = 10
convergence_threshold = 0.15

[cognitive_loop.adaptive_depth]
initial_budget = 2.0
decay = 0.3
min_improvement = 0.15
convergence_threshold = 0.05

[executor]
max_concurrent_tasks = 16
default_fuel = 1_000_000

[cache]
l1_capacity = 10_000
database_url = "sqlite:data/sisyphus.db"
enable_vectors = false
vector_dimension = 1536

[metrics]
prometheus_port = 9100

[sandbox]
max_memory_pages = 10
default_fuel = 1_000_000
wasm_tools_dir = "tools"
```

## Database Backends

The `database_url` in `[cache]` supports two backends:

| URL Prefix | Backend | Vector Support |
|------------|---------|----------------|
| `sqlite:`   | SQLite (default) | Full scan + memory cosine similarity |
| `postgres://` | PostgreSQL | pgvector ANN (`<->` operator) |

### PostgreSQL / pgvector

For semantic similarity search with ANN indexes:

1. Install the `vector` extension on your Postgres server
2. Set `database_url = "postgres://user:pass@host/sisyphus"`
3. Set `enable_vectors = true` and `vector_dimension = 1536` (or your embedding model's dimension)
4. SqlStore automatically runs `CREATE EXTENSION IF NOT EXISTS vector` on connect

## Override Order

1. **Default values** (hardcoded in each sub-config's `Default` impl)
2. **Config file** (path set via `SISYPHUS_CONFIG` env var or `Config::from_file()`)
3. **Environment variables** (prefixed with `SISYPHUS_`, highest priority)

## Environment Variables

All environment variables use the prefix `SISYPHUS_`:

| Variable | Overrides | Type | Default |
|----------|-----------|------|---------|
| `SISYPHUS_CONFIG` | Config file path | string | `config.toml` |
| `SISYPHUS_COGNITIVE_LOOP_MAX_ITERATIONS` | `cognitive_loop.max_iterations` | u32 | `10` |
| `SISYPHUS_COGNITIVE_LOOP_CONVERGENCE_THRESHOLD` | `cognitive_loop.convergence_threshold` | f64 | `0.15` |
| `SISYPHUS_COGNITIVE_LOOP_ADAPTIVE_DEPTH_INITIAL_BUDGET` | `cognitive_loop.adaptive_depth.initial_budget` | f64 | `2.0` |
| `SISYPHUS_COGNITIVE_LOOP_ADAPTIVE_DEPTH_DECAY` | `cognitive_loop.adaptive_depth.decay` | f64 | `0.3` |
| `SISYPHUS_COGNITIVE_LOOP_ADAPTIVE_DEPTH_MIN_IMPROVEMENT` | `cognitive_loop.adaptive_depth.min_improvement` | f64 | `0.15` |
| `SISYPHUS_COGNITIVE_LOOP_ADAPTIVE_DEPTH_CONVERGENCE_THRESHOLD` | `cognitive_loop.adaptive_depth.convergence_threshold` | f64 | `0.05` |
| `SISYPHUS_EXECUTOR_MAX_CONCURRENT_TASKS` | `executor.max_concurrent_tasks` | usize | `16` |
| `SISYPHUS_EXECUTOR_DEFAULT_FUEL` | `executor.default_fuel` | u64 | `1_000_000` |
| `SISYPHUS_CACHE_L1_CAPACITY` | `cache.l1_capacity` | usize | `10_000` |
| `SISYPHUS_CACHE_DATABASE_URL` | `cache.database_url` | string | `sqlite:data/sisyphus.db` |
| `SISYPHUS_CACHE_ENABLE_VECTORS` | `cache.enable_vectors` | bool | `false` |
| `SISYPHUS_CACHE_VECTOR_DIMENSION` | `cache.vector_dimension` | usize | `1536` |
| `SISYPHUS_METRICS_PROMETHEUS_PORT` | `metrics.prometheus_port` | u16 | `9100` |
| `SISYPHUS_SANDBOX_MAX_MEMORY_PAGES` | `sandbox.max_memory_pages` | u32 | `10` |
| `SISYPHUS_SANDBOX_DEFAULT_FUEL` | `sandbox.default_fuel` | u64 | `1_000_000` |
| `SISYPHUS_SANDBOX_WASM_TOOLS_DIR` | `sandbox.wasm_tools_dir` | string | `tools` |
