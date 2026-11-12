# 自动化开发能力开发计划（2026-11）

> 依据：`docs/plans/p1-p2-roadmap.md`、`docs/plans/single-node-full-functionality-audit.md`、
> `docs/runbooks/single-node-automation-acceptance.md`、`docs/comparison/opencode-jcode-sisyphus-comparison-2026-11.md`，
> 以及本轮差距分析与 OpenCode/JCode 可复刻能力评估。
>
> 原则：**引擎优先、单机优先、选择性产品化**；所有新能力必须复用 SOP/DAG、bridge（gRPC+REST+SSE）、
> CLI/TUI、三端 SDK、观测与门禁底座；不引入并行体系；不伪造证据。
>
> 边界：`cluster.control_plane.enabled=false`；ADR-020（REST 写入=否）不变；P2-D/ADR-019 生产准入保持外部后置。

## 1. 目标

把当前“能编排、能观测、能集成”的平台骨架，推进为“**跑得稳、跑得准、说得清、上手快**”的自动化开发工具：

| 维度 | 目标 | 判定方式 |
|---|---|---|
| 跑得准（闭环） | 真实任务可完成“目标 → 执行 → 失败 → 修复 → 验证”闭环 | L3 验收出现 PASS 证据（≥3 个场景） |
| 跑得稳（性能） | 关键路径尾延迟可归因、可回归 | p50/p95 基线与回归门禁上线 |
| 说得清（质量） | 任务成功率/修复率/回归率/成本可量化 | 指标定义 + 首轮数据 + 趋势 |
| 上手快（产品化） | 新用户可在 5 分钟内完成首次真实调用 | provider 引导 + 验收记录 |
| 可审计（治理） | 危险操作可审批、执行可回放 | 审批审计日志 + 会话回放页 |

## 2. 差距 → 任务映射

| 差距（来源） | 对应任务 | 阶段 |
|---|---|---|
| L3 真实闭环无成功证据 | T1.4、T1.1、T1.2 | 阶段 1 |
| 编辑后无自动验证/回归 | T1.1、T1.2 | 阶段 1 |
| 执行过程不可见（步骤/耗时/token） | T1.3 | 阶段 1 |
| 无对话式 Agent 循环（对比 OpenCode） | T2.1 | 阶段 2 |
| 权限规则弱、审批无审计 | T2.2 | 阶段 2 |
| 会话不可回放/分享（离线版） | T2.3 | 阶段 2 |
| 自定义命令形态缺失 | T3.1 | 阶段 3 |
| Provider 接入无引导 | T3.2 | 阶段 3 |
| 启动尾延迟未归因、TUI 首帧未测 | T3.3、T0.2 | 阶段 0/3 |
| 无质量指标与性能回归门禁 | T4.1、T4.2、T4.3 | 阶段 4 |
| 并行编排无冲突感知 | T3.4（可选） | 阶段 3 |
| 统一 trace 关联缺失 | T4.4（先行设计） | 阶段 4 |

## 3. 阶段与里程碑

| 阶段 | 周期（建议） | 里程碑（Exit Criteria） |
|---|---|---|
| 阶段 0 基线与门禁 | 1 周 | 提交同步；性能口径含 p50/p95；L3 环境清单就绪 |
| 阶段 1 自动化闭环 | 2–3 周 | ≥1 个真实任务闭环 + ≥3 场景 L3 证据；编辑后自动验证可用 |
| 阶段 2 交互与治理 | 3 周 | TUI 对话式执行 MVP；审批审计；会话回放页 |
| 阶段 3 生态与性能 | 2–3 周 | 自定义命令、Provider 引导、启动分段归因 |
| 阶段 4 质量与回归 | 持续 | 四项质量指标 + 性能回归门禁 + evals 子集入 CI |

> 周期为单机、单人力估算，不含外部 LLM/凭据等待时间；遇阻塞项按 §7 降级推进。

## 4. 任务分解

### 阶段 0：基线与门禁（1 周）

