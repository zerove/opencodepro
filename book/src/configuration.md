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
use_sandboxd = false
# sandboxd_path = "sisyphus-sandboxd"
```

### `[sandbox]` 字段

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `max_memory_pages` | `u32` | `10` | WASM 线性内存上限（以 64KB 页为单位） |
| `default_fuel` | `u64` | `1_000_000` | 沙箱默认 fuel 上限（WASM 指令数） |
| `wasm_tools_dir` | `string` | `"tools"` | WASM 工具二进制文件目录 |
| `use_sandboxd` | `bool` | `false` | 使用独立进程 sandboxd 替代嵌入式 wasmtime。为 `true` 时 bridge 启动 `sisyphus-sandboxd` 子进程执行 WASM，避免 wasmtime 编译内存压力 |
| `sandboxd_path` | `string` | `None`（搜索 PATH） | `sisyphus-sandboxd` 二进制路径，仅 `use_sandboxd = true` 时有效 |

> **何时启用 sandboxd：** 如果 CI 或开发机上 `cargo check`/`cargo build` 因 wasmtime 的 LLVM codegen 内存峰值 >7GB 而 OOM，设置 `use_sandboxd = true` 并在 CI 中预先构建 `sisyphus-sandboxd` 二进制。bridge 将启动子进程处理 WASM 调用，自身不再链接 wasmtime。

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
| `SISYPHUS_SANDBOX_USE_SANDBOXD` | `sandbox.use_sandboxd` | bool | `false` |
| `SISYPHUS_SANDBOX_SANDBOXD_PATH` | `sandbox.sandboxd_path` | string | `None` |
| `SISYPHUS_LLM_API_KEY` | `llm.api_key` | string | `""` |
| `SISYPHUS_LLM_ENDPOINT` | `llm.endpoint` | string | `"http://localhost:8000/v1/chat/completions"` |
| `SISYPHUS_LLM_MODEL` | `llm.model` | string | `"gpt-4o"` |
| `SISYPHUS_LLM_BUDGET_MODE` | `llm.budget_mode` | string | `"Warning"` |
| `SISYPHUS_LLM_EMBEDDING_MODEL` | `llm.embedding.model` | string | `"text-embedding-3-small"` |
| `SISYPHUS_LLM_EMBEDDING_ENDPOINT` | `llm.embedding.endpoint` | string | `None` |
| `SISYPHUS_LLM_EMBEDDING_PROVIDER` | `llm.embedding.provider` | string | `"dummy"` |
| `SISYPHUS_REGISTRY_URL` | `registry.registry_url` | string | `"https://registry.sisyphus.ai"` |
| `SISYPHUS_REGISTRY_TIMEOUT_SECONDS` | `registry.timeout_seconds` | u64 | `30` |
| `SISYPHUS_JCODE_SOCKET_PATH` | `jcode.socket_path` | string | `"/tmp/jcode-debug.sock"` |
| `SISYPHUS_AUTH_TOKEN` | `auth_token` | string | `""` |
| `SISYPHUS_BRIDGE_ADDRESS` | `bridge_address` | string | `"http://[::1]:50051"` |
| `SISYPHUS_HTTP_ADDRESS` | `http_address` | string | `"http://[::1]:8080"` |
| `SISYPHUS_CHAOS_ENABLED` | `chaos.enabled` | bool | `false` |
| `SISYPHUS_CHAOS_DEFAULT_FAULT_PROBABILITY` | `chaos.default_fault_probability` | f64 | `0.1` |
| `SISYPHUS_CHAOS_DEFAULT_DURATION_SECONDS` | `chaos.default_duration_seconds` | u64 | `300` |
