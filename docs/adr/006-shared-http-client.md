# ADR-006: 生产级共享 HTTP 客户端（最终产品标准）

**Status**: Accepted (2026-08-28)
**Deciders**: core team
**Supersedes**: N/A

## Context

Sisyphus 在多 LLM provider 场景下，每个 provider 各自构造 `reqwest::Client`，导致：

1. **每个 provider 独立连接池** — N 个 provider = N 个 TCP/TLS 池，浪费 fd、握手延迟、内存。
2. **API 不一致** — `new(config)` 失败时直接 panic（`expect("...")`），破坏生产可用性。
3. **缺乏可观测性** — 不知道 in-flight 请求数、错误率、关闭状态。
4. **缺乏优雅关闭** — bridge 重启时 in-flight 请求被截断，丢消息。
5. **错误分类粗糙** — 所有 4xx 与 5xx 一律当 `Transient`，导致 401/403 等不可恢复错误被无谓重试。
6. **配置散落** — 三个 provider 各自 `Duration::from_mins(2/3)`，没有"集中可调优"入口。

参考 jcode 的 [`jcode-provider-core`](https://github.com/...) 设计：核心抽象、共享 client、生产级配置、可观测性，集中在低层 crate。

## Decision

### 1. 引入 `SharedHttpClient`（生产级 HTTP 客户端工厂）

位置：`crates/sisyphus-tools-llm/src/http_client.rs`

设计要点：

- **共享连接池**：所有 provider 通过 `Arc<SharedHttpClient>` 共享同一 `reqwest::Client`。
- **显式配置**：`SharedHttpClientConfig` 暴露
  - `request_timeout`（默认 5 分钟）
  - `connect_timeout`（默认 10 秒，防 DNS 黑洞）
  - `pool_idle_timeout`（默认 90 秒）
  - `tcp_keepalive`（默认 60 秒，防 NAT 静默断开）
  - `pool_max_per_host`（默认 16，防单 host 过度并发）
  - `redirect_limit`（默认 3，防重定向循环）
  - `user_agent`（默认 `Sisyphus/<version>`）
- **环境变量覆盖**：`SISYPHUS_HTTP_*` 系列，`from_env()` 一行应用。
- **构造期错误返回 `Result`**：绝不 `panic!`。
- **可观测**：`stats()` 暴露 `requests_total / errors_total / in_flight / shutdowns_total`。
- **优雅关闭**：`shutdown(wait)` 等 in-flight 归零，超时则 `Err` 返回剩余数；幂等。

### 2. RAII `RequestGuard`

- 入口：`on_request_start()` → 失败时快速返回 `Err(closed)`。
- 成功路径：构造 `RequestGuard`，Drop 时自动 `on_request_end(errored)`。
- 错误路径：调用 `mark_errored()` 后 Drop，统计准确。

避免"忘记收尾"导致的 in-flight 计数泄漏。

### 3. 错误分类细化

| HTTP 状态 | 错误类型 | 是否计入 errors_total |
|-----------|---------|---------------------|
| 2xx       | OK      | 否                  |
| 401/403   | `Auth`  | 是                  |
| 429       | `RateLimited` | 是            |
| 5xx       | `Transient`（可重试） | 是  |
| 其它 4xx  | `Permanent`（不可重试） | 是 |

之前所有 4xx 都是 `Transient`，导致不可恢复错误浪费重试预算。

### 4. LlmProvider 加 `Any` supertrait

```rust
pub trait LlmProvider: Any + Send + Sync { ... }
```

- 允许外部 `Arc<dyn LlmProvider>` 通过 `&dyn Any` downcast 到具体实现。
- jcode 标准模式，便于测试断言和未来插件系统。

### 5. 破坏性 API 变更

**`Provider::new(config)` → `Provider::new(config, Arc<SharedHttpClient>)`**

- 旧 API 完全移除（不留 `new_with_client` 双 API）。
- 强制所有调用方显式传入 client，杜绝"忘记共享"的隐患。
- 所有现有测试 helper 同步更新（`test_http()` 工厂）。

### 6. `build_provider_registry` 返回 `Result`

- 旧：`ProviderRegistry`（构造失败只 log，不阻断）
- 新：`Result<ProviderRegistry, String>`（构造失败显式 Err）
- 启动期由上层（bridge / bin）决定如何处理（log + 继续 / 退出）

## Consequences

### 正面

- ✅ **连接池复用**：3 个 provider 共享 1 个 pool，省 ~2× fd、减少握手延迟。
- ✅ **零 panic 路径**：启动期错误一律 `Err` 返回。
- ✅ **可观测**：`stats()` 可直接对接 prometheus / bridge /api。
- ✅ **优雅关闭**：bridge SIGTERM 时可 `shutdown(5s)` 等 in-flight。
- ✅ **错误分类正确**：401/403 不再被重试，省 token。
- ✅ **集中可调优**：env 变量一行改超时，零重新编译。
- ✅ **Any downcast**：未来可加具体实现特定的 hooks（cache、metrics、debug）。
- ✅ **回归保护**：2 个新增测试（共享 client 跨 provider、Any downcast）。

### 负面 / 成本

- ❌ **API 破坏性**：3 个 provider 的 `new()` 签名变更，所有调用方需更新。
- ❌ **测试 helper 成本**：现有 5+ 处测试需 `test_http()` 注入。
- ❌ **Cargo.toml 变更**：`sisyphus-tools-llm` 需 `tokio` feature 加 `rt-multi-thread`（用于 `Runtime::new()`）。
- ❌ **构建结果**：`build_provider_registry` 现返回 `Result`，调用方需 `.unwrap()` 或 `.ok()`。

### 风险

- ⚠️ **in-flight 计数精度**：`on_request_end` 依赖 Drop；若调用方在 Drop 前 panic（`std::panic` unwind 时仍会执行 Drop），计数仍能归零。但若 `std::process::exit` 或 abort 则泄漏。**缓解**：崩溃路径本身已不可信，不做额外处理。
- ⚠️ **共享 client 关闭顺序**：若在 `shutdown(wait)` 超时后仍有 in-flight 强引用持有 client，Drop 时会触发 panic（hyper 内部）。**缓解**：文档化"shutdown 应在所有 provider 引用 drop 前调用"。

## Validation

### 编译

```bash
cargo check -p sisyphus-tools-llm    # ✓
cargo check-all                       # ✓
```

### 测试

```bash
cargo test -p sisyphus-tools-llm --lib
# test result: ok. 96 passed; 0 failed
```

### 性能基准（实测）

`evals/http_client_bench.rs` 测量两种策略的吞吐量差异（dev 模式 + 本地 axum echo）：

| 请求数 | 并发 | 共享 client (ms) | 每请求新建 (ms) | **加速比** |
|--------|------|------------------|-----------------|----------|
| 50     | 5    | 2,776            | 14,193          | **5.11x** |
| 200    | 20   | 2,146            | 69,540          | **32.40x** |
| 500    | 50   | 4,199            | 254,066         | **60.51x** |

**结论**：规模越大，共享 client 优势越显著（连接复用从 N→1 + 省去 N 次 TCP/TLS 握手）。
这是"按最终产品性能标准"实施后第一个**可量化**的胜利。

### 单元测试覆盖

新增 13 测试：
- `http_client::*` (10 个)：默认配置、env 覆盖、构造、统计、关闭幂等、关闭后拒绝、共享 state
- `registry_shares_one_http_client_across_providers`：跨 provider 共享 + shutdown 真的生效
- `provider_downcast_to_openai_works`：Any supertrait downcast

## References

- jcode MODULAR_ARCHITECTURE_RFC §`jcode-provider-core` / `jcode-provider-openrouter`
- reqwest: <https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html>
- tokio Notify: <https://docs.rs/tokio/latest/tokio/sync/struct.Notify.html>

## Future Work

- [ ] **Step E: metrics 导出**：在 `sisyphus-bridge` 加 `/metrics` 端点暴露 `SharedHttpClient::stats()`。
- [ ] **Crate 拆分**：考虑将 `http_client` 抽到独立 `sisyphus-tools-llm-http` crate（高 ROI 候选）。
- [ ] **TLS 调优**：当前用 reqwest 默认（rustls + native verifier），可显式 `with_webpki_roots()` 强化。
- [ ] **retry 中间件**：当前 5xx 由 ProviderRegistry 重试；可在 SharedHttpClient 层加 transport-level retry（仅 GET）。
