# Bridge `execute_goal` 成功路径 — clean 树多样本实跑（2026-11-15）

> 日期：2026-11-15
> 引擎：clean 树（`02835d956735`，`engine_dirty=false`），二进制由
> `cargo build -p sisyphus-bridge` 产出（HEAD `02835d9`）。
> 目标：把 `docs/benchmarks/acceptance/2026-11-15/goal-success-path-clean.md`
> 的 **1/1** 样本扩展为 **3/3** clean 样本，给出「成功路径在 clean 树上」的粗略
> success_rate 区间（**不冒烟稳定性结论**）。

## 1. 实跑概览

| # | sop_id | 启动/结束 | duration_ms | status | prompt/completion tokens | converged |
|---|---|---|---|---|---|---|
| 1 | `goal-success-clean` | clean 树 `42816f00abd4`（dirty） | 258645 | pass | 2348 / 952 | false |
| 2 | `goal-success-r2` | clean 树 `02835d956735` | 188890 | pass | 2551 / 1056 | true |
| 3 | `goal-success-r3` | clean 树 `02835d956735` | 174477 | pass | 2357 / 1051 | false |
| 4 | `goal-success-r4` | clean 树 `02835d956735` | 134356 | pass | 2389 / 797 | false |

> 样本 1 来自 `2026-11-15/goal-success-path-clean`（前一次本轮 clean 实跑）；
> 样本 2–4 来自本节多样本实跑（`runs/goal-success-clean-multi/`）。

## 2. 聚合（`runs/goal-success-clean-multi/aggregate.json`，命令
`sisyphus metrics quality --records <dir> --json`，共 **3** 条样本）

```json
{
  "total": 3, "passed": 3, "failed": 0, "blocked": 0, "success_rate": 1.0,
  "mean_duration_ms": 165907.67,
  "mean_prompt_tokens": 2432.33, "mean_completion_tokens": 968.0, "mean_total_tokens": 3400.33,
  "engine_commits": ["02835d956735"], "engine_unknown": 0,
  "scenario_breakdown": {"bridge-goal-execute": {"total": 3, "passed": 3, "failed": 0, "blocked": 0, "success_rate": 1.0}}
}
```

## 3. 结论（诚实口径）

- **3/3 clean 样本成功**（`status=pass`，`bash_exec` exit 0，token 非 null）。
- clean 树上 `mean_total_tokens ≈ 3400.33` token/任务（样本内范围 3186–3607），与
  失败路径样本（515/479）显著不同 —— 完整合成→执行链路的 token 成本已验证可达。
- 但 **N=3，success_rate=1.0 只表示 3/3，不构成稳定性结论**；converged 值在样本间
  波动（true / false），说明成功≠收敛。
- 样本之间耗时 134s–189s（qwen2.5-coder:3b CPU 推理），未做统计显著性检验（N 过小，
  按本项目口径不冒烟）。

## 4. 与单样本的关系

- 1/1（clean `42816f00abd4`，tokens 2348/952）与 3/3 clean（`02835d956735`，mean 3400）：
  **不可直接比较**（不同提交/模型随机性/单次样本）。
- 本节多样本用于验证「clean 树上执行级 token 归因成功路径产出真实 token」；仍不改变
  「3/3 ≠ 稳定」的边界。
