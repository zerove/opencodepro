# 任务级质量记录目录

本目录存放**任务级质量记录**（每个文件一条 JSON），供 `sisyphus metrics quality` 聚合：

- 任务成功率 = pass / (pass + fail)（blocked 单独计数）
- 任务修复率 = 初次验证失败且最终通过 / 初次验证失败
- 任务回归率 = 引入回归 / 执行过修复
- 平均单任务耗时

## 记录格式

```json
{
  "task_id": "my-task",
  "scenario": "diagnose-fix",
  "status": "pass",
  "initial_verify_passed": false,
  "final_verify_passed": true,
  "fix_attempts": 1,
  "regressed": false,
  "duration_ms": 188,
  "recorded_at": "2026-11-11T00:00:00Z"
}
```

## 生成方式

两条链路都会写出同样格式的 `TaskRecord`：

```bash
# 1) 验证-修复循环（不涉及 LLM 规划）
sisyphus verify --record docs/benchmarks/quality-metrics/records/<task>.json

# 2) dev 闭环（LLM 规划 + 客观闸门）；--record 必须配 --verify-cmd
sisyphus dev "<目标>" \
  --verify-cmd "cargo test" \
  --record docs/benchmarks/quality-metrics/records/<task>.json \
  --scenario feature
```

`dev --record` 的语义：

- **闸门失败也会落记录**（`status: fail`）—— 失败样本是成功率的分母，不能只记成功；
- `initial_verify_passed` 是闸门**首轮**结果，`fix_attempts` 是闸门自动修复次数，
  二者共同定义修复率；
- `regressed` 在 `dev` 链路恒为 `false`（dev 不做跨任务回归检测，不伪造）。

`scripts/l3-run-scenarios.sh --records-dir <dir>` 会在三场景运行时自动落记录。

聚合与采集：

```bash
sisyphus metrics quality --records docs/benchmarks/quality-metrics/records --json
./scripts/quality-metrics-collect.sh
```

无记录时各比率为 `null`（不伪造 0 值）。

## 目录内容

| 文件 | 来源 | 说明 |
|---|---|---|
| `repo-sisyphus-tools-build.json` | 真实仓库任务 | `sisyphus verify -p sisyphus-tools-build --max-warnings 0`（PASS，0 错 0 警，78.977s） |
| `repo-sisyphus-types.json` | 真实仓库任务 | 同上（`sisyphus-types`，PASS，19.782s） |
| `repo-sisyphus-metrics.json` | 真实仓库任务 | 同上（`sisyphus-metrics`，PASS，46.028s） |
| `repo-sisyphus-tools-agent.json` | 真实仓库任务 | 同上（`sisyphus-tools-agent`，PASS，90.770s） |
| `l3-s1-diagnose-fix.json` | L3 闭环（本地 ollama `qwen2.5-coder:3b`） | `dev` 诊断修复场景（闸门 `cargo check`）：首轮 FAIL → 修复 1 次 → PASS，34.6s |
| `l3-s2-quality-gate.json` | L3 闭环（本地 ollama `qwen2.5-coder:3b`） | `dev` 质量门场景（闸门 `cargo clippy -- -D warnings`）：首轮 FAIL → 修复 1 次 → PASS，22.9s |
| `l3-s3-feature.json` | L3 闭环（本地 ollama `qwen2.5-coder:3b`） | `dev` TDD 跨文件场景（闸门 `cargo test`）：首轮即 PASS，0 修复，24.6s |

受控 fixture 样本（用于验证口径与链路，非仓库任务）单独放在
`../fixture-samples/`，不混入本目录。
