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

```bash
sisyphus verify --record docs/benchmarks/quality-metrics/records/<task>.json
```

聚合与采集：

```bash
sisyphus metrics quality --records docs/benchmarks/quality-metrics/records --json
./scripts/quality-metrics-collect.sh
```

无记录时各比率为 `null`（不伪造 0 值）。仓库真实任务样本需在具备 LLM 凭据的环境
（T1.4）执行后写入本目录。

## 目录内容

| 文件 | 来源 | 说明 |
|---|---|---|
| `repo-sisyphus-tools-build.json` | 真实仓库任务 | `sisyphus verify -p sisyphus-tools-build --max-warnings 0` 的真实运行（PASS，0 错 0 警，78.977s） |

受控 fixture 样本（用于验证口径与链路，非仓库任务）单独放在
`../fixture-samples/`，不混入本目录。
