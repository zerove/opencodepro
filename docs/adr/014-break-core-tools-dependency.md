# ADR-014: 解除 sisyphus-core 对 sisyphus-tools-* 的反向依赖

**Status**: Accepted（四个阶段全部完成）
**Deciders**: core team
**Related**: ADR-003 (modular restructure), ADR-007 §工程约定

## Context

`sisyphus-core` 的 `Cargo.toml` 直接依赖 6 个 `sisyphus-tools-*` 实现 crate：

| crate | core 中非测试引用点 |
|---|---|
| `sisyphus-tools-llm` | 55（`LlmClient` / `EmbeddingClient` / `BudgetProvider` 三个 trait + 具名类型） |
| `sisyphus-tools-llm-tools` | 12（`MetaLearner`） |
| `sisyphus-tools-executor` | 5（`TestExecutor`） |
| `sisyphus-tools-build` | 6（`ContinuousBenchmark`） |
| `sisyphus-tools-system` | 2（`AgentChannel`） |
| `sisyphus-tools-agent` | 0（仅 benches 使用） |

核心引擎因此在编译期绑定了工具层实现：工具层任何签名调整都要动 core，
而 core 的单元测试基座（`setup_manager`）直接实例化
`ContinuousBenchmark` / `MetaLearner`，无法脱离工具层单独运行。

## 依赖的实际形态

审计发现的关键约束——**这不是一次机械改路径就能完成的搬迁**，因为存在
两处真实的层次倒置：

1. **`ToolError` / `ToolResult` 定义在 `tools-core`，却被 `sisyphus-types`
   邻接的代码使用。** `AgentChannel`、`ContinuousBenchmark`、`MetaLearner`
   三者都依赖 `sisyphus_tools_core::{ToolError, ToolResult}`。由于
   `tools-core` 依赖 `sisyphus-types`，`sisyphus-types` 不能反过来依赖它们。
   **必须先把错误类型上移。**

2. **`LlmClient::shared_http()` 的返回类型是 `Arc<SharedHttpClient>`，
   而后者内部包装 `reqwest::Client`。** 若把该 trait 移入
   `sisyphus-types`，等于把 `reqwest` 引入基础 crate，而后者被全部 27 个
   成员依赖。

   桥接侧确实以 `dyn LlmClient` 多态调用该方法
   （`bridge_service.rs:788` 的 `self.llm_client.shared_http()`，
   喂给 `RestApiState::with_shared_http` 供 `/api/metrics` 读取）。
   正确做法是让 trait 返回 `sisyphus_types` 中的**纯数据快照**
   （如 `HttpStatsSnapshot { requests, errors, … }`）而非客户端句柄——
   这本身也是更好的抽象：trait 不该泄漏 HTTP 客户端类型。

## Decision

分三阶段推进，**每阶段独立可发布、保持向后兼容**。阶段之间用
`pub use` 再导出，因此 104 处既有调用点（35 个文件）无需改动。

### 阶段 1：错误类型上移

`ToolError` / `ToolResult` 移入 `sisyphus-types`，`tools-core` 改为
`pub use sisyphus_types::{ToolError, ToolResult};`。零调用点变更。

### 阶段 2：叶子实体上移

`AgentChannel`、`ContinuousBenchmark`、`MetaLearner` 移入 `sisyphus-types`
（阶段 1 后它们已无 `tools-core` 依赖）。`tools-*` 侧同样 `pub use` 再导出。

完成后 `sisyphus-core` 移除 `tools-system` / `tools-build` /
`tools-llm-tools` / `tools-agent` 四个依赖。

### 阶段 3：LLM trait 上移 + 解耦 shared_http

`LlmClient` / `EmbeddingClient` / `BudgetProvider` 移入 `sisyphus-types`；
同时把 `shared_http()` 改为返回 `HttpStatsSnapshot`（阶段 1 中
`CircuitBreaker` 一并上移——它只依赖 `sisyphus_types::CircuitState`）。

完成后 `sisyphus-core` 移除 `tools-llm`，**core 与 tools 之间的依赖归零**。

## Consequences

**正面**
- core 可脱离工具层独立编译与测试
- 工具层重构不再牵动核心引擎
- `LlmClient` 不再泄漏 `reqwest` 类型

**负面 / 代价**
- 三个阶段的 diff 都不小；阶段 1 触及全部 13 个使用 `ToolError` 的 crate
- `SharedHttpClient` 的优雅关闭语义需要重新安置（trait 的
  `shutdown()` 目前已承担部分职责，需确认覆盖完整）
- 上移 `ContinuousBenchmark` 意味着它进入基础 crate 的公开 API

**风险控制**
- 全程 `pub use` 再导出，任何阶段都可单独回滚
- 每阶段结束跑完整 `cargo test-all` + 四个被排除的重型 crate
- 阶段 3 涉及 `/api/metrics` 输出，需同步更新 OpenAPI 与指标测试

## Alternatives Considered

