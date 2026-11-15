# Bridge 路径 TaskRecord 落盘与 `metrics quality` 覆盖（最小设计）

> 状态：**已覆盖主要执行入口**（2026-11-13）。HTTP `execute_goal` / composite / goal streaming，以及 gRPC `execute_sop` / `stream_execution`（goal 与非 goal 分支）/ `call_jcode` 均已接线 best-effort 落盘；单测与聚合兼容性验证完成。live E2E 已覆盖 `execute_goal` 失败路径、`composite` 成功 + `goal streaming` 失败、gRPC `execute_sop` 失败、gRPC `stream_execution`（goal 分支与非 goal/SOP 分支）失败、gRPC `call_jcode` 失败（含非 null token 样本，见 `docs/benchmarks/acceptance/2026-11-13/bridge-task-record-e2e.md`、`docs/benchmarks/acceptance/2026-11-13/bridge-task-record-coverage.md` 与 `runs/bridge-record-coverage/grpc-stream-calljcode/`、`grpc-stream-sop/`）。token 归因已引入 **执行作用域（task-local）优先**：`DynamicLlmClient::scope_usage`（实现下沉为 `sisyphus_types::usage_scope`）+ bridge `run_with_usage_scope`，跨 `tokio::spawn` 边界由 `sisyphus-core` 编排层显式传播，不可用时回退独占租约+累计差值（仍 best-effort，未走传播原语的第三方自 spawn 调用可能无法归因）。**待后续**：无（2026-11-14 已补齐 `execute_goal` 成功路径 live 与 scope 跨 spawn 传播证据，见 `../2026-11-14/goal-success-path.md`）。
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

**首选最小落点**：HTTP `execute_goal_handler` 的完成分支（成功/失败均写一条记录），因为上下文完整、语义清晰、易做集成测试与外部验证。写入逻辑已覆盖全部 6 个入口（HTTP goal/composite/streaming、gRPC execute_sop/stream_execution/call_jcode）；共用的确定性部分（usage→token 映射）已收敛为 `quality_record::scope_usage_token_pair`，各入口的计时/分支/事件排空语义差异**刻意保持显式接线**（强行抽象会掩盖各入口的 pass/fail 判定差异）。

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
   - **已完成**：探测逻辑迁移到 `sisyphus-metrics::engine_revision`（`engine_revision()` / `engine_revision_at()` / `EngineRevision`，含单测：sha 规范化、非 git/不存在目录、dirty 标签、仓库内解析；数量以实跑输出为准）；`sisyphus-cli::engine_revision` 保留为 re-export 稳定路径，bridge `quality_record` 直接复用同一实现，不再各自探测。
2. **Bridge 记录写入模块**：新增轻量 writer（依赖 `sisyphus-metrics::quality::TaskRecord`），负责序列化、目录创建、原子写文件、best-effort 错误处理。
   - **已完成（最小）**：`quality_record` 模块实现原子写入与 best-effort 告警；通过 `SISYPHUS_BRIDGE_QUALITY_RECORDS_DIR` 环境变量 opt-in 启用（未设置则不写）。
3. **接线 `execute_goal_handler`**：计时开始/结束；在成功与失败分支组装记录并调用 writer；补充 `tracing` 字段（`task_id`/`duration_ms`/`status`）便于排查。
   - **已完成**：HTTP `/api/execute/goal` 成功/失败（含响应序列化失败）分支写入 `scenario=bridge-goal-execute` 记录。
   - **token usage（best-effort，已增强）**：优先使用 **执行作用域归因**（`DynamicLlmClient::scope_usage` / task-local，`bridge.run_with_usage_scope`）：在作用域内可见的 provider 上报 usage 会同步写入全局累计与当前作用域，记录取作用域快照（不依赖全局差值）；`tokio::spawn` 子任务不自动继承 scope，跨 spawn 的 LLM 调用可能无法归因。无 scope 客户端/不可用时回退为独占租约 + 累计差值；并发/无 usage 时仍保持 `null`（未知 ≠ 0）。`token_usage_delta` 与 scope 单测已覆盖（`scope_usage_captures_reported_tokens_deterministically` 等）。
   - **已扩展**：同一 writer 现覆盖 HTTP composite（`bridge-composite-execute`）、HTTP/gRPC goal streaming（`bridge-goal-streaming`）、gRPC `execute_sop` 与流式 SOP/工具分支（`bridge-sop-execute`）、gRPC `call_jcode`（`bridge-jcode-execute`）。各入口均接入同一 best-effort token 采集，不改执行路径；usage→(prompt,completion) 映射收敛为 `quality_record::scope_usage_token_pair`（6 入口共用，含 None 语义单测，防「某入口把 None 写成 0」的口径漂移）。
