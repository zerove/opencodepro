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
| 单任务耗时/token 成本 | 单任务端到端耗时、LLM token 用量 | 任务记录 `duration_ms` + bridge `/api/metrics`（`sisyphus_token_budget_usage` 等） | 链路已就绪（bridge 未运行时记 `null`） |

## 2. 采集口径

- **任务记录**：`sisyphus verify --record <path>` 每次执行落一条 JSON 记录
  （`task_id` / `scenario` / `status` / `initial_verify_passed` / `final_verify_passed` /
  `fix_attempts` / `regressed` / `duration_ms` / `recorded_at`），记录目录约定为
  `docs/benchmarks/quality-metrics/records/`（格式说明见该目录 `README.md`）。
- **聚合**：`sisyphus metrics quality --records <dir> [--json]` → 成功率 / 修复率 /
  回归率 / 平均耗时；无记录时各比率为 `null`（不伪造 0 值）。
- **Bridge 指标**：`GET /api/metrics` → Prometheus 文本。当前采集的**真实**指标名：
  `sisyphus_token_budget_usage`（LLM 阶段 token 消耗估算，gauge）、
  `sisyphus_token_budget_capped`、`sisyphus_token_budget_exhausted`、
  `sisyphus_http_requests_total`。
  （此前脚本里写的 `sisyphus_llm_requests_total` / `sisyphus_llm_tokens_total`
  **并不存在**，导致 bridge 在线时仍恒为 `null`，已修正。）
- **证据目录**：`scripts/single-node-acceptance-evidence.sh --run-l0` 已自动化 L0；L3 待环境。

## 3. 采集脚本

`scripts/quality-metrics-collect.sh [records_dir]` 输出指标快照 JSON
（`docs/benchmarks/quality-metrics/<date>-<commit>.json`）：

- `bridge_metrics_available` / `token_budget_usage` / `token_budget_capped` /
  `token_budget_exhausted` / `http_requests_total`
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
| `2026-11-11-6f1bcbd.json` | 指标名修正后重采（bridge 在线，任务级记录目录仍为空 → `null`） |
| `2026-11-11-d4a6ef0.json` | **首个真实仓库任务样本**：`sisyphus verify -p sisyphus-tools-build --max-warnings 0` → PASS（0 错 0 警，79s）→ 成功率 1.0、平均 79.0s |
| `2026-11-11-6f1bcbd-fixture-samples.json` | **受控 fixture 样本**：3 个真实 `verify --fix` 运行（可自动修复 lint / 不可修复类型错误 / 干净代码）→ 成功率 0.667、修复率 0.5、回归率 0.0、平均 640ms |
| `fixture-samples/` | 上述 3 条任务记录原文（`sisyphus verify --record` 产出） |
| `pipeline-demo-2026-11-11.json` | 最早的流程演示样本（2 个 fixture crate），保留作对照 |

**样本性质说明**：`fixture-samples/` 下的记录来自**受控 fixture crate** 的真实
`sisyphus verify --fix` 运行（真实 cargo check / clippy / cargo clippy --fix 执行），
用于验证指标口径与聚合链路；**不是**仓库真实开发任务的样本。
仓库真实任务样本目录为 `records/`，现已含 1 条真实样本
（`repo-sisyphus-tools-build.json`：`sisyphus verify -p sisyphus-tools-build --max-warnings 0`
的真实运行结果）。后续在具备 LLM 凭据的环境（T1.4）补齐更多真实开发任务样本。

## 5. 关联

- `docs/plans/automation-development-plan-2026-11.md`（T4.1）
- `docs/plans/single-node-performance-baseline.md`
- `crates/sisyphus-metrics/src/quality.rs`（记录模型与聚合）
- `crates/sisyphus-cli/src/handle_metrics.rs`、`crates/sisyphus-cli/src/handle_verify.rs`
- `scripts/single-node-bench-sample.sh`