| ID | 任务 | 说明 | 验收/证据 | 工作量 |
|---|---|---|---|---|
| T0.1 | 提交与仓库同步 | 清理未推送提交；统一 `origin/master == HEAD` 检查 | `git rev-parse HEAD origin/master` 一致；工作区干净 | S |
| T0.2 | 性能采样口径升级 | `single-node-bench-sample.sh` 增加 p50/p95、构建类型、时间戳；固定 `N>=10` 分批规则 | `--json` 输出含 `p50/p95/build/timestamp`；文档记录口径 | S–M |
| T0.3 | L3 环境准备清单 | 固化 LLM/凭据/MCP 所需配置项、最小验证命令与失败判定 | `docs/runbooks/single-node-automation-acceptance.md` 增补清单；`sisyphus provider test` 记录 | S |
| T0.4 | 验收证据自动化 | 证据目录生成后可一键填充 L0/L1（脚本串联）；纳入 CI artifact | `acceptance-evidence.sh` 支持 `--run-l0`；CI 上传 artifact | M |

### 阶段 1：自动化闭环（2–3 周）

| ID | 任务 | 说明 | 验收/证据 | 工作量 |
|---|---|---|---|---|
| T1.1 | 编辑后自动验证（post-edit hook） | `write/edit/apply_patch` 后按配置触发 LSP diagnostics 或 `verify_cmd`；结果写入节点输出并可阻断“成功” | 故意引入类型错误 → 自动失败并输出诊断摘要；修复后通过；单测 + 实跑证据 | M |
| T1.2 | 验证-修复循环 | 基于 `scenario_diagnose_fix_verify_template`，失败时携带诊断上下文重试（有界次数）；记录每次尝试 | 一次真实“失败→修复→通过”记录（含耗时/次数）；无 LLM 时走确定性路径 | M |
| T1.3 | 执行进度事件 | `/api/execute/goal/{id}/stream` 增加 `step_started/step_finished/tool_call/token_usage` 事件（向后兼容） | SSE 事件契约文档 + 消费端展示；旧客户端不受影响 | M |
| T1.4 | L3 真实闭环证据 | 在具备 LLM/凭据环境执行 ≥3 个场景（诊断修复、质量门、功能新增），归档请求/响应/trace/耗时/成本 | `docs/benchmarks/acceptance/<date>-l3/` 完整证据；`summary.md` 判定 | M |
| T1.4 状态（执行） | DONE（本地真实模型）/ 云端外部后置 | 用本机 ollama `qwen2.5-coder:3b` 跑三场景 × 3 轮 = 9/9 PASS（**旧判定口径**：只判闸门；复核发现 s3 的 PASS 含「整文件覆盖删掉 `pub fn add`」的静默破坏。判定加严（必须保留既有公开 API）+ 引擎补回机制后实测 **7/9**，见证据 §4.4b；每场景判定由外部 `cargo` 命令决定；归档请求/响应/token/耗时/trace（`phase1/runs/`）；过程中新增 `dev --verify-cmd` 客观闸门并修复 14 个真实缺陷（dev 闭环），MCP 侧另修 4 个互操作缺陷；**云端 provider 首次真实调用仍未验证**（无有效云端凭据）；多文件重构已加 s4 探索场景（本地 3B FAIL，用于覆盖缺口/发现缺陷，opt-in）；**MCP 已覆盖**（独立实现的 MCP 服务器 + CLI/引擎两级 17/17 PASS（stdio + HTTP 两种传输），并修掉 4 个真实互操作缺陷） | 证据 `docs/benchmarks/acceptance/2026-11-12/phase1/t1-4-l3-closed-loop.md`；复现 `bash scripts/l3-run-scenarios.sh s1 s2 s3` |

**阶段 1 完成标准**：至少 1 个真实任务形成“改文件 → 验证失败 → 修复 → 验证通过”的完整记录；其余场景有可解释结果或明确 `BLOCKED`。

### 阶段 2：交互与治理（3 周）

