# 并行编码代理成熟化路线图（2026-11）

> 状态：Draft（可执行路线图）  
> 日期：2026-11-18  
> 依据：本仓库现状（Sisyphus v0.4.1）、`docs/plans/automation-development-plan-2026-11.md`、以及对 OpenCode / Claude Code / jcode 的对标分析结论。

## 1. 背景与目标

当前 Sisyphus 已具备认知引擎（DAG/`predict → execute → reflect → evolve`）、gRPC/REST bridge、CLI/TUI、工具链与部分并行编排基础（`sisyphus-tools-agent::team` 的任务 DAG、冲突检测/分波次串行化、流式事件；`sisyphus-tools-session::collaboration` 的会话协作原语等）。但距离“成熟的自动化并行开发 AI 编码代理工具”仍有差距：**并行编码运行时的产品化闭环、多代理协作体验、编码任务成功率与可观测性、生态集成与性能**尚未系统收敛。

**目标（北极星）**：在单机优先前提下，交付可稳定完成“目标分解 → 多代理并行编码 → 冲突感知与协调 → 自动验证/修复 → 可审计汇报”的编码代理工作流。

**非目标（本阶段）**：
- 不复刻桌面端/Tauri（`ide/tauri` 已归档方向不回退）。
- 不以控制面生产写入/分布式生产准入为前置（遵循现有 ADR/计划边界）。
- 不追求一次性对标所有成熟产品能力；以可验证里程碑迭代。

## 2. 对标借鉴要点（转化为能力项）

| 来源优势 | 转化目标 | 优先级 |
|---|---|---|
| jcode：多代理并行/Swarm、冲突感知、低开销会话 | 并行编码运行时（调度、文件级冲突、事件与协调原语）、性能预算 | P0 |
| OpenCode：配置/MCP/插件/API-SDK 生态与 client-server 体验 | 统一配置与扩展面、稳定协议与可嵌入性 | P1 |
| Claude Code：高质量编码闭环与护栏体验 | 计划→编辑→验证→迭代闭环可靠性、权限与可审计执行 | P0/P1 |

## 3. 分阶段路线图

### Phase M0（1–2 周）：基线、度量与场景定义
**目标**：让“并行编码代理”可度量、可回归。

- **M0.1 并行编码场景集**：定义 ≥5 个代表性场景（并行修复多文件 lint/test、跨模块功能拆分、读多写少调研+落码、同文件冲突压力场景、长任务中断恢复）。
- **M0.2 指标与门禁**：扩展质量/性能指标口径（并行任务成功率、冲突检测率/误杀率、增量会话资源占用、端到端 p50/p95、验证通过率）。
- **M0.3 基准脚本与证据模板**：可复跑脚本 + `docs/benchmarks/` 归档格式（遵循 ADR-021：计数以实跑为准）。

**验收**：场景与指标文档落地；至少 1 条基准可在本地复跑并产出记录。

### Phase M1（2–4 周）：并行编码运行时内核
**目标**：稳定的“多 agent 并行执行 + 冲突感知 + 可观测”。

- **M1.1 编码感知的冲突推断**：从任务参数中自动推断文件读/写键（`path/file/files/...` 等启发式），与显式 `read_keys/write_keys` 合并，降低 planner 遗漏导致的静默覆盖风险；保持向后兼容与可测试。  
  - *初始实现（本次）*：`sisyphus-tools-agent::team::conflict` 增强写键推断与单测覆盖。
- **M1.2 工作区协调原语**：将 `TeamContext`/协作会话与文件变更事件关联，支持“变更通知→受影响任务重评估”的最小闭环。
- **M1.3 并行执行可观测性**：统一事件模型（任务状态、冲突、串行化波次、耗时/token/验证结果）并通过 bridge SSE/CLI/TUI 一致呈现。
- **M1.4 失败恢复策略**：任务级重试/降级/人工介入点标准化（复用 delegation policy，补充编码场景策略）。

