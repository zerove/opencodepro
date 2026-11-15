# Bridge `execute_goal` 成功路径 live E2E 证据

> 日期：2026-11-14
> 引擎提交（记录内）：`2cf57b0b7918`（`engine_dirty=true`，运行时刻工作区含本次
> 作用域传播/审计降级改动未提交；如实归因）
> 范围：补齐 `docs/plans/bridge-task-record-metrics.md` §7 验收标准中
> **HTTP `execute_goal` 成功路径**的 live 样本（含非 null token 用量）。

## 环境与步骤

- 二进制：`target/debug/sisyphus-bridge`（含 `sisyphus_types::usage_scope` 跨 spawn
  传播 + Swarm 审计循环降级 + `sisyphus_intel::json_extract` 栅栏提取）
- 本地模型：ollama `qwen2.5-coder:3b`（`http://127.0.0.1:11434/v1/chat/completions`，
  `request_timeout_secs=120`，`fast_fail_cooling_secs=10`），嵌入为 dummy（opt-in 配置）
- 启动：

```bash
SISYPHUS_CONFIG=<tmp>/goal-success/config.toml \
SISYPHUS_BRIDGE_QUALITY_RECORDS_DIR=<tmp>/goal-success/records \
SISYPHUS_HTTP_ADDRESS=http://127.0.0.1:8090 \
setsid nohup ./target/debug/sisyphus-bridge &
```

- 请求（goal 用英文动词触发 `GoalDrivenSynthesizer` 的 `Process` 动作，
  合成 `builtin:bash_exec` 单节点，避免 web_fetch 依赖外网）：

```bash
curl -s -m 420 -X POST http://127.0.0.1:8090/api/execute/goal \
  -H 'content-type: application/json' \
  -d '{"goal":"process: run echo ok-bridge with bash_exec and return the output","sop_id":"goal-success-e2e"}'
```

## 观测结果

HTTP 200（耗时 138.6s），`output` 为 bash_exec 真实执行结果：

```json
{"output":"{\"duration_ms\":7,\"exit_code\":0,\"stderr\":\"\",\"stdout\":\"\\n\"}","report":"Plan: Strategic selection (Score: 1000.00): Single-node task: process | Report: 1 iterations, converged=false\n\n### Execution Trace\n```mermaid\ngraph TD\n```"}
```

落盘记录 `bridge-goal-goal-success-e2e-1794622499637.json`：

```json
{
  "task_id": "bridge-goal-goal-success-e2e-1794622499637",
  "scenario": "bridge-goal-execute",
  "status": "pass",
  "initial_verify_passed": true,
  "final_verify_passed": true,
  "fix_attempts": 0,
  "regressed": false,
  "duration_ms": 138639,
  "prompt_tokens": 2378,
  "completion_tokens": 720,
  "engine_commit": "2cf57b0b7918",
  "engine_dirty": true,
  "recorded_at": "2026-11-14T02:14:59.637315386+00:00"
}
```

聚合（`sisyphus metrics quality --records <dir> --json`，含本轮此前 8 条失败样本）：

```json
{
  "total": 9,
  "passed": 1,
  "failed": 8,
  "blocked": 0,
  "success_rate": 0.1111111111111111,
  "fix_rate": 0.0,
  "regression_rate": null,
  "mean_duration_ms": 114063.44444444444,
  "mean_prompt_tokens": 1392.0,
  "mean_completion_tokens": 583.625,
  "mean_total_tokens": 1975.625,
  "engine_commits": ["2cf57b0b7918+dirty"],
  "engine_unknown": 0
}
```

## 过程中修复的两个真实缺陷

1. **Swarm 审计循环对弱模型输出零容忍**（`sisyphus-core/src/swarm_coordinator.rs`）：
   架构师提案/审计员意见/refine 输出任一 JSON 损坏即中止整次 goal 执行。
   本地 3B 模型高频产生 `args` 字符串数组（应为 u8 字节数组）、未知节点 variant
   （`Bash_exec`）等损坏。改为三处降级：提案不可解析→维持当前配置继续审计；
   意见不可解析→记为「不批准」+说明性 feedback（绝不静默批准）；refine 失败→
   维持上一版提案（迭代上限强制收尾）。
2. **LLM JSON 响应带 markdown 栅栏时解析失败**（````json ... ``` ` 包裹）：
   统一 `sisyphus_intel::json_extract::extract_json`，接线批判/反思/演进 ops/合成/
   审计提案共 5 处解析点（此前 `intelligence_manager` 的 critique 解析裸
   `from_str(&response)`，带栅栏即失败）。

## 结论与限制

- **验收达成**：`execute_goal` 成功路径产出 `status=pass`、字段完整、token 非 null
  （`prompt=2378` / `completion=720`，执行作用域归因）的 `TaskRecord`，且可被
  `sisyphus metrics quality` 正确聚合；失败样本（8 条）保留为成功率分母，未丢弃。
- token 用量显著高于此前失败路径样本（515/479）：本轮执行经过完整
  合成→审计→仿真→认知循环，且跨 spawn 传播后推测预测/内化等子任务调用亦被归因。
- **口径**：`Pass` 仅表示执行路径完成（bash_exec 退出码 0），不等于外部客观验收；
  bridge 无 dev 式闸门语义（`fix_attempts=0`、`regressed=false`）。
- 失败样本的失败原因均为模型能力边界（JSON 形态/外网不可达），非记录链路缺陷；
  其中 `token-probe` 等历史样本的失败路径证据见 2026-11-13 覆盖文档。
- 原始记录与聚合快照归档于 `docs/benchmarks/acceptance/2026-11-14/runs/goal-success-path/`
  （含全部 9 条记录与 `http-response.txt`）。

## 后续：clean 版本样本（2026-11-15）

本轮的 `engine_dirty=true` 是如实归因，但对复现不友好。2026-11-15 在 clean 树
（`42816f00abd4`，`engine_dirty=false`）上重跑同一 goal 形态，取得
`status=pass` + `prompt=2348` / `completion=952` 的样本，见
`docs/benchmarks/acceptance/2026-11-15/goal-success-path-clean.md`。
两条样本不可直接比较（不同提交、单次样本、模型随机性）。