| ID | 任务 | 说明 | 验收/证据 | 工作量 |
|---|---|---|---|---|
| T2.1 | TUI 对话式执行 MVP | 输入目标 → 流式输出 → 工具调用卡片 → 批准/拒绝/中断 → 继续/恢复；复用 SSE 与 `StreamEvent` | 一次会话含 ≥2 次工具调用、1 次审批、1 次中断恢复；录屏/日志证据 | L |
| T2.2 | 权限规则与审批审计 | `CliPermission` 支持 glob；危险模式默认 `ask`；TUI 弹窗审批；决策写入会话/审计日志 | 危险命令触发审批；拒绝后不执行；审计可查询 | M |
| T2.3 | 会话回放页 | 增强 `session export html`：时间轴、工具调用折叠、diff、认知 trace 关联；离线可打开 | 导出页可离线还原一次完整执行；快照测试 | S–M |
| T2.4 | 错误提示有效性抽样 | 针对 `hint` 场景设计“按提示操作后恢复”的抽样检查 | 抽样结果记录（恢复/未恢复/无法判定） | S |

### 阶段 3：生态与性能（2–3 周）

| ID | 任务 | 说明 | 验收/证据 | 工作量 |
|---|---|---|---|---|
| T3.1 | 自定义开发命令 | `.sisyphus/commands/*.md` 参数化映射到 SOP 模板/技能；CLI/TUI 可发现 | 新增命令文件即可执行；文档 + 单测 | S–M |
| T3.2 | Provider 交互式引导 | `provider login` 选择→输入→连通性测试→写配置；失败可解释 | 新用户 5 分钟内完成首次真实调用（有凭据环境；本地真实 provider 已可达成） | M |
| T3.3 | 启动分段归因与优化 | bridge 就绪分段计时（配置/sqlite/embedding/端口）；TUI 首帧 PTY 测量；按需懒加载 | 分段耗时表 + 优化前后对比；TUI 首帧基线文档 | M |
| T3.4 | 并行冲突感知（可选） | `team run --strategy parallel` 检测同文件并发编辑，串行化或告警 | 冲突场景产生告警而非静默覆盖 | M–L |
| T3.4 状态（执行） | DONE | 新增 `crates/sisyphus-tools-agent/src/team/conflict.rs`：`detect_conflicts`（写写 / 读写冲突，写键来源含显式 `write_keys` 与兼容旧 `args["_write_keys"]`）+ `partition_by_write_conflicts`（按写冲突拆执行波次）；调度器在每批就绪任务上检测 → 发 `TeamEvent::ConflictDetected` 告警 + `tracing::warn!` → 冲突任务分波次串行化（波次内仍并行），避免静默覆盖；既有 `TeamContext::detect_write_conflicts` 改为复用新模块。证据：6 个新单测（5 模块级 + 1 调度器级：两任务写同一 key → 产生 1 个 `write_write` 告警且两任务均执行成功）；`cargo test -p sisyphus-tools-agent --lib` 104 passed |

### 阶段 4：质量指标与回归（持续）

