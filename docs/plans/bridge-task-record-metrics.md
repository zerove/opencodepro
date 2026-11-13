# Bridge 路径 TaskRecord 落盘与 `metrics quality` 覆盖（最小设计）

> 状态：**已覆盖主要执行入口**（2026-11-13）。HTTP `execute_goal` / composite / goal streaming，以及 gRPC `execute_sop` / `stream_execution`（goal 与非 goal 分支）/ `call_jcode` 均已接线 best-effort 落盘；单测与聚合兼容性验证完成。live E2E 已覆盖 `execute_goal` 失败路径、`composite` 成功 + `goal streaming` 失败、gRPC `execute_sop` 失败、gRPC `stream_execution`（goal 分支与非 goal/SOP 分支）失败、gRPC `call_jcode` 失败（含非 null token 样本，见 `docs/benchmarks/acceptance/2026-11-13/bridge-task-record-e2e.md`、`docs/benchmarks/acceptance/2026-11-13/bridge-task-record-coverage.md` 与 `runs/bridge-record-coverage/grpc-stream-calljcode/`、`grpc-stream-sop/`）。**待后续**：`execute_goal` 成功路径 live、执行级确定性 token usage 来源（当前为累计差值 best-effort）。
>
> 背景：`sisyphus dev` / `sisyphus verify` 已可通过 `--record` 产出 `TaskRecord`（`sisyphus-metrics::quality`）并由 `sisyphus metrics quality` 聚合；bridge 侧当前**无 Developer/dev 闭环执行路径**，因此 dev 语义的质量记录无法直接复用。bridge 可观测、可执行的是 **goal/SOP 执行**（HTTP/gRPC/SSE），需要定义与现有指标口径兼容且不伪造语义的最小落盘方案。

## 1. 目标与非目标

### 目标
- 为 bridge 的 goal/SOP 执行产出可聚合的任务级记录，使 `sisyphus metrics quality --records <dir>` 能覆盖 bridge 路径样本（与 CLI 样本分目录或同目录均可，但必须可区分）。
- 记录包含可归因元数据：引擎版本（`engine_commit`/`engine_dirty`）、时间、耗时、场景、终态与可用的 token/cost 信息。
- 实现最小、可测试，不改变现有 CLI `dev/verify` 记录语义。

### 非目标（本设计阶段不做）
- 不在 bridge 内引入 Developer/dev 闭环或 LLM 规划-修复循环。
- 不伪造 `initial_verify_passed` / `final_verify_passed` / `fix_attempts` / `regressed` 的 dev 客观闸门语义（bridge goal 执行默认无 `--verify-cmd` 式外部判据）。
- 不改 `TaskRecord` 的既有字段含义（避免破坏历史聚合口径）；如需 bridge 特有字段，优先通过 `scenario` 约定与可选扩展字段（向后兼容）表达。
- 不覆盖 cluster/control-plane 路径（遵循单机优先与现有边界）。

## 2. 现状与执行落点

| 路径 | 入口 | 结果/失败信号 | 备注 |
|---|---|---|---|
| HTTP goal 执行 | `crates/sisyphus-bridge/src/http/handlers.rs::execute_goal_handler` (`POST /api/execute/goal`) | `Ok((output, report))` → 200；`Err(e)` → 错误响应；handler 内已 `await` SSE forwarder 排空 | 有 `sop_id`/`project_id`/`goal`/`trace_id` 上下文，首个落点；**已接线** |
| HTTP composite 执行 | `execute_composite_handler` | 成功/失败 + 响应序列化失败 | 用请求级 `composite-<uuid>` 归属；**已接线** |
| HTTP goal streaming | `goal_stream_handler` (`/api/execute/goal/:goal/stream`) | spawn 内执行结束 | 场景 `bridge-goal-streaming`；**已接线** |
| gRPC SOP 执行 | `grpc/execution.rs::execute_sop_impl` | `run_execution` 成功/失败 | 场景 `bridge-sop-execute`；**已接线** |
| gRPC 流式执行 | `stream_execution_impl` | goal 分支 / SOP-工具分支 | `bridge-goal-streaming` / `bridge-sop-execute`；**已接线** |
| gRPC Jcode | `call_jcode_impl` | goal 执行成功/失败 | 场景 `bridge-jcode-execute`；**已接线** |
| 其他工具/会话执行 | 相关 handlers | 视具体语义 | 仅在可定义“任务级”完成判定时纳入 |

