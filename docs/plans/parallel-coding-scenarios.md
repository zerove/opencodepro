# 并行编码代理场景集（M0.1 草案）

> 日期：2026-11-18  
> 目标：为“自动化并行开发 AI 编码代理”建立可复跑、可度量的代表性场景，支撑 M0–M2 的实现与验收。  
> 约束：遵循 ADR-021（计数/结果以实跑输出为准）；不伪造证据；单机场景优先。

## 场景列表（首批）

| ID | 场景 | 并行形态 | 预期能力验证点 | 成功判定（示例） | 证据/指标 |
|---|---|---|---|---|---|
| S1 | 多文件独立修复 | 多 agent 并行写不同文件 | 任务分解、无冲突并行、独立验证 | 目标文件均满足验收命令（如 `cargo test`/lint 子集），无回归 | 成功率、耗时、并行度、每任务 token |
| S2 | 同文件冲突写入 | 多 agent 可能写同一路径 | 冲突检测、告警与串行化/协调，无静默覆盖 | 最终文件状态一致且可解释；冲突被记录（`ConflictDetected`/波次） | 冲突检出率、误串行化率、最终正确性 |
| S3 | 读多写少（调研+落码） | 并行读取/分析，少量写入 | 读写冲突识别、上下文汇聚、写入阶段安全 | 写入不覆盖调研输入假设；验证通过 | 读写冲突次数、重试次数、验证通过率 |
| S4 | 编辑后验证-修复循环（并行子任务） | 子任务失败触发定向修复 | post-edit/verify-fix 与并行编排衔接、失败可恢复 | 失败→修复→通过的闭环记录完整 | 修复率、回归率、额外耗时 |
| S5 | 长任务中断与恢复 | 并行执行中断/重启 | 状态持久化/恢复、任务幂等与续跑策略 | 恢复后可继续并最终达到一致结果或可解释终止 | 恢复成功率、重复执行副作用 |

### 已落地确定性子场景与证据（M0.3 扩展）