| ID | 任务 | 说明 | 验收/证据 | 工作量 |
|---|---|---|---|---|
| T4.1 | 质量指标定义与采集 | 成功率、修复率、回归率、单任务耗时/token 成本；从执行记录汇总 | 指标文档 + 首轮数据（脚本可复跑） | M |
| T4.1 状态（执行） | DONE | 指标口径与采集链路完成；受控 fixture 首轮样本（成功率 0.667 / 修复率 0.5）之后，已积累 **7 条真实任务样本**：4 条仓库 `verify` 任务 + 3 条 L3 闭环（`dev --record`，本地 ollama `qwen2.5-coder:3b`，s2 首轮失败→修复 1 次通过、s1/s3 一次通过，均带 provider 上报的真实 token 用量）。聚合结果（含 1 条失败样本的真实一轮）：成功率 0.857 / 修复率 0.667 / 回归率 0.0 / 平均 65.6s / 平均 1893.7 token（3 条 dev 记录），快照见 `docs/benchmarks/quality-metrics/2026-11-12-2e8bf69.json`；记录生成方式见 `docs/benchmarks/quality-metrics/records/README.md` |
| T4.2 | 性能回归门禁 | 定时/触发式采样，对比基线阈值，超阈值告警 | CI job 或 `make perf-regression`；阈值与例外记录 | M |
| T4.2 状态（执行） | DONE | `scripts/perf-regression-check.sh <mode>` + `make perf-regression{,-bridge}`；基线 `docs/benchmarks/perf-regression-baseline-2026-11-10.json`；实跑 `perf-regression-check.sh cli` rc=0（3 CLI 指标 p50 0.0097-0.0118s < 0.05s） |
| T3.3 启动分段归因与优化 | DONE | bridge 启动分段（`startup_phase_*_ms` + `scripts/measure-bridge-startup.sh`，实跑 end_to_end_ready_ms≈1530ms、phases bridge≈59ms/http≈68ms/grpc≈67ms）；TUI 首帧 PTY 测量（`scripts/measure-tui-first-frame.sh` + `make perf-tui`，实跑 3 采样 p50≈14.3ms，产物 `docs/benchmarks/tui-first-frame/2026-11-11-75b9959.json`）；按需懒加载未做（记入 follow-up） |
| T4.3 | evals 子集入 CI | 选取稳定、无外部依赖的 cases（metis/momus/security 等） | CI 通过；用例清单与运行时长记录 | M |
| T4.3 状态（执行） | DONE | `scripts/evals-ci-run.sh [cases]` + eval-runner `--cases` 过滤（`agent/id`，支持 `--cases X`/`--cases=X`）；实跑子集 `metis/metis-001,momus/momus-plan-002` rc=0（2/2 PASS, final=1.0, dur≈1.5s）；证据 `docs/benchmarks/evals-ci/2026-11-11-5a23f9a.json`；默认子集为 criteria 全 keyword/regex 的稳定 case（mock 语义断言失败属预期） |
| T4.4 | 统一 trace 关联 | 入口生成 trace id，贯穿 SOP/工具/LLM 日志与响应头；先行设计文档 | 一次请求可查到全部子步骤；设计评审记录 | M–L |
| T4.4 状态（执行） | DONE | 设计：`docs/plans/t4-4-trace-correlation-design.md`；实施步骤 1-7 全部落地：trace id 工具（types，6 单测）、HTTP 入口中间件（extensions+span+响应头，3 单测）、gRPC 拦截器 + 鉴权组合（3 单测）、SSE 负载 `trace_id` 维度键（1 单测）、CLI metadata 注入、gRPC 关键方法 span 记录 trace_id、`book/src/sse-events.md` + OpenAPI 契约。实跑 bridge：带 `x-trace-id: trace-live-check` → 原样回写；不带 → 生成 `trace-77f68a…`；非法值 → 替换。测试独立成文件以避免 OpenAPI 路由覆盖测试误扫 |
| T4.5 | 长窗口稳定性 | 连续执行/资源泄漏（内存、句柄、DB）监测 | 数小时运行记录与结论（不伪造连续样本） | M |
| T4.5 状态（执行） | DONE | 监测脚本 + nightly 接线；4 小时窗口 2881 采样：探活 100%，RSS 斜率 -0.83 KB/min、FD/线程不增反降 → 无泄漏迹象；证据 `docs/benchmarks/long-run/2026-11-11-31d14c3/` |

## 5. 质量与性能指标（初始目标，待基线校准）

| 指标 | 定义 | 初始目标 | 采集方式 |
|---|---|---|---|
| L3 场景成功 | 真实任务达到验收标准 | 首批 ≥3 场景有结论（PASS 或可解释 BLOCKED） | 验收证据目录 |
| 任务成功率 | 成功任务 / 总任务 | 建立基线后设定 | T4.1 汇总 |
| 修复率 | 失败后自动修复通过 / 失败任务 | 建立基线后设定 | T1.2 记录 |
| 回归率 | 修复引入新失败 / 修复任务 | 建立基线后设定 | 验证前后对比 |
| 单任务成本 | 耗时、token、工具调用次数 | 记录中位数与 p95 | 执行记录 |
| Bridge 就绪 | 启动 → `/health` 可达 | debug p95 ≤ 1500 ms（分批 N≥10） | T0.2 |
| 只读端点 | `/health`、`/api/sops` | p95 ≤ 10 ms（本地回环） | T0.2 |
| CLI 启动 | `--help`（热） | p95 ≤ 30 ms（debug，参考） | T0.2 |
| 提示有效性 | 按 `hint` 操作后恢复比例 | 抽样记录，逐步提升 | T2.4 |

> 目标值为工程目标，不是既有证据；每次发布前更新基线并复核。

## 6. 验收与证据要求（每个任务）