**首选最小落点**：HTTP `execute_goal_handler` 的完成分支（成功/失败均写一条记录），因为上下文完整、语义清晰、易做集成测试与外部验证。后续将写入逻辑下沉到共享执行编排层以覆盖 gRPC/composite/streaming，避免重复与漏记。

## 3. 记录契约与字段映射

复用 `sisyphus_metrics::quality::TaskRecord`（`crates/sisyphus-metrics/src/quality.rs`），保持 serde 兼容与聚合逻辑不变。

| TaskRecord 字段 | bridge goal 执行映射 | 说明 |
|---|---|---|
| `task_id` | 稳定唯一 ID（建议 `bridge-goal-<sop_id>-<start_ts_ms>` 或含 trace/request 标识） | 需可定位到单次执行；避免仅用 `sop_id`（可复用） |
| `scenario` | `bridge-goal-execute`（可按 composite/sop 细分，如 `bridge-composite-execute`） | 供按场景过滤聚合；与 CLI `dev-closed-loop`/`verify` 区分 |
| `status` | 成功 `Pass`；执行错误/不可完成 `Fail`；明确的前置条件/环境缺失且可判定为无法执行时 `Blocked`（保守，默认不滥用） | 以执行结果为准，不以“模型自评文本”判定 |
| `initial_verify_passed` | `status == Pass`（无独立首轮闸门） | **语义说明**：bridge goal 无 dev 式客观闸门循环；置为与最终一致，避免虚构“修复前失败”样本污染修复率分母 |
| `final_verify_passed` | `status == Pass` | 同上 |
| `fix_attempts` | `0` | 无自动修复循环 |
| `regressed` | `false` | 无可判定回归信号时不声称回归 |
| `duration_ms` | handler/执行起止墙钟耗时（`Instant` 计时） | 必填，反映端到端 HTTP 执行耗时 |
| `prompt_tokens` / `completion_tokens` | 从 `report`/执行统计中提取 provider 上报 usage；无则 `None` | `None` ≠ 0（遵循现有口径） |
| `engine_commit` | `env!("CARGO_PKG_VERSION")` 不足以归因源码；优先复用 CLI 侧已有的 git commit 探测逻辑（提取为共享 helper） | 脏工作区必须标记 `engine_dirty=true` |
| `engine_dirty` | 同上 | 与 `single-node-quality-metrics.md` 口径一致 |
| `recorded_at` | RFC3339（UTC） | 统一时间格式 |

### 口径约束（必须在实现与文档中显式说明）
- **修复率/回归率**：bridge goal 记录因 `initial_verify_passed == final_verify_passed` 且 `fix_attempts == 0`，不会计入修复率分母，也不会计入回归率分母（与当前聚合逻辑一致）。这是预期行为，不代表“修复能力为 0”。
- **成功率**：可与 CLI 样本聚合，但**建议按 `scenario`/来源分组解读**；混合聚合时必须保留版本归因与样本构成信息（现有 `engine_commits`/`engine_unknown` 机制已部分覆盖，必要时增加来源维度的后续增强不在本次最小范围）。
- **无闸门 ≠ 质量已验证**：goal 执行 `Pass` 表示执行路径完成/返回成功，不等同于外部客观验收（除非未来显式接入 verify gate）。

## 4. 落盘位置与配置

- **默认目录（建议）**：复用质量指标记录目录约定 `docs/benchmarks/quality-metrics/records/` 或由 bridge 配置项指定（如 `metrics.quality_records_dir` / `SISYPHUS_BRIDGE_QUALITY_RECORDS_DIR`）。最小实现可先支持环境变量/配置覆盖 + 安全默认（仅在显式启用或目录可写时写入，避免意外写入生产路径）。
- **文件命名**：`bridge-goal-<sanitized-sop-id>-<timestamp>.json`（单执行一文件），原子写入（写临时文件后 rename）以降低并发/崩溃损坏风险。
- **权限与失败处理**：落盘失败**不得**改变执行结果语义（记录写入 best-effort，失败仅 `warn` 日志并可选指标计数），避免观测副作用影响主路径。

## 5. 实现步骤（最小可交付）

