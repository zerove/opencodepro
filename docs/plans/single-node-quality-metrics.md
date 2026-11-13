# 单机质量指标与采集（T4.1）

> 对应计划任务：T4.1「质量指标定义与采集」。
> 指标口径与采集链路已落地，L3 场景已用本地真实模型解阻（三场景 × 3 轮 9/9 PASS，
> 见 `docs/benchmarks/acceptance/2026-11-12/phase1/t1-4-l3-closed-loop.md`），
> 并经 `sisyphus dev --record` 转成 `TaskRecord` 写入记录目录。

## 1. 指标定义

| 指标 | 定义 | 来源 | 状态 |
|---|---|---|---|
| L3 场景成功率 | 验收 PASS / 可解释 BLOCKED 的比例 | `docs/benchmarks/acceptance/<date>/phase1/runs/` 证据目录 | 已有本地模型数据（9/9）；样本量小，待更多真实仓库任务 |
| 任务成功率 | `pass / (pass + fail)`（blocked 单独计数） | 任务记录目录（`TaskRecord`）聚合 | 7 条真实样本（4 仓库 `verify` + 3 L3 `dev` 闭环，含 1 条失败样本）→ 成功率 0.857 |
| 任务修复率 | 初次验证失败且最终通过 / 初次验证失败 | 同上 | 0.667（3 条首轮失败样本，2 条修复通过） |
| 任务回归率 | 修复引入回归 / 执行过修复 | 同上 | 0.0（`dev` 链路现检测「公开 API 被删除」并写入 `regressed`；`verify` 链路仍恒 false） |
| 单任务耗时 | 单任务端到端耗时 | 任务记录 `duration_ms` | 7 条样本，均值 ~65.6s |
| 单任务 token 成本 | 单任务输入/输出 token 数 | 任务记录 `prompt_tokens` / `completion_tokens`（provider 上报的 usage；`dev` 链路自动写入） | 3 条 `dev` 记录有真实 token（均值 1894/任务）；`verify` 链路暂无（后端未暴露 usage） |
| 样本版本归因 | 样本涉及的引擎版本集合（脏工作区样本单列 `sha+dirty`） | 任务记录 `engine_commit` / `engine_dirty` | 聚合输出 `engine_commits`；>1 组说明样本混用不同引擎版本，**比率不可直接横向比较** |
| 全局 token 预算 | 进程级 LLM token 消耗估算 | bridge `/api/metrics`（`sisyphus_token_budget_usage` 等） | bridge 未运行时记 `null` |

## 2. 采集口径

- **任务记录**：`sisyphus verify --record <path>`（验证-修复循环）或
  `sisyphus dev --record <path>`（LLM 闭环）每次执行落一条 JSON 记录
  （`task_id` / `scenario` / `status` / `initial_verify_passed` / `final_verify_passed` /
  `fix_attempts` / `regressed` / `duration_ms` / `prompt_tokens` / `completion_tokens` /
  `engine_commit` / `engine_dirty` / `recorded_at`），记录目录约定为
  `docs/benchmarks/quality-metrics/records/`（格式说明见该目录 `README.md`）。
- **版本归因**：`engine_commit` 记录产出样本的引擎短 commit，`engine_dirty` 记录
  当时工作区是否有未提交改动。缺这两个字段时，「成功率 0.857」无法归因到具体
  引擎版本 —— 不同版本的样本混在同一个扁平目录里聚合，比率变化可能只是样本
  构成变了。脏工作区产出的样本**不对应任何已提交版本**，聚合时单列为
  `sha+dirty` 并显式提示。历史记录（字段引入前产出）显示为「未知」，
  **不猜测**历史版本号。
- **token 口径**：只统计 provider **实际上报**的 usage；provider 未返回 usage 时
  字段留空（`null`），聚合时该记录**不参与** token 均值（避免把「未上报」当 0 拉低均值）。
  `verify` 链路目前拿不到后端 usage，恒为 `null`。