**验收**：冲突压力场景下无静默覆盖；并行执行报告可解释每次串行化原因；基础回归测试稳定。

### Phase M2（3–5 周）：编码闭环可靠性（产品化核心）
**目标**：从“能并行跑”到“能稳定交付代码改动”。

- **M2.1 计划→编辑→验证→修复闭环**：将 post-edit 验证、`verify-fix` 循环与并行任务编排打通，验证失败可定向重试对应子任务。
- **M2.2 代码操作护栏**：权限分级、危险命令审批、沙箱/工作树隔离策略默认化；操作与决策可审计。
- **M2.3 上下文与记忆**：利用 `sisyphus-mem/knowledge` 提供跨任务上下文复用与失败经验回注（避免重复错误）。
- **M2.4 结果质量**：引入端到端评测（evals 子集扩展到并行编码场景），跟踪成功率/修复率/回归率。

**验收**：≥3 类真实编码场景达到可复现的闭环通过（有凭据/环境时）；失败可解释且可恢复。

### Phase M3（3–4 周）：体验与生态集成
**目标**：可被开发者稳定使用并嵌入工作流。

- **M3.1 CLI/TUI 并行视图**：并行任务树、每个 agent 状态/日志、冲突与审批交互、会话恢复。
- **M3.2 配置与可发现性**：并行代理/策略/预算配置项收敛（`sisyphus-types` + 示例 + 文档同步）。
- **M3.3 协议与扩展**：MCP/工具接入体验、REST/gRPC/OpenAPI 契约稳定、SDK 用例补齐（面向自动化编排调用）。
- **M3.4 模板化**：提供并行编码命令/模板（如拆模块实现、并行修测、并行文档与代码同步）。

**验收**：新用户可按文档完成一次并行编码任务；核心 API/配置有文档与兼容策略。

### Phase M4（持续）：性能、规模化与成熟度
**目标**：向“大规模并行、低开销”逼近（对标 jcode 性能思路），同时守住正确性。

- **M4.1 性能专项**：多会话/多 agent 资源画像、增量内存与调度开销优化、流式渲染与 I/O 优化；建立性能预算与回归门禁。
- **M4.2 稳定性与长跑**：长时间并行执行的泄漏/退化监测、故障注入与恢复验证。
- **M4.3 产品化打磨**：错误提示、可观测诊断、默认策略调优，基于真实使用数据迭代。

## 4. 实施原则与治理

- **增量可验证**：每个里程碑绑定场景、测试与证据；遵循仓库约定（`cargo check-all`/`make audit`/TODO 证据规则、无 `unwrap` 生产代码等）。
- **接口与配置同步**：涉及 trait/config/协议变更时，审计调用方并同步 `config.example.toml` 与 `book/` 文档。
- **安全与许可**：借鉴外部项目的设计思想与公开行为，不直接复制受限代码；依赖变更需符合 `deny.toml` 与供应链策略。
- **单机优先**：先把单机并行编码体验做到可靠，再评估分布式/多实例扩展。

## 5. 本轮落地（2026-11-18）

- [x] 新增本路线图文档：明确从当前基础到“成熟自动化并行开发 AI 编码代理”的分阶段目标、度量与里程碑。
- [x] **M1.1 初步实现（持续）**：增强 `crates/sisyphus-tools-agent/src/team/conflict.rs` 的编码场景冲突推断（向后兼容）。
  - 写键推断：从常见文件参数启发式提取并与显式 `write_keys` / `args["_write_keys"]` 合并，降低未声明写键导致的同文件静默覆盖风险。
  - 读键推断：新增 `read_keys_of` 与读意图启发式（`_read_keys`/常见路径参数），提升读写冲突检测覆盖率；`detect_conflicts` 使用推断后的读/写键集合。
  - 导出：`team` 模块补充导出 `read_keys_of` / `write_keys_of`。
  - 验证：`cargo test -j 1 -p sisyphus-tools-agent --lib` 通过（计数以实跑输出为准，含 conflict 推断相关新增用例）；`cargo clippy -p sisyphus-tools-agent --all-targets -- -D warnings` 通过。
