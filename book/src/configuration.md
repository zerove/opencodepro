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
issuer = "sisyphus-bridge"
```

### `[jwt]` 字段

| 字段 | 类型 | 默认值 | 说明 |
|-------|------|---------|-------------|
| `secret` | `string` | `""` | 签名密钥。生产环境必须设置；为空时不启用 JWT 认证 |
| `access_ttl_secs` | `i64` | `900` | Access token 有效期（秒），默认 15 分钟 |
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
| `SISYPHUS_COGNITIVE_LOOP_TOKEN_BUDGET_CAP` | `cognitive_loop.token_budget.cap` | u64 \| 空字符串清空 | `None` |
| `SISYPHUS_COGNITIVE_LOOP_TOKEN_BUDGET_RESERVE_RATIO` | `cognitive_loop.token_budget.reserve_ratio` | f64 | `0.1` |
| `SISYPHUS_COGNITIVE_LOOP_ANOMALY_DETECTOR_FUEL_THRESHOLD` | `cognitive_loop.anomaly_detector.fuel_threshold` | f64 (> 0) | `2.0` |
| `SISYPHUS_COGNITIVE_LOOP_ANOMALY_DETECTOR_DURATION_THRESHOLD` | `cognitive_loop.anomaly_detector.duration_threshold` | f64 (> 0) | `3.0` |
| `SISYPHUS_COGNITIVE_LOOP_ANOMALY_DETECTOR_ERROR_RATE_THRESHOLD` | `cognitive_loop.anomaly_detector.error_rate_threshold` | f64 [0.0, 1.0] | `0.3` |
| `SISYPHUS_COGNITIVE_LOOP_ANOMALY_DETECTOR_HISTORY_WINDOW` | `cognitive_loop.anomaly_detector.history_window` | usize (≥ 1) | `20` |
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
| `SISYPHUS_LLM_EMBEDDING_ALLOW_DUMMY` | `llm.embedding.allow_dummy_embeddings` | bool | `false` |
| `SISYPHUS_REGISTRY_URL` | `registry.registry_url` | string | `"https://registry.sisyphus.ai"` |
| `SISYPHUS_REGISTRY_TIMEOUT_SECONDS` | `registry.timeout_seconds` | u64 | `30` |
| `SISYPHUS_JCODE_SOCKET_PATH` | `jcode.socket_path` | string | `"/tmp/jcode-debug.sock"` |
| `SISYPHUS_AUTH_TOKEN` | `auth_token` | string | `""` |
| `SISYPHUS_JWT_SECRET` | `jwt.secret` | string | `""` |
| `SISYPHUS_JWT_ACCESS_TTL_SECS` | `jwt.access_ttl_secs` | i64 | `900` |
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
| `SISYPHUS_CI` | — | 设为 `1` 表示 CI 环境, 影响 SCM 工具行为 (跳过非必要网络调用) |
| `SISYPHUS_DATA_DIR` | — | CLI 数据目录覆盖 (`--data-dir` 同效) |
| `SISYPHUS_OUTPUT_FORMAT` | — | SCM 工具输出格式覆盖 (`json` / `text`) |

## Provider 适配（OpenAI 兼容协议）

`ProviderKind` 已从 3 个扩展到 7 个。OpenAI 兼容类（OpenAI/Groq/Mistral/Together）共享
`OpenAiCompatibleProvider` 客户端，零代码差异——仅 endpoint 与 model 不同。

| Kind | 默认 Endpoint | 默认 Model | 兼容性 |
|------|--------------|------------|--------|
| `openai` | `https://api.openai.com/v1/chat/completions` | `gpt-4o-mini` | OpenAI 兼容 |
| `google` | (custom protocol) | `gemini-1.5-flash` | 自定义 |
| `anthropic` | (custom protocol) | `claude-3-5-sonnet-20241022` | 自定义 |
| `groq` | `https://api.groq.com/openai/v1/chat/completions` | `llama-3.1-70b-versatile` | OpenAI 兼容 |
| `cohere` | (not yet implemented) | `command-r-plus` | 自定义 |
| `mistral` | `https://api.mistral.ai/v1/chat/completions` | `mistral-large-latest` | OpenAI 兼容 |
| `together` | `https://api.together.xyz/v1/chat/completions` | `meta-llama/Llama-3.3-70B-Instruct-Turbo` | OpenAI 兼容 |
| `openrouter` | `https://openrouter.ai/api/v1/chat/completions` | `anthropic/claude-3.5-sonnet` | OpenAI 兼容 |

