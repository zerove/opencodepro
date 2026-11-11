# 单机质量指标与采集（T4.1）

> 对应计划任务：T4.1「质量指标定义与采集」。
> 指标口径与采集链路已落地；**仓库真实任务样本**仍需 L3 环境（T1.4）执行后写入记录目录。

## 1. 指标定义

| 指标 | 定义 | 来源 | 状态 |
|---|---|---|---|
| L3 场景成功率 | 验收 PASS / 可解释 BLOCKED 的比例 | `docs/benchmarks/acceptance/<date>-l3/` 证据目录 | **BLOCKED**（缺 LLM/凭据环境，T1.4） |
| 任务成功率 | `pass / (pass + fail)`（blocked 单独计数） | 任务记录目录（`TaskRecord`）聚合 | 链路已就绪，真实样本待 L3 |
| 任务修复率 | 初次验证失败且最终通过 / 初次验证失败 | 同上 | 同上 |
| 任务回归率 | 修复引入回归 / 执行过修复 | 同上 | 同上 |
| 单任务耗时/token 成本 | 单任务端到端耗时、LLM token 用量 | 任务记录 `duration_ms` + bridge `/api/metrics`（`sisyphus_llm_*`） | 链路已就绪（bridge 未运行时记 `null`） |

## 2. 采集口径

- **任务记录**：`sisyphus verify --record <path>` 每次执行落一条 JSON 记录
  （`task_id` / `scenario` / `status` / `initial_verify_passed` / `final_verify_passed` /
  `fix_attempts` / `regressed` / `duration_ms` / `recorded_at`），记录目录约定为
  `docs/benchmarks/quality-metrics/records/`（格式说明见该目录 `README.md`）。
- **聚合**：`sisyphus metrics quality --records <dir> [--json]` → 成功率 / 修复率 /
  回归率 / 平均耗时；无记录时各比率为 `null`（不伪造 0 值）。
- **Bridge 指标**：`GET /api/metrics` → Prometheus 文本（`sisyphus_llm_requests_total`、
  `sisyphus_llm_tokens_total`）。
- **证据目录**：`scripts/single-node-acceptance-evidence.sh --run-l0` 已自动化 L0；L3 待环境。

## 3. 采集脚本

`scripts/quality-metrics-collect.sh [records_dir]` 输出指标快照 JSON
（`docs/benchmarks/quality-metrics/<date>-<commit>.json`）：

- `bridge_metrics_available` / `llm_requests_total` / `llm_tokens_total`
  （bridge 未运行时为 `null`，不伪造值）；
- `task_total` / `task_passed` / `task_failed` / `task_blocked` /
  `task_success_rate` / `task_fix_rate` / `task_regression_rate` /
  `task_cost_duration_ms_per_task`（由 `sisyphus metrics quality` 聚合；无记录时 `null`）。

## 4. 运行

```bash
# 1) 产生一条任务记录（验证-修复循环）
sisyphus verify --record docs/benchmarks/quality-metrics/records/<task>.json

# 2) 聚合查看
sisyphus metrics quality --records docs/benchmarks/quality-metrics/records

# 3) 采集快照
./scripts/quality-metrics-collect.sh
```

已落盘的快照：

| 文件 | 说明 |
|---|---|
| `2026-11-11-354a9c1.json` | 首轮采集（bridge 未运行、任务级指标 `null`） |
| `2026-11-11-ca3f870.json` | 采集链路完成后重采（记录目录为空 → 任务级比率 `null`） |
| `pipeline-demo-2026-11-11.json` | **流程演示样本**：记录来自 `/tmp` 下两个 fixture crate（一个 check 通过、一个类型错误），仅用于验证聚合链路，不代表仓库真实任务指标 |

## 5. 关联

- `docs/plans/automation-development-plan-2026-11.md`（T4.1）
- `docs/plans/single-node-performance-baseline.md`
- `crates/sisyphus-metrics/src/quality.rs`（记录模型与聚合）
- `crates/sisyphus-cli/src/handle_metrics.rs`、`crates/sisyphus-cli/src/handle_verify.rs`
- `scripts/single-node-bench-sample.sh`