- [x] **M0.1 / M0.2 / M0.3 场景、字段映射与确定性证据落地（含扩展）**：
  - 新增场景集：`docs/plans/parallel-coding-scenarios.md`（S1–S5 场景、环境分级、指标口径、证据归档建议）。
  - M0.2 字段映射：在场景文档中补充“可采集字段映射”与 `summary.json` schema（v1），明确通用执行字段来源与 team 冲突指标（`TeamReport.conflict_stats` / `TeamEvent::ConflictDetected`）采集口径。
  - 新增/增强证据采集模板脚本：`scripts/parallel-coding-bench-template.sh`（生成 `summary.md`/`env.txt`/`summary.json`；可选执行被测命令并记录退出码/耗时/日志，不内置伪造结论）。
  - 归档可复跑证据（基于 `parallel-coding-bench-template.sh`，命令/退出码/日志/环境以归档文件为准）：
    - S1（无冲突并行基线）：`docs/benchmarks/parallel-coding/2026-11-18-s1-parallel-no-conflict/`
    - S2（同文件写冲突检测与串行化）：`docs/benchmarks/parallel-coding/2026-11-18-s2-write-conflict-serialization/`
    - S3（并行读写冲突检测子场景）：`docs/benchmarks/parallel-coding/2026-11-18-s3-read-write-conflict-detected/`
    - S4（失败→重试/回退恢复子场景，委托策略）：`docs/benchmarks/parallel-coding/2026-11-18-s4-delegation-retry-fallback/`
  - S5（中断与恢复）：已落地最小可测中断恢复切片（`team/checkpoint.rs` 的 `TeamCheckpoint`/`TeamContextSnapshot`/`TeamResumeState` 保存与加载、`execute_plan_with_resume`/`execute_plan_from_checkpoint` 及确定性恢复测试）；CLI `team run` 已接入工具变更观测与影响记录输出，为后续持久化触发提供基础。端到端中断注入/可配置持久化恢复触发/幂等副作用场景化证据仍待深化（场景文档同步更新状态）。
