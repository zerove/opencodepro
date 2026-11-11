# T4.4「统一 trace 关联」设计文档

> 关联主计划：`docs/plans/automation-development-plan-2026-11.md`（阶段 4 / T4.4）。
> 验收条目：入口生成 trace id，贯穿 SOP/工具/LLM 日志与响应头；一次请求可查到全部子步骤；设计评审记录。
> 本文是 T4.4 的**先行设计**（主计划要求），实施步骤见 §5，尚未全部落地。

## 1. 目标

把「一次请求」与「它触发的全部子步骤」用同一个标识串起来：

- 入口（HTTP / gRPC）生成或透传 trace id；
- 该 id 出现在响应头、SSE 事件负载、结构化日志（tracing span 字段）里；
- 客户端（含 CLI/TUI）能凭同一个 id 把「入口日志 → 工具调用 → LLM 调用 → 事件流」串成一条链。

## 2. 现状盘点（已核实）

| 位置 | 现状 | 文件 |
|---|---|---|
| REST 入口 | axum `Router`，鉴权用 `route_layer(from_fn_with_state(jwt_auth_middleware))`；**无请求标识** | `crates/sisyphus-bridge/src/http/mod.rs:295-310` |
| gRPC 入口 | `SisyphusBridgeServer::with_interceptor(auth_interceptor)`；**无请求标识** | `crates/sisyphus-bridge/src/main.rs:361-374` |
| CLI → bridge | 所有 gRPC 调用统一经 `BridgeClient::with_auth` 注入 metadata —— **天然的注入点** | `crates/sisyphus-cli/src/bridge.rs:93` |
| SSE 事件 | `SseEvent::from_execution_event` 已把 `sop_id` 作为**维度键**注入 payload —— `trace_id` 可同构注入 | `crates/sisyphus-bridge/src/http/sse_handlers.rs:231-260` |
| 事件契约 | `ExecutionEvent` 变体不得改动（T1.3 兼容约束）→ trace id **不进枚举**，走 SSE 负载 | `crates/sisyphus-types/src/execution.rs` |
| 既有 request_id | 仅存在于 cluster 共识/控制面（租约幂等），与请求链路无关 | `crates/sisyphus-bridge/src/peer/control_plane.rs` |

## 3. 方案对比

| 方案 | 做法 | 结论 |
|---|---|---|
| A 仅日志 span | 只在 handler 内开 tracing span，不对外暴露 id | **不满足**验收：客户端无法串联，也无法按 id 查询 |
| B 入口生成 + 显式携带 | HTTP 头 / gRPC metadata / SSE 负载 / 响应头 显式携带 trace id | **采纳**：改动集中（4 处），向后兼容（均为增量字段） |
| C 全局上下文 | task-local / thread-local 隐式传递 | **不采纳**：跨 `await`、跨线程易丢，跨进程不可用；仅作为日志侧辅助手段（span 继承） |

## 4. 字段与传播路径

| 位置 | 承载方式 | 命名 |
|---|---|---|
| HTTP 请求头 | 客户端可自带，缺失则生成 | `x-trace-id` |
| HTTP 响应头 | 入口回写 | `x-trace-id` |
| gRPC metadata | CLI 侧 `with_auth` 注入 | `x-trace-id` |
| SSE 事件负载 | 维度键（与 `sop_id` 同构） | `trace_id` |
| 结构化日志 | tracing span 字段 | `trace_id` |

**格式与校验**：`trace-` + 32 位十六进制（uuid v4 simple）。入口只接受 `[A-Za-z0-9_-]{1,64}`，
非法或超长则**重新生成**（避免日志伪造与注入）。

**生成点**：仅在入口（HTTP middleware / gRPC interceptor）生成一次；子步骤继承，不再生成。

## 5. 实施步骤（待执行）

1. **trace id 工具函数**：新增 `generate_trace_id()` / `sanitize_trace_id(raw) -> Option<String>`
   （建议放 `crates/sisyphus-types`，bridge 与 CLI 共用）。
2. **bridge HTTP**：新增 `trace_id_middleware`——读取/生成 trace id → 写入 request extensions
   → `tracing::Span` 携带 `trace_id` → handler 返回后写入响应头 `x-trace-id`；
   以 `layer` 形式挂在 router 上（覆盖 public + protected 全部路由）。