1. **代码**：通过 `cargo fmt --all --check`、`cargo clippy`、`cargo check-all`（涉及 bridge/sandbox 时单独编译）。
2. **测试**：单元测试或集成测试；脚本类任务提供 `bash -n` 与样例运行。
3. **文档**：配置变更同步 `config.example.toml` 与 `book/`；行为变更同步 runbook。
4. **证据**：可复现命令 + 输出摘要；外部依赖缺失时标记 `BLOCKED`，不得伪造成功。
5. **提交**：中文 conventional commit；一个任务一个提交（或可独立回滚的小批次）。

## 7. 风险与应对

| 风险 | 影响 | 应对 |
|---|---|---|
| LLM/凭据不可用 | T1.4 已用本地真实模型解阻；T3.2 已用本地真实 provider 解阻 | 先做确定性路径（T1.1/T1.2 本地部分）；T1.4/T3.2 用本地 OpenAI 兼容服务做端到端；仅「云端 provider 首次真实调用」外部后置 |
| 网络推送失败 | 提交积压 | 本地小批次提交 + 定期重试；必要时生成 `git bundle` 备份 |
| 长命令/采样被中断 | 性能数据不完整 | 分批采样并标注聚合方式；不伪造连续样本 |
| 范围膨胀 | 交付延迟 | 每任务必须映射到 §2 差距表；阶段 Exit Criteria 不满足不进入下一阶段 |
| TUI 改动回归 | 交互不可用 | 先加测试与快照；保留 CLI 等价路径 |
| 性能优化引入不稳定 | 尾延迟反弹 | 优化前后同口径对比；回归门禁先行 |

## 8. 明确不做

- 不复刻 Desktop/Tauri 客户端（`ide/tauri` 已归档）。
- 不提供云端分享服务（离线 HTML 回放替代）。
- 不复刻 Swarm 自治协作（仅做可审计的并行冲突感知）。
- 不追求插件市场/生态规模（先补 hook 语义）。
- 不启用控制面生产写入/生产准入（P2-D 外部后置）。

## 9. 排期建议（单人力）

```text
第 1 周       阶段 0：T0.1–T0.4
第 2–4 周     阶段 1：T1.1 → T1.2 → T1.3 → T1.4（T1.4 视环境）
第 5–7 周     阶段 2：T2.1 → T2.2 → T2.3 → T2.4
第 8–10 周    阶段 3：T3.1 → T3.2 → T3.3 →（可选 T3.4）
第 11 周起    阶段 4：T4.1–T4.5 持续，随每阶段插入
```

每阶段结束产出：`docs/benchmarks/acceptance/<date>-<phase>/` 证据目录 + 本计划状态更新。

## 9.1 阶段验收证据目录

每阶段的验收证据（实际执行命令 + 观察值 + 复现方式）汇总在：

| 阶段 | 证据目录 |
|---|---|
| 阶段 0：基线与验收自动化 | `docs/benchmarks/acceptance/2026-11-12/phase0/summary.md` |
| 阶段 1：编辑后验证与闭环 | `docs/benchmarks/acceptance/2026-11-12/phase1/summary.md` |
| 阶段 2：交互与治理 | `docs/benchmarks/acceptance/2026-11-12/phase2/summary.md` |
| 阶段 3：生态与性能 | `docs/benchmarks/acceptance/2026-11-12/phase3/summary.md` |
| 阶段 4：质量指标与回归 | `docs/benchmarks/acceptance/2026-11-12/phase4/summary.md` |

细粒度产物（采样 JSON / 报告 / 快照）仍在 `docs/benchmarks/` 下的对应目录。

## 10. 关联文档

- 路线：`docs/plans/p1-p2-roadmap.md`
- 单机审计：`docs/plans/single-node-full-functionality-audit.md`
- 验收手册：`docs/runbooks/single-node-automation-acceptance.md`
- 验收矩阵：`docs/plans/single-node-acceptance-matrix.md`
- 性能基线：`docs/plans/single-node-performance-baseline.md`、`docs/benchmarks/single-node-runtime-baseline-2026-11-10.md`
- 对标分析：`docs/comparison/opencode-jcode-sisyphus-comparison-2026-11.md`
- 场景手册：`docs/runbooks/single-node-dev-scenarios.md`

## 11. 执行进展（滚动更新）