切换生产 provider 只需修改 `config.toml` 的 `[[llm.providers]]` 段。优先级（`priority` 字段）控制 fallback 链顺序——数值越小越优先。

### OpenRouter 归因 headers

OpenRouter 透传 OpenAI 协议；客户端在 provider 声明为 `openrouter` 时自动注入两个推荐 header：

- `HTTP-Referer`：归因站点，默认 `https://sisyphus.dev`，可通过 `base_url` 字段覆盖
- `X-Title`：固定为 `Sisyphus Cognitive Engine`

完整模型列表：<https://openrouter.ai/models>。使用 OpenAI 兼容协议但上游是不同的 provider，因此需要把 `kind` 显式声明为 `openrouter` 而非 `openai`，否则归因 header 不会被注入。

## HTTP 客户端调优（共享连接池）

所有 LLM provider 共享同一 [`SharedHttpClient`](https://github.com/zerove/opencodepro/blob/master/crates/sisyphus-tools-llm/src/http_client.rs)（生产级 reqwest 包装），由 `build_provider_registry` 在启动时构造一次。

### 环境变量

所有配置都可通过 `SISYPHUS_HTTP_*` 环境变量覆盖（无需重新编译）：

| 环境变量 | 默认 | 说明 |
|----------|------|------|
| `SISYPHUS_HTTP_REQUEST_TIMEOUT_SECS` | 300 | 总请求超时（含连接 + 等待响应） |
| `SISYPHUS_HTTP_CONNECT_TIMEOUT_SECS` | 10 | 单次 connect 阶段超时（防 DNS 黑洞） |
| `SISYPHUS_HTTP_POOL_IDLE_SECS` | 90 | 空闲连接在池中保持存活时间 |
| `SISYPHUS_HTTP_TCP_KEEPALIVE_SECS` | 60 | TCP keepalive 间隔（0 = 禁用） |
| `SISYPHUS_HTTP_POOL_MAX_PER_HOST` | 16 | 每 host 最大并发连接数 |
| `SISYPHUS_HTTP_REDIRECT_LIMIT` | 3 | HTTP 重定向最大次数 |
| `SISYPHUS_HTTP_TLS_ROOTS` | `webpki` | TLS 根证书源: `webpki` (Mozilla 内置, 默认) / `native` (系统 CA, 用于私有 CA) |
| `SISYPHUS_HTTP_RETRY` | `disabled` | 传输层重试: `disabled` (无) / `one_shot` (GET/HEAD 自动重试 1 次) |

### 性能收益

`evals/http_client_bench.rs` 实测（本地 echo server, 50 请求, 并发 5）：

```
共享 client 总耗时:   2776 ms (55520 µs/req)
每请求新建总耗时:     14193 ms (283860 µs/req)
加速比:               5.11x
```

详见 ADR-008（[`docs/adr/008-shared-http-client.md`](https://github.com/zerove/opencodepro/blob/master/docs/adr/008-shared-http-client.md)）和 [ADR-007：最终产品开发标准](https://github.com/zerove/opencodepro/blob/master/docs/adr/007-production-grade-standards.md)。

### 启动期错误处理

构造期错误（如 TLS、allocator 失败）以 `Err` 返回而非 panic。`build_provider_registry` 同样返回 `Result<ProviderRegistry, String>`，由调用方（bridge / bin）决定如何处理（log + 继续 / 退出）。

### 优雅关闭

```rust
let registry = build_provider_registry(&configs)?;
if let Some(shared) = registry.shared_http() {
    // 等待 in-flight 请求完成（最多 5 秒）
    let _ = shared.shutdown(Duration::from_secs(5)).await;
}
```

关闭后新请求会被快速失败（`on_request_start` 返回 false），不再向已关闭系统发起新请求。

## 认知循环配置

`[cognitive_loop]` 段控制 sisyphus-core 认知循环阶段的行为。bridge 启动时通过
`StrategicManager::apply_cognitive_loop_config` 应用（评审 A4 已接线）。

| 字段 | 环境变量 | 类型 | 默认 |
|------|----------|------|------|
| `max_iterations` | `SISYPHUS_COGNITIVE_LOOP_MAX_ITERATIONS` | integer | 8 |
| `convergence_threshold` | `SISYPHUS_COGNITIVE_LOOP_CONVERGENCE_THRESHOLD` | float | 0.15 |
| `llm_stage_timeout_secs` | `SISYPHUS_COGNITIVE_LOOP_LLM_STAGE_TIMEOUT_SECS` | integer | 30 |

`llm_stage_timeout_secs=0` 禁用反思/演进阶段的 LLM 调用（用于降级/纯启发式模式）。

## 集群与联邦经验聚合配置

`[cluster]` 段控制 peer 协议（ADR-009）与联邦经验聚合（ADR-016）。默认全关，
单机启动行为不变。

| 字段 | 环境变量 | 类型 | 默认 |
|------|----------|------|------|
| `enabled` | `SISYPHUS_CLUSTER_ENABLED` | bool | false |
| `node_id` | `SISYPHUS_CLUSTER_NODE_ID` | string | `""`（由 `HOSTNAME` 派生） |
| `cluster_name` | `SISYPHUS_CLUSTER_NAME` | string | `_sisyphus-bridge` |
| `mdns_service_type` | — | string | `_sisyphus-bridge._tcp.local` |
| `peers` | — | array | `[]` |
| `join_timeout_secs` | `SISYPHUS_CLUSTER_JOIN_TIMEOUT_SECS` | integer | 10 |
| `peer_ttl_secs` | `SISYPHUS_CLUSTER_PEER_TTL_SECS` | integer | 30 |
| `strategy_sync_interval_secs` | `SISYPHUS_CLUSTER_STRATEGY_SYNC_INTERVAL_SECS` | integer | 60 |

`peer_ttl_secs` 必须 ≥ 5 且严格大于 `join_timeout_secs`；
`strategy_sync_interval_secs` 为 0 时表示「只消费入站策略快照、不主动发布」。

### 集群策略权重同步（ADR-017）

集群启用后，每个节点按周期把本地 `MetaLearner` 的策略统计打包成
`NodeStrategySnapshot`（`generation` = unix 毫秒），经 `StateProposal` 集群事件
广播；对端按 `generation` 取新合并进共享视图，聚合结果通过
`GET /api/strategy/weights` 查看。

| 字段 | 环境变量 | 类型 | 默认 |
|------|----------|------|------|
| `strategy_sync_interval_secs` | `SISYPHUS_CLUSTER_STRATEGY_SYNC_INTERVAL_SECS` | integer ≥ 0 | 60 |

发布周期建议 ≥ 1；0 关闭主动发布（保留仅聚合视图的消费者）。发布与收发走同一套
入站事件总线，避免回环。快照合并幂等、可交换（同节点按更大 `generation` 取新）。

### `[cluster.federated]` 字段

入站经验在 CRDT 合并**之前**按质量分过滤。质量分算法见
`sisyphus_types::experience::evaluate_experience_quality`：

```
quality = clamp(
    if outcome_success { success_weight } else { 0.0 }
        - deviation_weight * clamp(deviation_score, 0.0, 1.0),
    0.0, 1.0)
```

| 字段 | 环境变量 | 类型 | 默认 |
|------|----------|------|------|
| `enabled` | `SISYPHUS_CLUSTER_FEDERATED_ENABLED` | bool | **false** |
| `min_quality_score` | `SISYPHUS_CLUSTER_FEDERATED_MIN_QUALITY_SCORE` | float `[0,1]` | 0.2 |
| `success_weight` | `SISYPHUS_CLUSTER_FEDERATED_SUCCESS_WEIGHT` | float ≥ 0 | 0.7 |
| `deviation_weight` | `SISYPHUS_CLUSTER_FEDERATED_DEVIATION_WEIGHT` | float ≥ 0 | 0.3 |

**默认关闭**：过滤会改变收敛结果集，关闭时入站经验全量合并，与 v0.3.0 行为
完全一致。仅在 `enabled = true` 时校验这些参数（未启用时它们不参与任何判定）。
被拒绝的经验会记 `warn` 日志并携带 `quality` / `min_quality_score`，不会静默丢弃。
参数在启动时装配，热重载不改变已启动的同步处理器。

### `[cluster.control_plane]` 字段（ADR-019，条件触发）

强一致控制面门禁。**默认关闭**；未满足 ADR-019 准入条件（语义/分工、proposer 装配、
故障模型、可观测、验证矩阵、回滚、专用运行器证据）时不得启用生产提交路径。
`enabled=true` 不代表控制面生产可用；v1 语义为多数派 ack + `request_id` 幂等 +
fencing/TTL，**超时/不确定=错误，不判定成功**（非 Raft 线性一致）。

| 字段 | 环境变量 | 类型 | 默认 |
|------|----------|------|------|
| `enabled` | `SISYPHUS_CLUSTER_CONTROL_PLANE_ENABLED` | bool | **false** |
| `propose_timeout_ms` | `SISYPHUS_CLUSTER_CONTROL_PLANE_PROPOSE_TIMEOUT_MS` | u64（毫秒） | 2000 |
| `connect_timeout_ms` | `SISYPHUS_CLUSTER_CONTROL_PLANE_CONNECT_TIMEOUT_MS` | u64（毫秒） | 1000 |
| `lease_ttl_min_secs` | `SISYPHUS_CLUSTER_CONTROL_PLANE_LEASE_TTL_MIN_SECS` | u64（秒） | 1 |
| `lease_ttl_max_secs` | `SISYPHUS_CLUSTER_CONTROL_PLANE_LEASE_TTL_MAX_SECS` | u64（秒） | 3600 |
| `tls_enabled` | `SISYPHUS_CLUSTER_CONTROL_PLANE_TLS_ENABLED` | bool | **false**（tonic TLS 已接线；启用时加载 PEM，失败/不完整 **fail-closed 拒绝装配**；wired 时影响共享 gRPC 端口） |
| `tls_ca_cert_path` / `tls_client_cert_path` / `tls_client_key_path` / `tls_domain_name` | 对应 `SISYPHUS_CLUSTER_CONTROL_PLANE_TLS_*` | string | 空（PEM 路径/域名；节点身份用于 server+client mTLS） |
| `submit_auth_mode` | `SISYPHUS_CLUSTER_CONTROL_PLANE_SUBMIT_AUTH_MODE` | string（`none`/`mtls_subject_allowlist`） | `none`；`mtls_subject_allowlist` 在当前构建 **fail-closed 拒绝装配**（X509 subject 提取/匹配未实现） |
| `allowed_submit_subjects` | — | string[] | `[]`（`mtls_subject_allowlist` 实现后使用） |

### `[cluster.strategy_selection]` 字段（v0.4.0 Phase E，ADR-017 Amendment 3）

执行路径消费集群策略权重的开关。`enabled = false`（默认）时策略提示只依据本地
`MetaLearner`，与 v0.3.0 行为完全一致。

| 字段 | 环境变量 | 类型 | 默认 |
|------|----------|------|------|
| `enabled` | `SISYPHUS_CLUSTER_STRATEGY_SELECTION_ENABLED` | bool | **false** |
| `min_uses` | `SISYPHUS_CLUSTER_STRATEGY_SELECTION_MIN_USES` | integer ≥ 1 | 5 |

启用后（`enabled = true`）的决策优先级（实现见 `sisyphus_types::strategy_weights` 的
`select_strategy`，纯函数 + 单测锁定）：

1. **本地证据充分**（`local.uses >= min_uses`）→ 用本地；
   本地样本最能反映本节点模型版本 / 工具链 / 负载，集群证据反而会稀释它。
2. **集群证据充分**（`cluster.uses >= min_uses`）→ 用集群；
   这是联邦的收益：新节点本地没有样本时不必自己踩一遍坑。
3. **两侧都不足** → 回退本地薄证据（有就用）；两边都没有 → 不注入提示。

边界是**含**的（`>=`）：恰好 `min_uses` 个样本就视为充分；`min_uses = 0` 被启动校验
拒绝（运行时会被 `effective_min_uses()` 静默改成 1，故零值必须显式挡在外）。
