# Bridge 路径 TaskRecord 落盘与 `metrics quality` 覆盖（最小设计）

> 状态：**设计草案**（2026-11-13）。用于落实计划中的“bridge 路径 TaskRecord 落盘 / `metrics quality` 覆盖”，在实现前明确执行落点、记录契约与归因口径。
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
| HTTP goal 执行 | `crates/sisyphus-bridge/src/http/handlers.rs::execute_goal_handler` (`POST /api/execute/goal`) | `Ok((output, report))` → 200；`Err(e)` → 错误响应；handler 内已 `await` SSE forwarder 排空 | 有 `sop_id`/`project_id`/`goal`/`trace_id` 上下文，适合作为首个落点 |
| HTTP composite 执行 | `execute_composite_handler` | 类似（SSE sop_id 需注意） | 可作为后续落点 |
| HTTP sop/工具执行 | 相关 handlers | 视具体语义 | 仅在可定义“任务级”完成判定时纳入 |
| gRPC 执行 | `crates/sisyphus-bridge/src/grpc/execution.rs` 等 | `execute_sop`/goal 类 RPC | 与 HTTP 对齐后再纳入，避免口径分叉 |
| 流式 goal | `execute_goal_streaming`/SSE 端点 | 结束事件可观测 | 若与 unary 路径共享内核完成点，可在共享层统一落盘 |

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
2. **Bridge 记录写入模块**：新增轻量 writer（依赖 `sisyphus-metrics::quality::TaskRecord`），负责序列化、目录创建、原子写文件、best-effort 错误处理。
3. **接线 `execute_goal_handler`**：计时开始/结束；在成功与失败分支组装记录并调用 writer；补充 `tracing` 字段（`task_id`/`duration_ms`/`status`）便于排查。
4. **配置与开关**：增加最小配置（env/Config 字段二选一，按现有模式）；更新 `config.example.toml` 与 `book/`（若引入 Config 字段）。
5. **测试**：
   - 单元测试：记录映射（成功/失败/无 token/dirty 归因）与 JSON 往返。
   - 集成测试：在可控 bridge 执行路径下断言记录文件生成且 `sisyphus metrics quality` 可聚合（注意 bridge 测试在 CI 的 OOM 约束与 `#[ignore]` 策略，按仓库惯例处理）。
6. **文档与证据**：更新 `docs/plans/single-node-quality-metrics.md`、`docs/plans/automation-development-plan-2026-11.md` 状态与边界说明；产出一次真实运行的记录与聚合快照作为证据。

## 6. 风险与开放问题

- **多路径覆盖**：仅接线 HTTP goal 会遗漏 gRPC/composite/streaming；需在后续迭代下沉到共享执行层并补齐验收。
- **token/usage 来源**：goal 执行报告中 usage 字段形态需确认；不可用时必须保持 `None`。
- **并发与高频写入**：原子写与唯一文件名可缓解；若执行量增大需考虑轮转/索引（非本次范围）。
- **语义误读**：`Pass` 不等于外部验收通过；需在 metrics 输出或文档中提示按 `scenario` 区分来源。

## 7. 验收标准（实现阶段）

- 至少 1 条来自 bridge `execute_goal` 的 `TaskRecord` 落盘（成功路径）与 1 条失败路径记录，字段完整、可被 `sisyphus metrics quality` 正确解析聚合。
- `engine_commit`/`engine_dirty` 填充规则与 CLI 一致；无法获取时显式为 `None`（不猜测）。
- 落盘失败不影响原有 HTTP 响应/状态码/SSE 行为（回归测试覆盖）。
- 文档同步（计划状态、质量指标口径、配置示例如适用）。