4. **配置与开关**：增加最小配置（env/Config 字段二选一，按现有模式）；更新 `config.example.toml` 与 `book/`（若引入 Config 字段）。
   - **当前进展**：采用 env opt-in（`SISYPHUS_BRIDGE_QUALITY_RECORDS_DIR`），未新增 `Config` schema 字段，故未修改 `config.example.toml`/`book/` 配置章节。
5. **测试**：
   - 单元测试：记录映射（成功/失败/无 token/dirty 归因/差值）与 JSON 往返。
     - **已完成**：`cargo test -p sisyphus-bridge --lib quality_record`（计数以实跑输出为准，不在此固化：先前为 6 项，`0004dab` 新增 `scope_usage_token_pair` None 语义单测后增加 1 项）；sha 解析/非 git 目录/dirty 标签由共享 helper 覆盖（`cargo test -p sisyphus-metrics --lib engine_revision`，同样以实跑输出为准）。
   - 聚合兼容性：样本记录可被 `sisyphus metrics quality --records <dir> --json` 正确解析（成功率/耗时/版本归因符合预期，fix/regression 为 `null`）。
   - 集成测试：在可控 bridge 执行路径下断言记录文件生成且 `sisyphus metrics quality` 可聚合（注意 bridge 测试在 CI 的 OOM 约束与 `#[ignore]` 策略，按仓库惯例处理）。
     - **已完成（live）**：HTTP `execute_goal` 失败路径（`bridge-task-record-e2e.md`）；composite 成功 + goal streaming 失败（`bridge-task-record-coverage.md`）；gRPC `execute_sop` 失败；gRPC `stream_execution` goal 分支与非 goal/SOP 分支失败、gRPC `call_jcode` 失败（含非 null token，`runs/bridge-record-coverage/grpc-stream-calljcode/`、`grpc-stream-sop/`）。
     - **已完成（live）**：HTTP `execute_goal` 成功路径（`docs/benchmarks/acceptance/2026-11-14/goal-success-path.md`，`status=pass`，`prompt=2378`/`completion=720`，执行作用域归因）；失败路径、composite/streaming/gRPC 各入口见 2026-11-13 覆盖文档。
6. **文档与证据**：更新 `docs/plans/single-node-quality-metrics.md`、`docs/plans/automation-development-plan-2026-11.md` 状态与边界说明；产出一次真实运行的记录与聚合快照作为证据。
   - **已完成**：设计文档、2026-11-13 E2E/覆盖扩展/gRPC live 证据、2026-11-14 成功路径 live 证据均已归档；质量指标文档与自动化计划状态已同步（含作用域传播与审计降级说明）。

## 6. 风险与开放问题

- **多路径覆盖**：已覆盖 HTTP goal/composite/streaming 与 gRPC execute_sop/
  stream_execution/call_jcode（2026-11-13 起）；后续增强方向为记录下沉到共享执行
  编排层以减少入口级重复接线（非功能缺口）。
- **token/usage 来源**：**已实现执行级确定性归因**：task-local 执行作用域（`sisyphus_types::usage_scope`，对外经 `DynamicLlmClient::scope_usage` 暴露）为首选路径，作用域覆盖到的 provider 上报 usage 直接计入作用域快照；跨 spawn 边界由编排层显式传播；不可用时回退独占租约+累计差值（best-effort）；并发/无 usage 时仍为 `None`（不伪造 0）。
- **并发与高频写入**：原子写与唯一文件名可缓解；当前记录量未达轮转阈值。
  轮转策略（2026-11-14 约定，触发再实施）：单目录记录数 > 1000 或目录体积 > 1 GB
  时，按 `recorded_at` 月份轮转到 `records/YYYY-MM/` 子目录；聚合命令保持接受目录
  （递归 glob 兼容），历史扁平目录不迁移；索引（如按 task_id 的 manifest）仅在轮转
  落地后仍出现聚合延迟 > 10s 时再评估。
- **语义误读**：`Pass` 不等于外部验收通过；需在 metrics 输出或文档中提示按 `scenario` 区分来源。

## 7. 验收标准（实现阶段）

- 至少 1 条来自 bridge `execute_goal` 的 `TaskRecord` 落盘（成功路径）与 1 条失败路径记录，字段完整、可被 `sisyphus metrics quality` 正确解析聚合。
  - **已完成（live，2026-11-14）**：成功路径 `status=pass` + 失败路径样本齐全，见 `docs/benchmarks/acceptance/2026-11-14/goal-success-path.md`；成功路径含非 null token（执行作用域归因）。
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