1. **抽取引擎版本探测 helper**：从 CLI `dev/verify` 记录路径复用或提取到共享位置（避免三处重复且口径漂移）。
   - **已完成**：探测逻辑迁移到 `sisyphus-metrics::engine_revision`（`engine_revision()` / `engine_revision_at()` / `EngineRevision`，含 6 个单测：sha 规范化、非 git/不存在目录、dirty 标签、仓库内解析）；`sisyphus-cli::engine_revision` 保留为 re-export 稳定路径，bridge `quality_record` 直接复用同一实现，不再各自探测。
2. **Bridge 记录写入模块**：新增轻量 writer（依赖 `sisyphus-metrics::quality::TaskRecord`），负责序列化、目录创建、原子写文件、best-effort 错误处理。
   - **已完成（最小）**：`quality_record` 模块实现原子写入与 best-effort 告警；通过 `SISYPHUS_BRIDGE_QUALITY_RECORDS_DIR` 环境变量 opt-in 启用（未设置则不写）。
3. **接线 `execute_goal_handler`**：计时开始/结束；在成功与失败分支组装记录并调用 writer；补充 `tracing` 字段（`task_id`/`duration_ms`/`status`）便于排查。
   - **已完成**：HTTP `/api/execute/goal` 成功/失败（含响应序列化失败）分支写入 `scenario=bridge-goal-execute` 记录。
   - **token usage（best-effort，已接入）**：bridge 的 `DynamicLlmClient` 暴露的是**进程累计**计数器且被并发共享；各记录入口在执行前后尝试获取独占观测租约（`begin_llm_task` / `end_llm_task`），仅在租约独占且 provider 上报 usage 时写入饱和差值，否则保持 `null`（未知 ≠ 0）。这避免并发归因污染，但**不保证**在高并发/未走同一租约协议的调用下严格准确；待内核/客户端支持执行级用量后可替换为确定性来源。`token_usage_delta` 辅助函数与单测已覆盖（`None`→`None`、基线未知按 0、饱和差值）。
   - **已扩展**：同一 writer 现覆盖 HTTP composite（`bridge-composite-execute`）、HTTP/gRPC goal streaming（`bridge-goal-streaming`）、gRPC `execute_sop` 与流式 SOP/工具分支（`bridge-sop-execute`）、gRPC `call_jcode`（`bridge-jcode-execute`）。各入口均接入同一 best-effort token 采集，不改执行路径。
4. **配置与开关**：增加最小配置（env/Config 字段二选一，按现有模式）；更新 `config.example.toml` 与 `book/`（若引入 Config 字段）。
   - **当前进展**：采用 env opt-in（`SISYPHUS_BRIDGE_QUALITY_RECORDS_DIR`），未新增 `Config` schema 字段，故未修改 `config.example.toml`/`book/` 配置章节。
5. **测试**：
   - 单元测试：记录映射（成功/失败/无 token/dirty 归因/差值）与 JSON 往返。
     - **已完成**：`cargo test -p sisyphus-bridge --lib quality_record` → 6 passed（文件名清洗、task id 构造、pass/fail 字段映射、原子写入与反序列化、token delta 2 项）；sha 解析/非 git 目录/dirty 标签由共享 helper 的 6 个单测覆盖（`cargo test -p sisyphus-metrics --lib engine_revision`）。
   - 聚合兼容性：样本记录可被 `sisyphus metrics quality --records <dir> --json` 正确解析（成功率/耗时/版本归因符合预期，fix/regression 为 `null`）。
   - 集成测试：在可控 bridge 执行路径下断言记录文件生成且 `sisyphus metrics quality` 可聚合（注意 bridge 测试在 CI 的 OOM 约束与 `#[ignore]` 策略，按仓库惯例处理）。
     - **已完成（live）**：HTTP `execute_goal` 失败路径（`bridge-task-record-e2e.md`）；composite 成功 + goal streaming 失败（`bridge-task-record-coverage.md`）；gRPC `execute_sop` 失败；gRPC `stream_execution` goal 分支与非 goal/SOP 分支失败、gRPC `call_jcode` 失败（含非 null token，`runs/bridge-record-coverage/grpc-stream-calljcode/`、`grpc-stream-sop/`）。
     - **待后续**：HTTP `execute_goal` 成功路径；更多可控场景/成功路径复核。
6. **文档与证据**：更新 `docs/plans/single-node-quality-metrics.md`、`docs/plans/automation-development-plan-2026-11.md` 状态与边界说明；产出一次真实运行的记录与聚合快照作为证据。
   - **当前进展**：设计文档与 2026-11-13 E2E/覆盖扩展/gRPC live 证据已归档（含 token-captured 与 grpc-stream-calljcode）；质量指标文档已同步 bridge 记录语义；自动化计划的系统性状态同步与成功路径证据待后续补齐。

