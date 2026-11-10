# P1/P2 路线细化（单机优先，引擎主线 + 选择性产品化）

> 日期：2026-11-10  
> 输入：`docs/comparison/opencode-jcode-sisyphus-comparison-2026-11.md`（对标分析）、S1–S4 单机交付基线。  
> 原则：**引擎优先**；产品化能力必须复用 SOP/DAG/bridge/观测等既有底座；不阻塞于专用 runner（ADR-019 控制面生产准入为外部后置，`cluster.control_plane.enabled=false`）。

## 1. 目标与边界

- **P1（产品化切面）**：在单机场景下提升“可用、可理解、可集成”的编码/任务工作流体验，缩小与 OpenCode/JCode 在关键体验上的可感知差距。
- **P2（差异化增强）**：强化 Sisyphus 独有优势（认知循环、SOP/DAG 编排、统一观测与集成面），形成可演示的差异化能力。
- **不做**：不追求全量产品形态对标；不提前启用控制面生产能力；不引入与主线无关的并行体系。

## 2. 可借鉴点 → 任务映射

### 2.1 来自 OpenCode（产品化体验）

| 借鉴点 | 单机价值 | 映射任务（P1/P2） | 验收要点 |
|---|---|---|---|
| 会话/任务流的顺畅体验 | 降低使用摩擦，提升端到端可用性 | **P1-A 会话与任务流体验**：统一创建/执行/查看/错误反馈路径（CLI/WebUI/SDK 一致）；补充常见失败的可解释提示 | 关键路径可按文档复现；错误信息可定位（配置/依赖/契约） |
| 工具/MCP 可发现性与配置清晰度 | 减少“能力存在但不会用” | **P1-B 工具与 MCP 可发现性**：工具/技能列表与参数说明完善；MCP 配置与状态反馈优化；示例与文档对齐 | 用户可从文档/界面发现并调用常用能力；分层前置条件明确（S3） |
| 文档与示例场景化 | 提升上手与集成效率 | **P1-C 示例与场景文档**：扩展最小示例为场景化用例（SOP 执行、会话、观测）；与 SDK/REST 契约同步 | 示例可运行（在具备前置条件时）；文档与实现一致 |

### 2.2 来自 JCode（协作与性能意识）

| 借鉴点 | 单机价值 | 映射任务（P1/P2） | 验收要点 |
|---|---|---|---|
| 多 agent 协作的清晰编排体验 | 复杂任务并行/分工的可理解性 | **P2-A 编排协作能力外显**：基于 SOP/DAG 提供可复用的多步骤/多角色编排模板与观测视图（非 Swarm 复刻，突出可审计编排） | 至少 1–2 个可演示模板；执行状态与结果可观测 |
| 性能/资源可感知与可回归 | 单机体验与工程质量 | **P2-B 单机性能与资源基线**：建立关键路径（启动、健康检查、典型执行/查询）的可复现测量与回归记录（口径自洽，不直接与 JCode 数值对标） | 基线文档化；后续变更可对比 |
| 高密度工作流的可靠性 | 长任务/多步骤稳定性 | **P1-D 稳定性与错误态**：超时/重试/降级提示一致性；bridge 不可达与依赖缺失时的统一体验（CLI/WebUI/SDK） | 无静默失败；错误可解释且可恢复指引清晰 |

### 2.3 Sisyphus 差异化强化（引擎主线）

| 方向 | 映射任务（P1/P2） | 验收要点 |
|---|---|---|
| SOP/DAG + 认知循环 | **P1-E SOP 端到端可用性**：典型 SOP 的创建/列表/执行/观测闭环打磨；认知追踪与结果解释增强 | 单机可按 runbook 完成闭环；观测与结果可关联 |
| 统一接入与观测 | **P1-F 契约与观测守护**：持续契约回归（bridge/SDK/WebUI）；只读观测面完善（延续已交付能力） | 契约测试与文档同步；`make audit`/check-all 全绿 |
| 平台化集成 | **P2-C 集成与扩展样例**：面向二次开发的集成示例（SDK + REST + 事件/观测），突出平台优势 | 示例与文档可支撑集成场景 |

## 3. 分阶段计划与里程碑

### P1（近期，1–3 个迭代，单机可交付）

| ID | 任务 | 优先级 | 依赖 | 主要交付 |
|---|---|---|---|---|
| P1-F | 契约与观测守护（持续） | P0 | 无 | 回归用例/文档同步、门禁稳定 |
| P1-A | 会话与任务流体验 | P0 | P1-F | 关键路径体验优化、错误反馈一致性 |
| P1-B | 工具与 MCP 可发现性 | P1 | P1-F | 配置/状态/文档与示例改进 |
| P1-E | SOP 端到端可用性 | P1 | 引擎/bridge 稳定 | SOP 闭环示例与观测关联 |
| P1-C | 示例与场景文档 | P1 | P1-A/B/E 进展 | 场景化文档与可运行示例 |
| P1-D | 稳定性与错误态 | P1 | P1-A | 统一降级与可解释错误 |