**实现进展（2026-11-13，方案 1 最小增量）**：
- `sisyphus-tools-llm`：新增 task-local 执行作用域（`EXECUTION_USAGE_SCOPE`），`TokenUsageCounters::record` 同步写入全局累计与当前作用域；提供 `DynamicLlmClient::scope_usage` / `current_scope_usage_snapshot()`；补单测 `scope_usage_captures_reported_tokens_deterministically`、`scope_usage_is_unknown_when_provider_omits_usage`、`scope_usage_isolates_concurrent_scopes`（`cargo test -p sisyphus-tools-llm --lib scope_usage`，计数以实跑输出为准）。
- `sisyphus-bridge`：持有可选具体 `DynamicLlmClient`（默认构造路径），新增 `run_with_usage_scope`（scope 优先，回退租约+差值），HTTP/gRPC 记录入口已切换为该路径；`cargo check/clippy -p sisyphus-tools-llm -p sisyphus-bridge`、`cargo test -p sisyphus-bridge --lib quality_record` 均通过（计数以实跑输出为准）。
- **live 证据（scope 路径）**：HTTP `execute_goal` 独占运行（`33c36f1ffb5a+dirty`，作用域路径）产出 `prompt_tokens=515` / `completion_tokens=479`（`runs/bridge-record-coverage/token-scope-captured/`，失败路径/模型能力边界，归因非 null）。
- **跨 spawn 传播（2026-11-14 落地）**：作用域实现从 `sisyphus-tools-llm` 私有 task-local 下沉为共享原语 `sisyphus_types::usage_scope`（`UsageScope` 累加器 + `with_scope` / `captured_scope` / `attach_scope` / `spawn_in_scope` / `record_current` / `current_snapshot`），原因：执行编排层（`sisyphus-core`）生产代码不依赖 LLM 客户端 crate，只有放在共享类型层才能让「记录方」与「编排方」共用同一 task-local。`sisyphus-core` 已在所有会发起 LLM 调用的 spawn 边界显式传播：DAG 节点执行（`executor.rs` 两处 `JoinSet::spawn`）、推测预测、认知内化、自编程循环（`cognitive_loop.rs`）、composite 子 SOP（`kernel/execution.rs` 两处 `JoinSet::spawn`）；`sisyphus-tools-agent` 的团队编排/委派/开发者并行步骤（`orchestrator.rs`、`delegate.rs` 两处、`developer.rs`）同样接入（其 handler 内会发起 LLM 调用）。`sisyphus-intel` MCTS 批量仿真 spawn 经核实为纯启发式（唯一 LLM 调用在 `simulate_logic` 主任务内），core 余下两处裸 `tokio::spawn`（trace 持久化 writer / trace recorder）无 LLM 调用。`sisyphus-types` 补单测（捕获/未知语义/部分上报/跨 spawn 传播/普通 spawn 反向对照/无作用域语义/并发隔离/嵌套不泄漏；覆盖项以清单为准，计数以实跑输出为准）；`sisyphus-tools-llm` 原 `scope_usage` / `current_scope_usage_snapshot` API 改为委托实现，原 scope 单测保持通过（计数以实跑输出为准）。剩余未传播风险：第三方 crate 内自 spawn 且未走本原语的路径（全工作区复查仅发现 `sisyphus-tools-agent` 四类站点，已全部接线；其余裸 spawn 经核实均无 LLM 调用）。
- **审计循环健壮性（2026-11-14 落地）**：`sisyphus-core::swarm_coordinator` 的 Swarm 审计循环对弱模型输出做了三处降级（不中止执行）：架构师提案不可解析/LLM 失败 → 维持当前配置继续审计；审计员意见不可解析 → 记为「不批准」并带说明性 feedback（绝不静默批准）；refine 失败 → 维持上一版提案（迭代上限强制收尾）。同时把 LLM JSON 提取统一为 `sisyphus_intel::json_extract::extract_json`（剥离 ```json 栅栏），接线批判/反思/演进 ops/合成/审计提案共 5 处解析点（此前弱模型带栅栏返回会直接整体失败）。集成测试 `swarm_audit_degrades_when_architect_proposal_is_unparseable`（`crates/sisyphus-tests/tests/single_node_full_stack.rs`）覆盖提案损坏+批判带栅栏的端到端降级路径，`json_extract` 单测覆盖（计数以实跑输出为准）。
- **限制（2026-11-14 更新）**：跨 `tokio::spawn` 传播已由 `usage_scope` 原语解决（见上，含 `sisyphus-tools-agent` 团队/委派/开发者站点）；剩余风险仅为未来新增 spawn 点未走本原语（约定已写入模块文档）。成功路径 live 对照已补齐（`docs/benchmarks/acceptance/2026-11-14/goal-success-path.md`）。