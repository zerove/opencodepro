# T2.1「TUI 对话式执行 MVP」后续开发计划

> 关联主计划：`docs/plans/automation-development-plan-2026-11.md`（阶段 2 / T2.1）。
> 本文固化 T2.1 的设计方案综合研判、切片边界、验收口径与回退策略，作为后续实施依据。
> 状态约定：DONE / IN_PROGRESS / TODO / BLOCKED（BLOCKED 必须写明缺失环境）。

## 1. 目标与验收标准（引自主计划）

- 目标链路：输入目标 → 流式输出 → 工具调用卡片 → 批准/拒绝/中断 → 继续/恢复；复用 SSE 与 `StreamEvent`。
- 验收：一次会话含 ≥2 次工具调用、1 次审批、1 次中断恢复；录屏/日志证据。
- 阶段 2 定位：交互与治理（T2.1 与 T2.2 共享审批通道，T2.1 负责 dev 循环内的审批接线）。

## 2. 设计综合（3 份并行子设计 + 评审结论）

| 维度 | 方案 A（前端最小化） | 方案 B（执行控制） | 方案 C（纵向切片） |
|---|---|---|---|
| 审批 | TUI 增 `dev_ask` 态 + `BgOp::DevAsk`，task 挂起等解 | `DevApprover` trait + `with_approval_handler`，动作统一门控 | 切片 2：gate seam + TUI 审批键位 |
| 中断 | Esc → 前端回位 + `resume=true`（task 不硬取消） | Esc → task 侧 cancel → checkpoint → `Cancelled` | 切片 1：interrupt → checkpoint → resume |
| 流式/卡片 | 扩 `StreamEvent` 结构化字段 | 新增 `BgOp::DevStepEvent` 复用 stream 区 | 复用 T1.3 `ToolCall` 事件形状 |
| LLM 流式 | token 级不可行 → step 进度 + 终态 report 作流式等价物 | 同 | 同 |

**共识与决策**：

1. MVP 走「路 A」——复用 in-process `Developer`（`sisyphus-tools-agent`），**不**引入 bridge goal stream（跨边界改动大、审批/中断需新 RPC）。
2. 采纳 B+C 的骨架（引擎补控制线 + 切片 1→2→3→4 推进）+ A 的前端细节（`dev_ask` 状态、`y/n/Esc` 键位、卡片复用 stream 区）。
3. 明确不做：token 级 LLM 流式、bridge goal stream、`DevApprover` 之外的通用事件总线。

**评审暴露的关键缺口（已修复）**：`spawn_dev_agent` 原先未接 `checkpoint_dir`，导致 `resume=true` 也无 checkpoint 可恢复——切片 1/3 必须接线。

## 3. 切片计划

| 切片 | 范围 | 关键文件 | 验收 | 状态 |
|---|---|---|---|---|
| S1 引擎中断 | `Developer` interrupt flag + `Cancelled` 状态 + checkpoint 保存 | `crates/sisyphus-tools-agent/src/developer.rs`、`developer/types.rs` | 中断返回 `Cancelled`；resume 跳过已完成 step | DONE |
| S2 引擎审批 | `DevApprover` trait + 动作门控（allow/deny） | 同上 | deny → step 标记 denied-by-user；allow → 放行 | DONE |
| S3 TUI 接缝 | `BgOp::DevAsk` + `DevRunHandle` + App 审批态 + `y/n/Esc` 键位 + 工具卡片渲染 | `crates/sisyphus-cli/src/tui/{background,app}.rs`、`tui/dialog/{handlers,renders}.rs` | 单测覆盖：ask 到达 → y/n 应答；Esc 中断；恢复入口 | DONE |
| S4 闭环与证据 | `spawn_dev_agent` 接线 checkpoint/审批/中断 + 一次会话验收（≥2 工具调用 + 1 审批 + 1 中断恢复） | `tui/background.rs`、证据脚本 | 可复跑证据落 `docs/benchmarks/acceptance/<date>-t2.1/` | DONE（脚本化验收测试 `crates/sisyphus-tools-agent/tests/t2_1_mvp_acceptance.rs`） |

切片顺序固定 **S1 → S2 → S3 → S4**：S1/S2 可独立验收与回退，S3 是 UI 接缝，S4 才做端到端闭环。

## 4. 验收证据口径

- 引擎级：`cargo test -p sisyphus-tools-agent --lib`（S1/S2 单测已覆盖）。
- TUI 级：`cargo test -p sisyphus-cli --lib`（S3 状态机单测：审批应答、中断、恢复入口）。
- 端到端（S4）：脚本化 mock 驱动的一次会话，落盘工具调用计数、审批决策、中断与恢复记录。
  - 已落地：`crates/sisyphus-tools-agent/tests/t2_1_mvp_acceptance.rs`（2 步确定性计划 → 第 1 步审批通过后中断 → `Cancelled` + checkpoint → resume 跳过已完成步 → `Success`；断言 2 次工具调用 / 2 次审批 / 1 次中断恢复）。
- 录屏不作为自动化验收依据（不可复跑）；以「权限审计 JSONL + checkpoint JSON + 事件流日志」作为等效证据，并在证据目录 `summary.md` 说明替代关系。
  - 当前等价证据：TUI 日志（`Dev agent awaiting approval` / `Dev tool call approved` / `Dev agent interrupted`）+ `dev_checkpoint.json`。
  - 真实 LLM 会话录制仍受 T1.4 环境阻塞（缺凭据），补齐后追加到 `docs/benchmarks/acceptance/<date>-t2.1/`。

## 5. 风险与回退

| 风险 | 回退 |
|---|---|
| 审批应答通道引入额外并发原语（oneshot/mpsc/watch）复杂度上升 | 退回「CLI stdin 审批」既有路径（T2.2 已实现），TUI 仅展示 pending 态 |
| 中断语义与既有「Esc 即取消」行为冲突 | 保留旧行为（仅置 idle）为默认；新语义需显式开关 |
| `Developer` 错误类型仍为 `String`（技术债） | 不在 T2.1 内改动签名，记入后续统一 `thiserror` 迁移 |
| 并行（parallel=true）下中断/checkpoint 语义不完整 | MVP 验收固定 `parallel=false`，并行语义记入 follow-up |

## 6. 与其他任务的边界

- T2.2（权限规则与审批审计）：CLI 层已 DONE，TUI 弹窗审批未完成。T2.1 的 dev 循环审批与 T2.2 的 TUI 弹窗审批共用同一「pending 审批 + 应答」通道，S3 落地后 T2.2 可直接复用，剩 T2.2 仅需把 `require_tool` 的 Ask 分支接到同一应答通道。
- T3.3（启动分段）：bridge 侧已落地，TUI 首帧 PTY 测量待续，与 T2.1 无耦合。
- T4.3（evals 子集入 CI）：已 DONE；T2.1 完成后可把新增 mock 单测并入 evals 稳定子集。
- T1.4 / T3.2：仍 BLOCKED（缺 LLM 凭据环境），T2.1 全链路使用 mock，不依赖其解除。

## 7. 状态更新规则

- 每完成一个切片：更新主计划 §11 执行进展表中 T2.1 行（状态 + 证据命令与结果），并在本文 §3 表格更新切片状态。
- 提交信息使用中文 conventional commits；每条提交附可复跑验证命令。
- 文档不使用 `- [x]` 勾选项（避免与 `scripts/audit_todo.py` 的证据门禁冲突）。