- [x] **M1.3 可观测性（初步，持续）**：并行冲突/串行化统计进入执行报告链路，并补充对外事件契约辅助。
  - `sisyphus-tools-agent::team`：新增 `ConflictStats`（`detected/write_write/read_write/serialized_waves/conflict_batches`，`#[serde(default)]` 兼容旧数据）并纳入 `TeamReport.conflict_stats`；`TeamReport` 同步暴露 `workspace_impact_records` 与 `processed_change_count`（`#[serde(default)]`）。
  - 调度器（`scheduler.rs`）：在冲突检测/分波次执行时累计统计并写入最终报告；保持 `ConflictDetected`/`WorkspaceImpact` 等事件发射与告警行为。
  - 编排器（`orchestrator.rs`）：多轮/多阶段聚合报告补齐/合并冲突与影响记录并携带处理进度；反思（`reflect`）提示词纳入 `conflict_stats`、`workspace_impact_records` 与近期工作区变更，支持影响感知的再规划建议。
  - CLI（`sisyphus-cli/src/delegate.rs`）：`team run` 报告输出冲突与串行化统计摘要、工作区影响记录摘要及编排后验证结果。
  - Bridge（`sisyphus-bridge`）：`SseEvent` 新增 team 事件 `Custom` 构造辅助（`team_task_started`/`team_task_completed`/`team_conflict_detected`/`team_workspace_impact`/`team_synthesizing`/`team_complete`）、`from_team_event` 转换（`TeamEvent` → SSE，含 `team_planning`/`team_plan_ready`/`team_reflecting`/`team_replanning`/`team_impact_retry`/`team_error`/`team_complete` 等）及 `forward_team_events_to_sse` 转发辅助。
  - Bridge 端到端广播接线（深化）：新增 `POST /api/team/execute`（显式 `TeamPlan`，避免依赖未装配的 planner LLM），由 bridge `TeamOrchestrator` 执行并把 `TeamEvent` 实时广播到 `/api/events`；`TeamSseScope` 注入 `goal_id`/`project_id`/`trace_id` 维度；`SseQuery`/`SseFilters` 新增 `?goal_id=` 过滤（`SseEvent::goal_id()` 覆盖认知迭代/SOP/Custom team 事件），响应返回 `sse_filter` 订阅建议；请求支持 `impact_retry_rounds`（M1.2）。OpenAPI 同步收录路径与 `Team*` schema。
  - 测试：`team_execute_handler_broadcasts_scoped_events`（handler 端到端 + SSE 维度/过滤）、`test_forward_team_events_injects_scope_dimensions`、`test_team_events_pass_goal_filter`、`test_cognitive_iteration_events_pass_goal_filter`、`every_route_is_documented` 通过；证据：`docs/benchmarks/parallel-coding/2026-11-19-m13-team-sse-endpoint/`（`exit_code=0`，命令与输出以归档为准）。
  - SSE 断线重连事件重放（深化）：新增 `http/sse_replay.rs` —— `SseReplayBuffer`（有界环形缓冲，默认 256 条）、`PublishedSseEvent { id, event }`（广播单元，`Deref` 兼容既有订阅代码）、`SsePublisher`（**先记录后广播**的唯一写入口）。事件在每个发布点分配进程内单调 ID 并写入 SSE `id:` 字段；`/api/events` 连接携带 `Last-Event-ID` header（优先）或 `?last_event_id=` 时，先补发严格大于该 ID 的缓冲事件再接续实时流；订阅与快照重叠窗口通过 `replayed_through` 游标去重。服务重启导致游标大于最新 ID 时退回实时流（避免永久跳过新事件）；旧版 UUID 客户端按「未声明断点」处理。重放同样受 `events`/`sop_id`/`session_id`/`project_id`/`goal_id` 过滤约束。`/api/events/health` 新增 `replay.{buffered,capacity,latest_id}` 指标，OpenAPI 同步 `last_event_id`/`Last-Event-ID` 参数与重放语义。
  - 测试（重放）：`sse_replay` 缓冲单测 6 项（单调 ID、游标过滤、容量淘汰、无订阅者仍可补发、发布/广播同 ID）+ handler 级 5 项（游标补发、header 优先、无游标不重播、重放受维度过滤、重启游标回退）；证据：`docs/benchmarks/parallel-coding/2026-11-19-m13-sse-replay/`（`exit_code=0`，命令与输出以归档为准）。
  - 待深化：TUI 团队冲突/影响状态展示；跨路径（gRPC/CLI）统一订阅语义；重放窗口可配置化（配置项/环境变量）与跨进程序号（多实例/重启后仍可续传）。
  - 测试加固：补充报告级 `conflict_stats` 断言（S1 无冲突为零、S2 写冲突与串行化波次可观测）。
  - 验证：`cargo test -j 1 -p sisyphus-tools-agent --lib`、`cargo check -p sisyphus-cli -p sisyphus-bridge` 通过（计数以实跑输出为准）。