## 6. 风险与开放问题

- **多路径覆盖**：仅接线 HTTP goal 会遗漏 gRPC/composite/streaming；需在后续迭代下沉到共享执行层并补齐验收。
- **token/usage 来源**：**已实现 best-effort 采集**（独占租约 + 累计差值，见 §3 步骤 3）：provider 上报且租约独占时可产出真实增量；并发/无 usage 时仍为 `None`（不伪造 0）。执行级确定性来源仍待内核/客户端支持。
- **并发与高频写入**：原子写与唯一文件名可缓解；若执行量增大需考虑轮转/索引（非本次范围）。
- **语义误读**：`Pass` 不等于外部验收通过；需在 metrics 输出或文档中提示按 `scenario` 区分来源。

## 7. 验收标准（实现阶段）

- 至少 1 条来自 bridge `execute_goal` 的 `TaskRecord` 落盘（成功路径）与 1 条失败路径记录，字段完整、可被 `sisyphus metrics quality` 正确解析聚合。
  - **当前进度**：`execute_goal` 失败路径已满足；成功路径未取得（需要可用 LLM/审计环境）。成功路径记录已由 composite 入口产出（`bridge-composite-execute`），但**不能替代** `execute_goal` 的验收。
- `engine_commit`/`engine_dirty` 填充规则与 CLI 一致；无法获取时显式为 `None`（不猜测）。
- 落盘失败不影响原有 HTTP 响应/状态码/SSE 行为（回归测试覆盖）。
- 文档同步（计划状态、质量指标口径、配置示例如适用）。

## 8. 执行级确定性 token 来源（设计，待实现）

**现状**：`DynamicLlmClient` 仅维护进程级累计计数（`TokenUsageCounters`）并通过 `token_usage()` 暴露快照；bridge 以「独占租约 + 累计差值」在单执行独占时归因，已在 live 中验证可产出真实增量，但并发/租约缺失/跨任务交错时无法确定性归因（保持 `null`，不伪造）。

**候选方案（需评估后选型）**：

1. **执行作用域累加器（推荐方向）**：在 LLM 调用记录点同时写入“当前执行作用域”的累加器。
   - 形态：`sisyphus-tools-llm` 提供作用域 API（如 `UsageScope` guard / `tokio::task_local!` + `scope`），`record()` 在更新全局累计的同时，若存在活动作用域则原子累加到该作用域；bridge/内核在执行入口包裹 scope，结束后直接读取作用域快照作为该次执行的 usage（不再依赖全局差值与独占租约）。
   - 优点：不改变 `LlmClient` trait 签名（对调用者透明），可与全局累计并存；单执行/并发均可确定性归因（前提：同一执行内的 LLM 调用在 scope 内完成，或显式传递 scope）。
   - 风险/成本：`tokio::spawn` 不自动继承 task-local，内核若在执行期跨 spawn 调用 LLM 需显式传播 scope（或改为 `scope` 包裹整个执行 future）；需审计调用路径与测试覆盖。
2. **按调用返回 usage 并向上聚合**：让 provider/client 在每次 `chat` 返回 `(content, usage)` 或 side-channel，并在执行上下文累加。
   - 优点：归因最直接、与全局计数解耦。
   - 风险/成本：需改 `LlmClient` trait 与所有实现/调用者（`generate/chat` 签名或新增方法），影响面大，需全量审计；与当前“不改 trait 签名而不审计所有调用者”约束冲突，适合作为较大重构单独推进。
3. **执行隔离 client 实例**：每次执行构造/持有独立的计数域或 client 包装器。
   - 优点：隔离天然确定。
   - 风险/成本：client/registry/连接池生命周期与配置热更新需梳理；可能增加资源开销与改造面。

**落地建议（最小增量）**：优先验证方案 1 —— 在 `DynamicLlmClient`（及 provider-registry 路径可达的记录点）增加可选 execution-scope 累加；bridge 从“租约+全局差值”切换为“scope 快照”并保留 `null` 语义（无上报仍 `null`）；补单测（并发两执行互不污染）与一条 live 证据。实现前需确认内核执行期 LLM 调用的 task/spawn 边界并补齐 scope 传播。