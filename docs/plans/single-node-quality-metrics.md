# 单机质量指标与采集（T4.1 落地部分）

> 对应计划任务：T4.1「质量指标定义与采集」。本文件冻结本阶段已落地的指标口径与采集来源；
> 依赖执行证据的任务级指标（成功率/修复率/回归率）在 L3 环境具备后补齐。

## 1. 指标定义

| 指标 | 定义 | 来源 | 状态 |
|---|---|---|---|
| L3 场景成功率 | 验收 PASS / 可解释 BLOCKED 的比例 | `docs/benchmarks/acceptance/<date>-l3/` 证据目录 | **BLOCKED**（缺 LLM/凭据环境，T1.4） |
| 任务修复率 | 失败任务中修复后恢复通过 的比例 | T1.2 验证-修复循环记录（`VerifyFixReport`） | **BLOCKED**（未跑真实闭环） |
| 任务回归率 | 修复引入的新失败数 / 修复任务数 | 修复前后验证对比 | **BLOCKED**（需执行证据） |
| 单任务耗时/token 成本 | 单任务端到端耗时、LLM token 用量 | bridge `/api/metrics`（`sisyphus_llm_*`）、执行事件 | **IN_PROGRESS**（collector 已就位） |

## 2. 采集口径

- Bridge 指标端点：`GET /api/metrics` → Prometheus 文本（`sisyphus_llm_requests_total`、`sisyphus_llm_tokens_total`）。
- CLI 验证-修复循环：`sisyphus verify` 输出（human/JSON），每次运行即一次修复率样本。
- 证据目录：`scripts/single-node-acceptance-evidence.sh --run-l0` 已自动化 L0；L3 待环境。

## 3. 采集脚本

`scripts/quality-metrics-collect.sh` 输出首轮指标快照 JSON（`docs/benchmarks/quality-metrics/<date>-<commit>.json`）：

- `bridge_metrics_available` / `llm_requests_total` / `llm_tokens_total`（bridge 未运行时为 `null`，不伪造值）；
- `task_success_rate`、`task_fix_rate`、`task_regression_rate` 按定义置为 `null` 直至 L3 证据。

## 4. 运行

```bash
./scripts/quality-metrics-collect.sh
```

## 5. 关联

- `docs/plans/automation-development-plan-2026-11.md`（T4.1）
- `docs/plans/single-node-performance-baseline.md`
- `crates/sisyphus-metrics/`、`scripts/single-node-bench-sample.sh`