- [x] **M1.2 工作区协调原语（最小闭环：接口 + 调度内信号/标记接线，持续深化）**：
  - `sisyphus-tools-agent::team::context`：新增 `WorkspaceChange`/`WorkspaceChangeKind` 与事件记录/清理接口；基于读写键（含启发式推断）的 `affected_tasks_by_changes` / `tasks_affected_by_recorded_changes`，提供“变更通知 → 受影响任务重评估”所需最小原语。
  - `team` 模块导出上述类型；补充单元测试覆盖记录/清理与受影响任务判定。
  - 策略与审计（深化）：新增 `WorkspaceImpactPolicy`（`Observe`/`RecordAffected`/`FailAffectedPending`）、`WorkspaceImpactRecord` 及上下文标记/查询/清理接口（受影响任务 id 去重记录、批次影响审计记录）。
  - 调度接线（`scheduler.rs`）：成功任务完成后按声明/推断写键记录工作区变更；每个 ready 批次执行后消费 `TeamContext` 中新增变更（含外部/回调记录）并评估对**仍未完成**任务的影响，发出 `TeamEvent::WorkspaceImpact`，按策略写入可审计标记；`FailAffectedPending` 策略将受影响待执行任务定向标记为失败（可审计）以避免在失效前提下继续执行。
  - 工具/执行回调事件源（深化）：`sisyphus-tools-executor` `BuiltinToolExecutor` 新增 `file_mutation_sink`，`file_write`/`file_edit`/`file_edit_all` 成功后回调路径；CLI `team run`（`run_team_with_config`）将 sink 绑定到共享 `TeamContext`，实现工具文件写入的自动注入记录（路径级）。
  - 影响感知反思/重规划输入（深化）：`TeamOrchestrator::reflect` 提示词纳入 `conflict_stats`、`workspace_impact_records` 与近期工作区变更，引导在 gaps/deviations/suggested_tasks 中产出针对性补救/重跑建议（迭代工作流中的定向重规划信息输入）。
  - 自动定向重跑（深化）：`TeamOrchestrator::execute_plan_with_impact_retry(plan, event_tx, context, max_rounds)` 首轮正常执行，随后仅对 `TaskResult::is_workspace_impact_failure()` 的任务构造恢复态（排除影响类失败）重跑；已完成/非影响类失败任务不重复调度，每轮发出 `TeamEvent::ImpactRetry { round, task_ids }`，轮次上限保证终止；报告 `iterations` 反映实际轮次并在 synthesis 标注 `[impact-retry]`。`TaskResult::is_workspace_impact_failure()` 统一识别逻辑（调度/恢复态/CLI 共用，避免错误字符串匹配漂移）。
  - CLI 接线：`team run` 支持 `SISYPHUS_TEAM_IMPACT_RETRY_ROUNDS=<N>`（默认 `0` 关闭）启用上述自动定向重跑；与 `SISYPHUS_TEAM_CHECKPOINT_OUT`/`SISYPHUS_TEAM_INTERRUPT_AFTER_BATCHES` 路径互斥，保持检查点/中断恢复语义可预期。
  - 报告可审计性：`TeamReport.workspace_impact_records`（`#[serde(default)]`）与 `TeamReport.processed_change_count`（`#[serde(default)]`，支持检查点连续性）记录批次影响评估/变更消费进度，orchestrator 多阶段/多轮聚合时合并影响记录并携带处理进度。
  - 测试：新增 context 策略/记录单测与 orchestrator 集成测试 `test_workspace_impact_recorded_for_pending_task`、`test_workspace_impact_fail_affected_pending_strategy`、`test_impact_retry_recovers_failed_pending_task`、`test_impact_retry_disabled_with_zero_rounds`、`test_impact_retry_skips_non_impact_failures`；新增 M0.3/S3 用例 `test_parallel_read_write_conflict_is_detected`；executor 增加文件变更 sink 回调测试。
  - 验证：`cargo test -j 1 -p sisyphus-tools-executor`、`cargo test -j 1 -p sisyphus-tools-agent --lib`（169 passed, 0 failed）、`cargo check -p sisyphus-cli -p sisyphus-bridge` 通过（计数以实跑输出为准）。
  - 证据：`docs/benchmarks/parallel-coding/2026-11-19-s4-impact-auto-retry/`（自动定向重跑三用例，`exit_code=0`；命令与输出以归档为准）。
  - 待深化：覆盖更多工具/FS 事件源与非 CLI 执行路径的自动注入；基于影响的子计划重生成（LLM/启发式）与跨轮次预算/回归保护，并补充场景化端到端证据。