- **聚合**：`sisyphus metrics quality --records <dir> [--json]` → 成功率 / 修复率 /
  回归率 / 平均耗时 / 平均 token / 样本涉及的引擎版本；无记录时各比率为 `null`
  （不伪造 0 值）。
- **Bridge 指标**：`GET /api/metrics` → Prometheus 文本。当前采集的**真实**指标名：
  `sisyphus_token_budget_usage`（LLM 阶段 token 消耗估算，gauge）、
  `sisyphus_token_budget_capped`、`sisyphus_token_budget_exhausted`、
  `sisyphus_http_requests_total`。
  （此前脚本里写的 `sisyphus_llm_requests_total` / `sisyphus_llm_tokens_total`
  **并不存在**，导致 bridge 在线时仍恒为 `null`，已修正。）
- **Bridge 执行记录（opt-in）**：设置 `SISYPHUS_BRIDGE_QUALITY_RECORDS_DIR=<dir>`
  后，bridge 会为执行入口 best-effort 写入 `TaskRecord`（未设置则不写）。当前覆盖
  HTTP `execute_goal` / composite / goal streaming，以及 gRPC `execute_sop` /
  `stream_execution` / `call_jcode`；`scenario` 分别为 `bridge-goal-execute`、
  `bridge-composite-execute`、`bridge-goal-streaming`、`bridge-sop-execute`、
  `bridge-jcode-execute`。口径约束：bridge 执行**没有** dev 式客观闸门循环，
  `initial/final_verify_passed` 与终态一致、`fix_attempts=0`、`regressed=false`
  （不污染修复率/回归率分母）；`Pass` 仅表示执行路径完成，**不等于**外部验收通过。
  token usage 采用 **best-effort 独占租约采集**：provider 上报且观测租约独占时记录累计差值，
  并发/无 usage 时保持 `null`（未知 ≠ 0，不伪造）。
  首次 live E2E（失败路径）证据见
  `docs/benchmarks/acceptance/2026-11-13/bridge-task-record-e2e.md`；覆盖扩展
  （composite 成功 / streaming 失败）证据见
  `docs/benchmarks/acceptance/2026-11-13/bridge-task-record-coverage.md`。
- **证据目录**：`scripts/single-node-acceptance-evidence.sh --run-l0` 已自动化 L0；L3 待环境。

## 3. 采集脚本

`scripts/quality-metrics-collect.sh [records_dir]` 输出指标快照 JSON
（`docs/benchmarks/quality-metrics/<date>-<commit>.json`）：

- `bridge_metrics_available` / `token_budget_usage` / `token_budget_capped` /
  `token_budget_exhausted` / `http_requests_total`
  （bridge 未运行时为 `null`，不伪造值）；
- `task_total` / `task_passed` / `task_failed` / `task_blocked` /
  `task_success_rate` / `task_fix_rate` / `task_regression_rate` /
  `task_cost_duration_ms_per_task` / `task_cost_tokens_per_task` /
  `task_cost_prompt_tokens_per_task` / `task_cost_completion_tokens_per_task`
  （由 `sisyphus metrics quality` 聚合；无记录时 `null`）；