**进展（2026-11-10）**：
- P1-C/P1-E：新增 `docs/runbooks/single-node-typical-workflow.md`、`docs/runbooks/single-node-sop-e2e-guide.md`、`docs/examples/single-node-sdk-examples.md`（场景化与端到端指引）；补充 `sop run verify_sop` 成功与 `execute/goal` 无 LLM 可解释错误的路径差异实测；新增 `scripts/single-node-demo-flow.sh` 与 `sisyphus sop show`（HTTP 详情查看）。
- P1-A：新增 `docs/runbooks/single-node-sessions-and-tasks.md`（本地 CLI 会话 vs bridge 服务端会话与任务流）。
- P1-B：新增 `docs/runbooks/single-node-tools-mcp-guide.md`（工具/MCP 可发现性与使用路径）；CLI 增强 `sop list`/命令引导与 `quickstart` 指引。
- P1-D：`fix(cli): 优化 bridge 不可达时的错误提示` 已落地；新增 `sisyphus doctor`（本地+gRPC/HTTP 诊断）与 `sisyphus quickstart`（上手引导）；`docs/runbooks/single-node-tools-and-dependencies.md` 补充错误处理与反馈一致性指引。
- P1-F：新增 `docs/runbooks/single-node-contract-guard.md`，并落地 `scripts/single-node-contract-smoke.sh` 与 `make smoke-single-node`/`make demo-single-node`/`make readiness-check` 可执行入口；runbook/验收矩阵持续补充运行时实测证据。

**P1 里程碑**：单机“按文档可完成典型任务流”的体验达标；契约与门禁持续全绿；差异化能力（SOP/观测）可被清晰感知。

### P2（中期，依赖 P1 基线与资源）

| ID | 任务 | 优先级 | 依赖 | 主要交付 |
|---|---|---|---|---|
| P2-A | 编排协作能力外显（SOP/DAG 模板） | P1 | P1-E | 可演示编排模板与观测 |
| P2-B | 单机性能与资源基线 | P1 | 测量口径确定 | 基线与回归记录 |
| P2-C | 集成与扩展样例 | P2 | P1-C/F | 平台化集成示例 |
| P2-D | 控制面准入（条件触发，外部前提） | 后置 | 专用 runner 真实网络硬件 | ADR-019 证据矩阵 → 准入评审（不改变默认关闭策略） |

**进展（2026-11-10）**：
- P2-A：新增 `docs/plans/single-node-orchestration-templates.md` 与 `docs/examples/sop-templates/`（顺序与扇出/汇聚“受控并行探索”模板）；`parallel_explore_aggregate_template` 已实测保存/查询/`sop run` 执行成功并产出可观测 CognitionReport。
- P2-B：新增 `docs/plans/single-node-performance-baseline.md` 与 `docs/benchmarks/single-node-runtime-baseline-2026-11-10.md`（dev/release bridge 就绪与只读端点延迟、CLI 命令耗时采样；release `--help` 存在波动已记录复测说明）。
- P2-C：新增/增强 `docs/examples/single-node-integration-samples.md`；提供 `scripts/single-node-readiness-check.sh`（实测）与 `scripts/single-node-demo-flow.sh`（端到端发现→执行→观测流程实测）可运行样例，配合 `scripts/single-node-contract-smoke.sh`。
- P1-F：契约守护补充可执行冒烟清单与自动化入口（脚本 + Make 目标）；`single-node-contract-smoke.sh` 支持 `SMOKE_BRIDGE_HTTP` 可选运行时就绪冒烟。
- P2-D：仍为**外部后置**（专用 runner 真实网络证据与 ADR-019 准入），不阻塞单机 P1/P2 可实施交付；`cluster.control_plane.enabled` 保持 `false`。

**P2 里程碑**：形成可对外说明的差异化能力组合（编排+观测+集成）；性能基线可回归；控制面仅在证据完备后推进。

## 4. 执行与验证要求

- 每个任务交付：代码/文档/测试或可复现步骤 + 门禁证据（`cargo check-all`/`make audit`/相关测试）。
- 不伪造证据；外部依赖（LLM/凭据/专用 runner）缺失时明确标注为环境限制或后置项。
- 保持 ADR-019/020 边界与默认安全配置不变。

## 5. 与现有计划的关系

- 承接 `docs/plans/single-node-full-functionality-audit.md` 的 S1–S4 基线，作为 P1 启动输入。
- 对标依据见 `docs/comparison/opencode-jcode-sisyphus-comparison-2026-11.md`；历史对比见 `docs/comparison/opencode-omo-comparison.md`（含 2026-11 更新说明）。