3. **bridge gRPC**：新增 `TraceInterceptor`（tonic interceptor），与 `AuthInterceptor` 串联
   （Auth → Trace），从 metadata 取 `x-trace-id`，缺失则生成并回写 metadata。
4. **bridge SSE**：`from_execution_event` 增加 `trace_id` 维度键注入（调用方从请求上下文传入）。
5. **CLI**：`BridgeClient::with_auth` 为每次调用注入 `x-trace-id` metadata，并在 CLI/TUI 日志中
   打印同一 id（便于用户从界面反查 bridge 日志）。
6. **工具与 LLM 调用**：在既有 `tracing::info!` 中补 `trace_id = %trace_id` 字段（不改函数签名，
   从 span 字段读取）。
7. **文档与契约**：`book/src/sse-events.md` 增补 `trace_id` 说明；OpenAPI 增补响应头声明。

### 5.1 实施进展

| 步骤 | 状态 | 说明 |
|---|---|---|
| 1 trace id 工具 | DONE | `crates/sisyphus-types/src/trace.rs`：`generate_trace_id` / `sanitize_trace_id` / `resolve_trace_id` / `TraceId`（6 个单测） |
| 2 HTTP 入口 | DONE | `crates/sisyphus-bridge/src/http/mod.rs`：`trace_id_middleware`（extensions + tracing span + 响应头），3 个单测 |
| 3 gRPC 入口 | DONE | `crates/sisyphus-bridge/src/trace.rs`：`TraceInterceptor` + `AuthTraceInterceptor`（组合），3 个单测 |
| 4 SSE 负载 | DONE | `SseEvent::from_execution_event_with_trace` 注入 `trace_id` 维度键；`execute_goal_handler` / `execute_composite_handler` 从 request extensions 取 trace id，1 个单测 |
| 5 CLI 注入 | DONE | `BridgeClient::with_auth` 为每次 gRPC 调用注入 `x-trace-id` metadata |
| 6 工具/LLM 日志 | DONE | HTTP 入口 span（`http_request`）带 `trace_id`，子 span 继承；gRPC 侧 `grpc.execute_sop` / `grpc.execute_tool` 的 span 增加 `trace_id` 字段（从 metadata 读取并 record） |
| 7 文档与契约 | DONE | `book/src/sse-events.md` 新增「请求关联：`trace_id`」小节；OpenAPI `info.description` 说明所有响应带 `x-trace-id` |

**实跑证据**（bridge debug 构建，`cluster.enabled=false`）：

| 请求 | 响应头 `x-trace-id` |
|---|---|
| `-H 'x-trace-id: trace-live-check'` | `trace-live-check`（原样透传） |
| 不带该头 | `trace-77f68aeed7c24087b30d2ae59991b411`（生成） |
| `-H 'x-trace-id: bad id!'` | `trace-acce6fe781c44634938e2d7a45fe6957`（非法值被替换） |


## 6. 兼容性与风险

- **兼容性**：不修改任何 trait 签名；不修改 `ExecutionEvent` 变体；SSE 负载新增键为增量，
  旧客户端忽略未知键即可。
- **风险 1**：日志量增加（每请求一个 span）。缓解：只在入口建 span，子步骤继承；
  调试级别下可关闭。
- **风险 2**：客户端可伪造 trace id 串联他人日志。单机场景风险可接受；已用字符集与长度校验收敛。
- **风险 3**：SSE 事件与入口请求不同源（异步执行）。缓解：执行侧在事件负载注入发起时的 trace id，
  执行记录本身已带 `sop_id`/`project_id` 维度，可交叉定位。

## 7. 验收口径

- 单测：middleware 生成/透传/非法字符拒绝；interceptor metadata 注入与回写；
  SSE 负载含 `trace_id`；CLI 侧 metadata 注入。
- 实跑（一次请求可查到全部子步骤）：
  1. `curl -H 'x-trace-id: trace-abc' http://127.0.0.1:<port>/api/...`；
  2. 响应头回写同一 id；
  3. bridge 日志中该 id 出现在入口行与其触发的工具/LLM 行；
  4. SSE 流对应事件负载含同一 id。
- 证据目录：`docs/benchmarks/acceptance/<date>-t4.4/`（含请求、响应头、日志片段、事件片段）。

## 8. 回退

全部改动为增量（新增头 / 新增负载键 / 新增日志字段），可按配置开关 `trace.enabled`
单独关闭；关闭后行为退化为当前实现，不影响既有契约。
