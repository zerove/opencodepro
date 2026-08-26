# 配置

Sisyphus 通过 TOML 文件配置，反序列化为 `sisyphus_types::Config` 结构体。

项目根目录提供了示例配置文件 `config.example.toml`。

## 默认配置

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

## JWT 认证

可通过 `[jwt]` 为 REST API 启用基于 token 的 JWT 认证：

```toml
[jwt]
secret = "your-256-bit-secret"
access_ttl_secs = 900
refresh_ttl_secs = 604800
issuer = "sisyphus-bridge"
```

### `[jwt]` 字段

| 字段 | 类型 | 默认值 | 说明 |
|-------|------|---------|-------------|
| `secret` | `string` | `""` | 签名密钥。生产环境必须设置；为空时不启用 JWT 认证 |
| `access_ttl_secs` | `i64` | `900` | Access token 有效期（秒），默认 15 分钟 |
| `refresh_ttl_secs` | `i64` | `604800` | Refresh token 有效期（秒），默认 7 天 |
| `issuer` | `string` | `"sisyphus-bridge"` | Token 签发者标识 |

## OpenTelemetry 分布式追踪

可通过 `[metrics.opentelemetry]` 启用 OpenTelemetry 追踪：
```toml
[metrics]
prometheus_port = 9100

[metrics.opentelemetry]
enabled = false
endpoint = "http://localhost:4317"
service_name = "sisyphus-bridge"
sample_rate = 1.0
```

### `[metrics.opentelemetry]` 字段

| 字段 | 类型 | 默认值 | 说明 |
|-------|------|---------|-------------|
| `enabled` | `bool` | `false` | 启用 OpenTelemetry OTLP 导出 |
| `endpoint` | `string` | `"http://localhost:4317"` | OTLP 导出端点 |
| `service_name` | `string` | `"sisyphus-bridge"` | 服务名称用于追踪标识 |
| `sample_rate` | `f64` | `1.0` | 采样率 (0.0 - 1.0) |

## 数据库后端

`[cache]` 中的 `database_url` 支持两种后端：

| URL 前缀 | 后端 | 向量支持 |
|------------|---------|----------------|
| `sqlite:`   | SQLite (默认) | 全扫描 + 内存余弦相似度 |
| `postgres://` | PostgreSQL | pgvector ANN (`<->` 操作符) |

### PostgreSQL / pgvector

用于带 ANN 索引的语义相似度搜索：

1. 在 Postgres 服务器安装 `vector` 扩展
2. 设置 `database_url = "postgres://user:pass@host/sisyphus"`
3. 设置 `enable_vectors = true` 和 `vector_dimension = 1536`（或你的嵌入模型维度）
4. SqlStore 连接时自动运行 `CREATE EXTENSION IF NOT EXISTS vector`

## 覆盖优先级

1. **默认值**（各子配置 `Default` 实现中硬编码）
2. **配置文件**（通过 `SISYPHUS_CONFIG` 环境变量或 `Config::from_file()` 指定路径）
3. **环境变量**（`SISYPHUS_` 前缀，最高优先级）

## 环境变量

所有环境变量使用前缀 `SISYPHUS_`：

| 变量 | 覆盖字段 | 类型 | 默认值 |
|----------|-----------|------|---------|
| `SISYPHUS_CONFIG` | 配置文件路径 | string | `config.toml` |
| `SISYPHUS_COGNITIVE_LOOP_MAX_ITERATIONS` | `cognitive_loop.max_iterations` | u32 | `10` |
| `SISYPHUS_COGNITIVE_LOOP_LLM_STAGE_TIMEOUT_SECS` | `cognitive_loop.llm_stage_timeout_secs` | u64 | `30` |
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
| `SISYPHUS_CACHE_BATCH_SIZE` | `cache.batch_size` | usize | `100` |
| `SISYPHUS_CACHE_FLUSH_INTERVAL_MS` | `cache.flush_interval_ms` | u64 | `100` |
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
| `SISYPHUS_LLM_FAST_FAIL_COOLING_SECS` | `llm.fast_fail_cooling_secs` | u64 | `30` |
| `SISYPHUS_LLM_REQUEST_TIMEOUT_SECS` | `llm.request_timeout_secs` | u64 | `5` |
| `SISYPHUS_LLM_CONNECT_TIMEOUT_SECS` | `llm.connect_timeout_secs` | u64 | `2` |
| `SISYPHUS_LLM_EMBEDDING_MODEL` | `llm.embedding.model` | string | `"text-embedding-3-small"` |
| `SISYPHUS_LLM_EMBEDDING_ENDPOINT` | `llm.embedding.endpoint` | string | `None` |
| `SISYPHUS_LLM_EMBEDDING_PROVIDER` | `llm.embedding.provider` | string | `"dummy"` |
| `SISYPHUS_REGISTRY_URL` | `registry.registry_url` | string | `"https://registry.sisyphus.ai"` |
| `SISYPHUS_REGISTRY_TIMEOUT_SECONDS` | `registry.timeout_seconds` | u64 | `30` |
| `SISYPHUS_JCODE_SOCKET_PATH` | `jcode.socket_path` | string | `"/tmp/jcode-debug.sock"` |
| `SISYPHUS_AUTH_TOKEN` | `auth_token` | string | `""` |
| `SISYPHUS_JWT_SECRET` | `jwt.secret` | string | `""` |
| `SISYPHUS_JWT_ACCESS_TTL_SECS` | `jwt.access_ttl_secs` | i64 | `900` |
| `SISYPHUS_JWT_REFRESH_TTL_SECS` | `jwt.refresh_ttl_secs` | i64 | `604800` |
| `SISYPHUS_JWT_ISSUER` | `jwt.issuer` | string | `"sisyphus-bridge"` |
| `SISYPHUS_BRIDGE_ADDRESS` | `bridge_address` | string | `"http://[::1]:50051"` |
| `SISYPHUS_HTTP_ADDRESS` | `http_address` | string | `"http://[::1]:8080"` |
| `SISYPHUS_CHAOS_ENABLED` | `chaos.enabled` | bool | `false` |
| `SISYPHUS_CHAOS_DEFAULT_FAULT_PROBABILITY` | `chaos.default_fault_probability` | f64 | `0.1` |
| `SISYPHUS_CHAOS_DEFAULT_DURATION_SECONDS` | `chaos.default_duration_seconds` | u64 | `300` |
| `SISYPHUS_METRICS_OTEL_ENABLED` | `metrics.opentelemetry.enabled` | bool | `false` |
| `SISYPHUS_METRICS_OTEL_ENDPOINT` | `metrics.opentelemetry.endpoint` | string | `"http://localhost:4317"` |
| `SISYPHUS_METRICS_OTEL_SERVICE_NAME` | `metrics.opentelemetry.service_name` | string | `"sisyphus-bridge"` |
| `SISYPHUS_METRICS_OTEL_SAMPLE_RATE` | `metrics.opentelemetry.sample_rate` | f64 | `1.0` |