| 场景 | 子场景（确定性） | 代表用例/命令 | 证据路径（归档） | 备注 |
|---|---|---|---|---|
| S1 | 无冲突并行基线 | `cargo test -j 1 -p sisyphus-tools-agent --lib test_execute_parallel_tasks` | `docs/benchmarks/parallel-coding/2026-11-18-s1-parallel-no-conflict/` | `conflict_stats` 全 0 断言（计数以实跑输出为准） |
| S2 | 同文件写冲突检测与串行化 | `cargo test -j 1 -p sisyphus-tools-agent --lib test_parallel_write_conflict_is_detected_and_serialized` | `docs/benchmarks/parallel-coding/2026-11-18-s2-write-conflict-serialization/` | write-write 冲突与串行化波次可观测 |
| S3 | 并行读写冲突检测（读多写少切片） | `cargo test -j 1 -p sisyphus-tools-agent --lib test_parallel_read_write_conflict_is_detected` | `docs/benchmarks/parallel-coding/2026-11-18-s3-read-write-conflict-detected/` | read-write 冲突被检测并记录（统计/事件）；执行并行性/串行化策略以当前实现为准 |
| S4 | 失败→重试/回退恢复（任务级委托策略 + CLI 定向重试最小切片） | `bash -lc 'cargo test -j 1 -p sisyphus-tools-agent --lib test_delegation_retry_succeeds_on_second_attempt && cargo test -j 1 -p sisyphus-tools-agent --lib test_delegation_fallback_agent_takes_over'` | `docs/benchmarks/parallel-coding/2026-11-18-s4-delegation-retry-fallback/` | 覆盖“首次失败后重试成功”和“主代理失败后 fallback 接管”；CLI `team run` 增加验证失败后的 workspace-impact 任务定向重试（`SISYPHUS_TEAM_VERIFY_RETRY=1`，最小实现）。完整 post-edit/verify-fix 并行编排闭环与场景化证据仍待深化（M2.1） |
| S4 | 工作区影响自动定向重跑（调度层最小闭环） | `bash -lc 'cargo test -j 1 -p sisyphus-tools-agent --lib test_impact_retry_recovers_failed_pending_task && cargo test -j 1 -p sisyphus-tools-agent --lib test_impact_retry_disabled_with_zero_rounds && cargo test -j 1 -p sisyphus-tools-agent --lib test_impact_retry_skips_non_impact_failures'` | `docs/benchmarks/parallel-coding/2026-11-19-s4-impact-auto-retry/` | `FailAffectedPending` 标记的待执行任务在 `execute_plan_with_impact_retry` 中仅重跑受影响项（`ImpactRetry` 事件、已完成任务不重复调度）；`0` 轮保持旧行为；非影响类失败不触发重跑。CLI 对应 `SISYPHUS_TEAM_IMPACT_RETRY_ROUNDS`（默认 0）。 |
| S5 | 长任务中断与恢复（确定性切片） | `bash -lc 'cargo test -j 1 -p sisyphus-tools-agent --lib team::checkpoint::tests::checkpoint_round_trip_preserves_state && cargo test -j 1 -p sisyphus-tools-agent --lib test_resume_plan_from_checkpoint_continues_remaining_tasks && cargo test -j 1 -p sisyphus-tools-agent --lib test_interrupt_recovery_via_persisted_checkpoint_file && cargo test -j 1 -p sisyphus-tools-agent --lib test_periodic_checkpoint_saved_during_execution && cargo test -j 1 -p sisyphus-tools-agent --lib test_interrupt_after_batches_returns_partial_report_and_resumable_checkpoint'` | `docs/benchmarks/parallel-coding/2026-11-19-s5-interrupt-injection-checkpoint-slice/`（另有 `2026-11-19-s5-interrupt-recovery-checkpoint-slice/` 前序子集） | 已落地检查点序列化/加载、上下文恢复、持久化文件中断点恢复、周期性检查点保存，以及调度器级确定性中断注入（`interrupt_after_batches`，返回部分报告+可恢复检查点）与续跑测试；CLI `team run` 支持 `SISYPHUS_TEAM_CHECKPOINT_OUT` 周期性+最终检查点保存。**待深化/部分 BLOCKED**：真实进程/任务级中断注入、幂等副作用与更完整端到端场景化证据。 |

### 可观测性/接口接线证据（M1.3）

| 能力 | 代表用例/命令 | 证据路径（归档） | 备注 |
|---|---|---|---|
| Bridge 团队执行端点 + SSE 广播 + `?goal_id=` 过滤 | `bash -lc 'cargo test -j 1 -p sisyphus-bridge --lib team_execute_handler_broadcasts_scoped_events && cargo test -j 1 -p sisyphus-bridge --lib forward_team_events_injects_scope_dimensions && cargo test -j 1 -p sisyphus-bridge --lib goal_filter && cargo test -j 1 -p sisyphus-bridge --lib every_route_is_documented'` | `docs/benchmarks/parallel-coding/2026-11-19-m13-team-sse-endpoint/` | `POST /api/team/execute` 执行显式计划并广播 `team_*` 事件（注入 `goal_id`/`project_id`/`trace_id`）；OpenAPI 路由收录断言通过 |
| SSE 断线重连事件重放（`Last-Event-ID`） | `bash -lc 'cargo test -j 1 -p sisyphus-bridge --lib sse_replay && cargo test -j 1 -p sisyphus-bridge --lib replay && cargo test -j 1 -p sisyphus-bridge --lib stale_cursor'` | `docs/benchmarks/parallel-coding/2026-11-19-m13-sse-replay/` | 发布侧单调 ID + 有界重放缓冲；断点补发/header 优先/维度过滤/重启回退；`/api/events/health` 暴露重放窗口指标 |

## 执行前提与环境分级