| 任务 | 状态 | 证据 |
|---|---|---|
| T0.1 提交与仓库同步 | DONE | `git rev-parse HEAD origin/master` 一致；工作区干净 |
| T0.2 性能采样口径升级 | DONE | `scripts/single-node-bench-sample.sh` 输出 `n/min/p50/p95/avg/max` + `timestamp/build`；`N=3 ... cli --json` 经 `python3 -m json.tool` 校验 |
| T0.3 L3 环境准备清单 | DONE | `docs/runbooks/single-node-automation-acceptance.md` §7.1 清单；§8 更新为分位数口径 |
| T0.4 验收证据自动化 | DONE | `scripts/single-node-acceptance-evidence.sh --run-l0` 生成 `l0-static.log`（模板校验 + 脚本语法）；`L0_FULL_SMOKE=1` 追加契约冒烟 |
| T1.1 编辑后自动验证 | DONE | 新增 `crates/sisyphus-cli/src/post_edit.rs`；11 个单测；实跑：`auto` 模式捕获 `E0308` 类型错误、计时/超时/严格模式行为符合预期；文档见 `book/src/crates/cli.md` |
| T1.2 验证-修复循环 | DONE | `verify_fix_loop_with`（`sisyphus-tools-llm-tools/src/diagnose.rs`）+ `sisyphus verify` 命令；5 个循环单测 + 3 个输出格式单测；实跑证据：临时 crate 第 1 轮 2 warning（`--max-warnings 0` 未通过）→ fix → 第 2 轮 0 warning（PASS，188ms）；不可修复错误在 2 轮后退出非零 |
| T1.3 执行进度事件 | DONE | `ExecutionEvent` 新增 `ToolCall`/`TokenUsage`（共 10 变体）；`/api/tools/execute` 成对广播 `tool_call`（started → completed/failed，含 duration/success/error/project_id）；SSE 映射、TUI 渲染、OpenAPI 与 `book/src/sse-events.md` 已同步；bridge 侧 2 个广播单测 + 变体映射守卫 + TUI 3 个格式单测 |
| T2.3 会话回放页 | DONE | `render_session_html` + 渲染 helper（时间轴/工具折叠/diff 着色/长消息折叠/trace 徽标/CSS）；13 个渲染单测；实跑 `session export --format html` 后校验：`#1/2` 序号、`&lt;b&gt;` 转义、`<details class="tool">` 折叠、`+`/`-`/`@@` 着色、无外部资源；文档见 `book/src/crates/cli.md` 与 `docs/runbooks/single-node-sessions-and-tasks.md` §2.1 |
| T3.1 自定义开发命令 | DONE | 新增 `custom_command` 模块（用户级 + 项目级目录、同名项目覆盖、`{{arg}}` 渲染、必填/默认校验）+ `sisyphus command list/show/run`；Shell 命令 `--exec` 复用 `require_tool` 审批与审计；TUI `/commands` 可发现；11 个 loader/render 单测 + 4 个输出/参数单测 + 2 个审批单测；实跑：提示词渲染、`--exec` 拒绝/批准/失败退出码、审计三条记录均符合预期 |
| T2.4 错误提示有效性抽样 | DONE | 新增 `sisyphus-cli::hints`：`HINTS` 提示目录 + `suggest_hint`（多 pattern 命中取最长）；`sample_hints` 抽样聚合（初始判定 Indeterminate）+ `sample_hint_outcome`/`HintSample::with_action` 回填；TUI SOP 失败路径改用统一 `suggest_hint`；7 个 hint 单测 |