**一次性搬迁**：把三个 trait + 三个实体一次性移入 `sisyphus-types`。
拒绝——单次 diff 过大，出问题难以二分定位，且无法在中途交付可用状态。

**保持现状，仅在文档记录**：拒绝——`core` 与 `tools` 的编译期耦合是持续的
维护成本，不是可以搁置的技术债。

**让 core 反向依赖 tools 不变，改由 tools 依赖 core**：拒绝——这会把认知引擎
的实现细节（`SisyphusKernel`）泄漏给工具层，方向错误。

## Migration Status

- [x] **阶段 1：错误类型上移** — `ToolError` / `ToolResult` 已移入
  `sisyphus-types::tool_error`，`sisyphus-tools-core` 改为 `pub use` 再导出。
  全 workspace 零调用点变更通过编译。
- [x] **阶段 2：叶子实体上移** — `AgentChannel`、`ContinuousBenchmark`
  （含 `Timer` / `BaselineComparison`）、`MetaLearner`（含 `types` 子模块）
  已移入 `sisyphus-types`，原 crate 改为 glob 再导出。
  `sisyphus-core` 的生产依赖因此移除 `tools-system` 与 `tools-build`；
  `tools-agent` 本就无生产引用，一并移除。
  `tools-system` 仅保留为 core 的 `dev-dependencies`（集成测试仍构造真实
  `SystemExecutor`）。
- [x] **阶段 3：LLM trait 上移 + shared_http 解耦** — `LlmClient` /
  `EmbeddingClient` / `BudgetProvider`、`CircuitBreaker` + `BreakerConfig`、
  `HttpClientStats` 全部移入 `sisyphus-types`（新增 `llm_traits.rs`、
  `circuit_breaker.rs`、`http_stats.rs`），`tools-llm` 改为再导出。
  关键解耦：trait 的 `shared_http() -> Option<Arc<SharedHttpClient>>`
  改为 `http_stats() -> Option<HttpClientStats>`（纯计数快照）。
  bridge 侧 `RestApiState.shared_http` 改为
  `http_stats: Option<Arc<dyn Fn() -> HttpClientStats + Send + Sync>>`——
  用读取闭包而非冻结快照，保持 `/api/metrics` 每次请求读实时计数。

### core 的 tools-* 生产依赖：6 → 0

| crate | 状态 |
|---|---|
| `tools-system` | 已移除（仅保留为 dev-dependency，集成测试构造真实 `SystemExecutor`） |
| `tools-build` | 已移除 |
| `tools-agent` | 已移除（本就零生产引用） |
| `tools-llm` | 已移除（仅 dev-dependency，供 `tests.rs` 的 `DummyLlmClient`） |
| `tools-llm-tools` | 已移除（阶段 4；仅 dev-dependency 供测试构造真实 reviewer） |
| `tools-executor` | 已移除（阶段 4；仅 dev-dependency 供测试构造真实 executor） |

- [x] **阶段 4：最后两个实现依赖走注入** — `sisyphus-core` 的生产依赖图
  中已 **零** `sisyphus-tools-*`。

  - **测试执行器**：`Kernel::new` / `KernelBuilder` 新增
    `test_executor: Arc<dyn sisyphus_types::TestExecutor>`，
    `StrategicManager` 新增同名字段（原先 `run_cognitive_loop` 在运行时
    现场 `new()` 一个具体实现）。此处**不可**用空实现顶替：该执行器被
    `self_programming_manager` 真正调用（`run_test` 会 shell 出去跑测试），
    换成 no-op 会让自编程特性静默地认为测试通过。
  - **MCP 接线**：新增 `sisyphus_types::McpExecutorFactory`
    （`connect_stdio` / `connect_url`），实现在 `tools-executor` 侧；
    `init_mcp_executors` 改为接收该 trait 而非构造具体
    `MCPToolExecutor`。
  - **专家评审**：采纳「单一 `ExpertReviewer` 接缝」方案。新增
    `sisyphus_types::ExpertReviewer`，把 `intelligence_manager` 里
    oracle/momus 的启发式判断与格式化整块移入
    `tools-llm-tools::expert_reviewer`。core 只负责收集输入（改动文件 +
    各节点输出）并接收 `Vec<String>` 建议，**不再知道 momus/oracle 存在**，
    也不需要把 `FileAnalysis` / `MomusReview` 等结果类型搬进 `types`。
    格式化字符串逐字保留——它们是最终落到
    `ReflectionReport::suggested_improvements` 的用户可见文本。
  - `src/bin/integration_stress.rs` 改用本地 `UnusedTestExecutor`：该压测
    图只含 `WasmSOPNode`，从不调用 `run_test`，且本地桩**返回错误而非伪造
    成功**，以免将来真的跑测试时静默虚报通过。

  > 该 bin 在此之前即已无法编译（`sisyphus_tools_llm` 不可用、
  > `WasmSOPNode` 缺 `args` 字段），且位于 `sandbox` feature 之后，
  > 默认构建不覆盖。属于既有问题，本次未修。

`/api/metrics` 的输出格式与语义在解耦前后逐字节一致（已真机比对）。