- **L0/L1（确定性/本地）**：不依赖外部 LLM 或仅用 mock/本地可控依赖；适合 CI 与快速回归。
- **L2（本地真实模型）**：如本机 Ollama 等；记录模型/参数与硬件环境。
- **L3（云端/外部依赖）**：需凭据与网络；缺失时标记 `BLOCKED`，不得伪造结果。

## 指标口径（M0.2 初稿）

- **正确性**：场景通过/失败/阻塞；最终验证命令结果；是否存在静默覆盖或未检测冲突。
- **并行与协调**：并行任务数、冲突检测次数与类型（write-write/read-write）、串行化波次数。
- **效率**：端到端 wall time、各阶段耗时、（可得时）token/调用次数。
- **稳定性**：重试次数、恢复成功率、重复运行一致性（波动需注明环境因素）。

## 可采集字段映射（M0.2）

> 原则：字段值必须来自实跑输出/归档产物，不伪造；场景特定指标由对应 harness/test/报告导出，本模板仅承载通用执行证据。

| 指标维度 | 建议字段（`summary.json` / 报告） | 来源与说明 |
|---|---|---|
| 场景标识与环境 | `scenario`, `timestamp_utc`, `git.head`, `git.dirty_files`, `environment.*` | `scripts/parallel-coding-bench-template.sh` 从当前仓库与环境快照采集 |
| 执行结果 | `execution.status` (SKIPPED/COMPLETED), `execution.exit_code`, `execution.command`, `execution.log` | 模板实跑记录；`exit_code==0` 不等同于业务验收通过，需结合日志/断言结论 |
| 端到端耗时 | `execution.elapsed_ms`, `metrics.wall_time_ms` | 模板计时（毫秒） |
| 正确性摘要 | `metrics.scenario_pass`（命令退出码是否为 0）、`metrics.command_exit_code` | 模板派生；复杂验收以场景专用断言/报告为准 |
| 并行与冲突（team 场景） | `TeamReport.conflict_stats.detected/write_write/read_write/serialized_waves/conflict_batches` | `sisyphus-tools-agent::team` 执行报告字段；可由测试断言或运行时序列化产物采集（S2 已有确定性用例与证据） |
| 冲突事件 | `TeamEvent::ConflictDetected { kind, key, task_ids }` | 调度层事件流；用于解释串行化原因与冲突归因 |
| token/调用/重试等 | 场景专用字段（如 LLM/代理度量、重试计数） | 由具体执行器/评测 harness 提供；缺失时在结论中标注未采集或 BLOCKED |

### `summary.json` schema（模板输出，v1）

- `schema_version`: 固定为 `1`
- `scenario`, `timestamp_utc`, `output_dir`, `note`
- `git`: `{ head, dirty_files }`
- `environment`: `{ host, kernel, pwd, rustc, cargo, env_file }`
- `execution`: `{ status, command[], exit_code|null, elapsed_ms|null, log|null }`
- `metrics`: `{ scenario_pass|null, wall_time_ms|null, command_exit_code|null }`

## 证据归档建议

- 路径：`docs/benchmarks/parallel-coding/<YYYY-MM-DD>-<scenario-or-batch>/`
- 内容：`summary.md`（命令、环境、结论）、`summary.json`（机器可读字段）、`env.txt`、原始日志 `run.log`、复现步骤；若为对比实验注明基线 commit/配置。
- 结论表述：使用“实跑 N 次，命令输出为 …”等可追溯描述，避免固定计数写死在结论中（除归档快照明确标注来源）。

## 下一步（与路线图对应）

- M0.2：将上述指标映射到可采集字段与脚本输出格式。
- M0.3：提供最小可跑基准脚本/模板（优先 S1/S2 确定性子场景）。
- M1.x：随冲突推断、协调原语与可观测性完善，逐步覆盖 S2–S5（S5 最小恢复切片已落地，端到端中断注入与持久化触发/证据待深化）。