- `task_engine_commits` / `task_engine_unknown`（样本版本归因；无记录时 `null`）。

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
| `2026-11-11-d4a6ef0.json` | 首个真实仓库任务样本（`sisyphus-tools-build`）→ PASS |
| `2026-11-11-a49b685.json` | **4 个真实仓库任务样本**（types / metrics / tools-build / tools-agent，均 `--max-warnings 0` PASS）→ 成功率 1.0、平均 58.9s |
| `2026-11-11-6f1bcbd-fixture-samples.json` | **受控 fixture 样本**：3 个真实 `verify --fix` 运行（可自动修复 lint / 不可修复类型错误 / 干净代码）→ 成功率 0.667、修复率 0.5、回归率 0.0、平均 640ms |
| `2026-11-12-a668479.json` | 7 条真实样本（4 仓库 `verify` + 3 L3 `dev` 闭环）→ 成功率 1.0、修复率 1.0、平均 45.4s（**无 token 维度**，采集时 token 采集尚未打通） |
| `2026-11-12-a247771.json` | 同 7 条样本 + **token 成本维度**（provider 上报 usage → 任务记录）→ 成功率 1.0、修复率 1.0、平均 53.7s、**1038.7 token/任务**（该轮样本全部成功，口径偏乐观） |
| `2026-11-12-2e8bf69.json` | **含失败样本**的一轮真实 L3 运行（判定已加严）→ 成功率 0.857、修复率 0.667、平均 65.6s、**1893.7 token/任务**（失败样本是成功率的分母，只留成功样本会让指标失真） |
| `2026-11-12-5150383.json` | **13 条样本**（4 仓库 `verify` + 9 L3 `dev` 闭环，含 1 条失败样本；后 6 条 L3 记录带引擎版本归因）→ 成功率 0.923、修复率 0.889、平均 82.7s、**1673.0 token/任务**；`task_engine_commits: ["51503830c53b"]`、`task_engine_unknown: 7` |
| `fixture-samples/` | 上述 3 条任务记录原文（`sisyphus verify --record` 产出） |
| `pipeline-demo-2026-11-11.json` | 最早的流程演示样本（2 个 fixture crate），保留作对照 |

**样本性质说明**：`fixture-samples/` 下的记录来自**受控 fixture crate** 的真实
`sisyphus verify --fix` 运行（真实 cargo check / clippy / cargo clippy --fix 执行），
用于验证指标口径与聚合链路；**不是**仓库真实开发任务的样本。
仓库真实任务样本目录为 `records/`，现已含 13 条真实样本：

- 4 条仓库 `verify` 任务（`repo-sisyphus-{tools-build,types,metrics,tools-agent}.json`）；
- 3 条 L3 开发闭环（`l3-s{1,2,3}-*.json`，`dev --record` 产出，本地 ollama
  `qwen2.5-coder:3b`：s1/s2 首轮闸门失败 → 修复 1 次通过；**s3 失败** ——
  3B 把函数写成模块，根因是闸门修复上下文缺测试文件，已修（见
  `phase1/t1-4-l3-closed-loop.md` §4.4c）；
- 6 条 L3 开发闭环（`l3-run{2,3}-s{1,2,3}-*.json`，修复后重跑两轮，6/6 PASS，
  **携带引擎版本** `51503830c53b`）。

最新聚合产物 `docs/benchmarks/quality-metrics/2026-11-12-5150383.json`
（13 条样本：成功率 0.923 / 修复率 0.889 / 回归率 0.0 / 平均 82.7s /
**1673.0 token/任务**）。样本量仍小，且 L3 样本来自本地 3B 模型，不代表云端
大模型能力；后续以真实仓库开发任务继续扩充。

**版本归因**：13 条样本中 6 条带引擎版本（`engine_commit: 51503830c53b`），
7 条产出于版本字段引入之前（`engine_unknown: 7`）。前者可归因，后者**不可**——
比率变化可能来自样本构成而非引擎本身。

## 5. 关联

- `docs/plans/automation-development-plan-2026-11.md`（T4.1）
- `docs/plans/single-node-performance-baseline.md`
- `crates/sisyphus-metrics/src/quality.rs`（记录模型与聚合）
- `crates/sisyphus-metrics/src/engine_revision.rs`（引擎版本归因；CLI 与 bridge 共用）
- `crates/sisyphus-bridge/src/quality_record.rs`（bridge 执行记录落盘）
- `crates/sisyphus-cli/src/handle_metrics.rs`、`crates/sisyphus-cli/src/handle_verify.rs`
- `scripts/single-node-bench-sample.sh`