| T1.4 L3 真实闭环 | DONE（本地真实模型）/ 云端外部后置 | 三场景 × 3 轮 = 9/9 PASS（本机 ollama `qwen2.5-coder:3b`，CPU，**旧判定口径**；判定加严「必须保留既有公开 API」+ 引擎补回被遗漏的公开 API 后为 **7/9**，见证据 §4.4b）；每场景判定由外部 `cargo` 命令决定；新增 `dev --verify-cmd` 客观闸门（原实现由模型自选 verification，实测报 SUCCESS 而 clippy 仍失败）；三场景完整请求/响应/token/耗时/trace 归档于 `docs/benchmarks/acceptance/2026-11-12/phase1/runs/`；过程中修复 14 个 dev 闭环缺陷 + 4 个 MCP 互操作缺陷（`phase1/l3-mcp-scenario.md`）；**云端 provider 首次真实调用仍未验证**；多文件重构由 s4 探索场景覆盖（3B FAIL，见证据 §4.5）；**MCP 已覆盖**：独立实现的 MCP 服务器（stdio `scripts/l3-mcp-server.py` + HTTP `scripts/l3-mcp-http-server.py`）+ CLI/引擎两级 **17/17 PASS**（stdio + HTTP 两种传输），并修掉 4 个真实互操作缺陷（线上字段 snake_case、通知同步等响应、`mcp call` 启动 panic、自家 server 回复通知），证据 `phase1/l3-mcp-scenario.md` |
| T2.1 TUI 对话式执行 MVP | DONE（脚本化验收） | 四切片落地：S1 `Developer` interrupt + `DevOverallStatus::Cancelled`；S2 `DevApprover` 审批闸；S3 TUI 接缝（`BgOp::DevAsk`/`DevRunHandle`/审批卡片/y·n·Esc 键位/Esc→Resume 恢复入口）；S4 验收测试 `crates/sisyphus-tools-agent/tests/t2_1_mvp_acceptance.rs`（2 次工具调用 / 2 次审批 / 1 次中断 + 恢复）；`sisyphus-cli` 120 单测、`tools-agent` 98 + 1 验收 |
| T2.2 权限规则与审批审计 | DONE | glob 规则 + 危险默认 `ask` + JSONL 审计 + `permission check/audit`；TUI 审批弹窗补齐（crate 级 broker `crates/sisyphus-cli/src/approval.rs` + `Dialog::ToolApproval` + `shell::exec` Ask 分支接入，审计口径 `asked-approved`/`asked-denied`）；**dev 工具闸已接入同一权限体系**（allow 直接放行 / deny 直接拒绝 / ask 才弹窗，3 个新单测；**CLI 的 `sisyphus dev` 也已接入**（此前完全绕过权限闸，模型提出的命令静默执行）→ 新增 `CliDevApprover` + `--yes`：非交互默认拒绝（fail-safe）、显式 deny 优先于 `--yes`、审计与其它入口同口径；确定性验收 `scripts/l3-dev-permission-check.sh` **16/16 PASS**（证据 `phase1/l3-dev-permission-gate.md`）；顺带修掉两个既有缺口：`delegate inline/run` 执行命令也绕过权限闸 → 现每条命令都过闸；`require_tool` 在 **stdin EOF** 时 fail-open（空串被当成「同意」，`sisyphus exec "..." < /dev/null` 静默执行危险命令）→ EOF 现判 `asked-unanswered` 并拒绝；配套给 `dev` / `exec` / `command run --exec` / `delegate` 加 `--yes`，让非交互自动化有**显式**出口（显式 `echo y` 仍可用））；5 个新单测 |
| T3.2 Provider 交互式引导 | DONE（本地真实 provider）/ 部分外部后置 | `provider login`（选择→输入→**真实连通性测试**→写配置，失败不落盘）、`provider test`（原为桩实现，现真实调用并对 401/403/404/429/解析失败/连接失败分类给出可执行 hint）、`provider remove`；顺带修复 `request_timeout_secs` 被 client 级 5s 硬编码钳制（配置调大无效，慢/本地模型必然失败）；端到端 12/12 PASS（本机 ollama `qwen2.5-coder:3b` 真实 completion + mock 401/404/HTML/不可达失败路径），证据 `docs/benchmarks/acceptance/2026-11-12/phase3/t3-2-provider-onboarding.md`；**云端 provider 首次真实调用仍未验证**（本机无有效云端凭据） |
| T3.3 启动分段归因与优化 | DONE | bridge `startup_phase_*_ms` + `scripts/measure-bridge-startup.sh`（end_to_end_ready_ms≈1530ms）；TUI 首帧 PTY 测量 `scripts/measure-tui-first-frame.sh` + `make perf-tui`（p50≈14.3ms）；已接入 nightly workflow |
| T3.4 并行冲突感知 | DONE | 新增 `team/conflict.rs`：`detect_conflicts`（写写/读写冲突，写键含显式 `write_keys` 与兼容旧 `args["_write_keys"]`）+ `partition_by_write_conflicts`（按写冲突拆波次）；调度器检测 → `TeamEvent::ConflictDetected` 告警 + `tracing::warn!` → 冲突任务串行化（波次内并行）；既有 `TeamContext::detect_write_conflicts` 复用新模块；6 个新单测 |
| T4.1 质量指标定义与采集 | DONE | 指标模型与聚合链路落地（`sisyphus-metrics::quality` 9 单测、`sisyphus verify --record`、`sisyphus metrics quality`、`scripts/quality-metrics-collect.sh` 接入真实聚合）；**首轮真实数据**：3 个受控 fixture 任务（可自动修复 lint / 不可修复类型错误 / 干净代码）→ 成功率 0.667、修复率 0.5、回归率 0.0、平均 640ms（`docs/benchmarks/quality-metrics/2026-11-11-6f1bcbd-fixture-samples.json`）；同时修正采集脚本两处真实缺陷：端点应为 `/api/metrics`、指标名 `sisyphus_llm_*` 并不存在（实际为 `sisyphus_token_budget_*`）。**现有 7 条真实任务样本**：4 条仓库 `verify` 任务（`records/repo-*.json`）+ 3 条 L3 闭环（`records/l3-s{1,2,3}-*.json`，由 `dev --record` 产生，本地 ollama `qwen2.5-coder:3b`）；聚合（**含 1 条失败样本**的一轮真实 L3 运行）：成功率 0.857 / 修复率 0.667 / 回归率 0.0 / 平均 65.6s；**token 成本维度已打通**：provider 上报的 usage 随 `dev --record` 落进记录（3 条 dev 记录均值 1893.7 token/任务，verify 链路暂无），快照见 `docs/benchmarks/quality-metrics/2026-11-12-2e8bf69.json`。`dev --record` 强制要求 `--verify-cmd`（无客观闸门不记录），闸门失败同样落记录以作成功率分母；`regressed` 恒 false（dev 不做跨任务回归检测） |
| T4.2 性能回归门禁 | DONE | `scripts/perf-regression-check.sh <mode>` + `make perf-regression{,-bridge}`；基线 `docs/benchmarks/perf-regression-baseline-2026-11-10.json`；已接入 nightly workflow |
| T4.3 evals 子集入 CI | DONE | eval-runner 支持 `--cases`（`agent/id`）；`scripts/evals-ci-run.sh` + `make eval-ci`；已接入 ci.yml（PR 期 `evals-ci` job）；实跑子集 2/2 PASS（`docs/benchmarks/evals-ci/2026-11-11-5a23f9a.json`） |
| T4.4 统一 trace 关联 | DONE | 设计 `docs/plans/t4-4-trace-correlation-design.md`；实现 7 步全落地：trace id 工具（types）、HTTP 入口中间件、gRPC 拦截器 + 鉴权组合、SSE 负载 `trace_id` 维度键、CLI metadata 注入、gRPC 关键方法 span 字段、`book/src/sse-events.md` + OpenAPI 契约；10 个 trace 单测；实跑透传/生成/非法替换三态验证；**SSE 端到端**（订阅 + 广播 → 事件 payload 含 trace_id）；**gRPC 端到端**：CLI `remote exec` → bridge `grpc.execute_sop` → 认知循环全部嵌套 span 继承同一 trace id（`docs/benchmarks/acceptance/2026-11-12/phase4/trace-grpc-e2e.txt`） |
| T4.5 长窗口稳定性 | DONE | `scripts/long-run-stability.sh` + `make long-run-stability`（RSS/FD/线程采样 + 只读负载 + 泄漏启发式判定），已接入 nightly workflow；**4 小时窗口**（2881 采样，14398s）：探活与 /api/metrics 成功率 100%，RSS 55404→55996 KB（斜率 **-0.83 KB/min**，decreasing）、FD 20→11、线程 18→14 均不增反降 → **无泄漏迹象**（证据 `docs/benchmarks/long-run/2026-11-11-31d14c3/`）；对照 10 分钟窗口斜率 +35.6 KB/min、60 分钟 +5.8 KB/min，说明短窗斜率含噪声 |

> 状态取值：DONE / IN_PROGRESS / BLOCKED / TODO。`BLOCKED` 必须写明缺失的环境或依赖。