- [x] **M0.3/S5 最小恢复切片（持续）**：
  - 新增 `crates/sisyphus-tools-agent/src/team/checkpoint.rs`：`TeamCheckpoint`/`TeamContextSnapshot`/`TeamResumeState`（含计划、已完成结果、冲突统计、上下文快照、工作区变更/影响审计与 `processed_change_count`）支持 JSON 保存/加载与上下文恢复。
  - 调度/编排：`execute_plan_with_resume`（公开 API）、`execute_plan_with_resume_and_checkpoint`（支持周期性检查点保存与 `interrupt_after_batches` 确定性中断注入，返回部分报告并保留可恢复检查点）、`TeamOrchestrator::execute_plan_from_checkpoint` 支持从检查点恢复继续执行剩余任务；`TeamCheckpoint::from_report` 便于从报告构建检查点；`TeamReport.processed_change_count` 提供恢复连续性字段。
  - 周期/完成持久化触发（最小）：调度器可在每批次后 best-effort 保存检查点；CLI `team run` 支持 `SISYPHUS_TEAM_CHECKPOINT_OUT=<path>` 启用周期性+最终检查点保存，支持 `SISYPHUS_TEAM_INTERRUPT_AFTER_BATCHES=<N>` 进行确定性中断注入（S5 测试/恢复切片），并支持 `SISYPHUS_TEAM_IMPACT_POLICY=observe|record_affected|fail_affected_pending`（CLI 默认 `record_affected`）。
  - 测试：checkpoint round-trip、`resume_state_filters_workspace_impact_failures`、`test_resume_plan_from_checkpoint_continues_remaining_tasks`、`test_interrupt_recovery_via_persisted_checkpoint_file`、`test_periodic_checkpoint_saved_during_execution`、`test_interrupt_after_batches_returns_partial_report_and_resumable_checkpoint`、`test_resume_is_idempotent_when_all_tasks_already_completed` 通过（计数以实跑输出为准）。
  - 证据：`docs/benchmarks/parallel-coding/2026-11-19-s5-interrupt-injection-checkpoint-slice/`（含 `summary.md`/`summary.json`/`env.txt`/`run.log`，命令与结果以归档为准；另有前序子集 `2026-11-19-s5-interrupt-recovery-checkpoint-slice/`）。
  - 待深化：真实进程/任务级中断注入、幂等副作用端到端验证、更完整场景化证据。
- [x] **M2.1 编排后验证触发与定向重试（CLI 最小集成，持续）**：
  - `sisyphus-cli::delegate`：`team run` 显式分解+执行计划；在编排完成后汇总上下文记录的文件类工作区变更路径与影响记录路径，调用 `post_edit::verify_after_edit` 执行配置驱动的 post-edit 验证（未配置时跳过）；输出验证摘要。
  - verify-fix + 定向重试闭环（最小，多轮）：支持 `SISYPHUS_TEAM_VERIFY_FIX_CMD` 修复钩子、`SISYPHUS_TEAM_VERIFY_RETRY`（workspace-impact 任务定向重试）与 `SISYPHUS_TEAM_VERIFY_FIX_ROUNDS`（默认 1）组合；每轮可执行修复命令→复验，并在仍失败时对 impact-failed 任务执行一次恢复重试→复验，直至通过或轮次耗尽。
  - 待深化：与 LLM 驱动 verify-fix/诊断修复循环的自动闭环、基于失败根因的更细粒度任务选择/重规划、可审计证据归档与跨路径（非 CLI/bridge）一致接线。

> 后续工作按 Phase M0→M4 推进；每项完成需附可复现验证证据并在对应计划/TODO 中登记。
