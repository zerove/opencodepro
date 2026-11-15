# Bridge `execute_goal` 成功路径 live E2E（clean build 归因）

> 日期：2026-11-15
> 引擎提交（记录内）：`42816f00abd4`，`engine_dirty=false`（**clean 树**）
> 目的：补一条 **clean 版本**的成功路径样本，替掉 2026-11-14 样本里
> `engine_dirty=true` 的归因缺口；同时验证执行级 token 作用域在成功路径上
> 仍能产出真实（非 null）用量。

## 1. 环境

- 二进制：`target/debug/sisyphus-bridge`（`cargo build -p sisyphus-bridge`，
  工作区干净；记录里的 `engine_commit`/`engine_dirty` 由
  `sisyphus-metrics::engine_revision` 在写入时探测）
- 本地模型：ollama `qwen2.5-coder:3b`
  （`http://127.0.0.1:11434/v1/chat/completions`）
- 配置：见 `runs/goal-success-path-clean/config.toml`
  （`request_timeout_secs=120`、`fast_fail_cooling_secs=10`、嵌入为 dummy）
- 端口：HTTP `18090` / gRPC `18051`（避开默认端口冲突）

启动方式：

```bash
SISYPHUS_CONFIG=<tmp>/config.toml \
SISYPHUS_BRIDGE_QUALITY_RECORDS_DIR=<tmp>/records \
setsid nohup ./target/debug/sisyphus-bridge > <tmp>/bridge.log 2>&1 &
```

## 2. 请求与响应

```bash
curl -sS -m 420 -X POST http://127.0.0.1:18090/api/execute/goal \
  -H 'content-type: application/json' \
  -d '{"goal":"process: run echo ok-bridge with bash_exec and return the output","sop_id":"goal-success-clean"}'
```

结果：**HTTP 200**，耗时 **258.5s**（`curl -w` 实测；模型为 CPU 推理，慢是预期）。
完整响应体见 `runs/goal-success-path-clean/http-response.json`：

```json
{"output":"{\"duration_ms\":4,\"exit_code\":0,\"stderr\":\"\",\"stdout\":\"\\n\"}",
 "report":"Plan: Strategic selection (Score: 1000.00): Single-node task: process | Report: 1 iterations, converged=false ..."}
```

`output` 是 `builtin:bash_exec` 的真实执行结果（`exit_code=0`），不是模型自述。

## 3. TaskRecord 与 token 归因

`runs/goal-success-path-clean/bridge-goal-goal-success-clean-1794706075733.json`：

| 字段 | 值 | 说明 |
|---|---|---|
| `status` | `pass` | 执行路径完成（**不表示外部验收通过**，见 §5） |
| `duration_ms` | 258645 | 与 curl 实测一致 |
| `prompt_tokens` / `completion_tokens` | **2348 / 952** | 执行作用域（task-local）归因，非全局差值 |
| `engine_commit` / `engine_dirty` | `42816f00abd4` / `false` | clean 树，归因可复现 |
| `fix_attempts` / `regressed` | 0 / false | bridge goal 无 dev 式客观闸门，语义见 `docs/plans/bridge-task-record-metrics.md` §3 |

聚合（`runs/goal-success-path-clean/aggregate.json`，命令
`sisyphus metrics quality --records <dir> --json`）：
`success_rate=1.0`、`mean_total_tokens=3300.0`、
`scenario_breakdown.bridge-goal-execute.success_rate=1.0`。

## 4. 与 2026-11-14 样本的关系

| 样本 | 引擎 | dirty | 结果 | tokens |
|---|---|---|---|---|
| `2026-11-14/runs/goal-success-path/` | `2cf57b0b7918` | true | pass | 2378 / 720 |
| `2026-11-15/runs/goal-success-path-clean/` | `42816f00abd4` | **false** | pass | 2348 / 952 |

两条样本**不可直接比较**（不同提交、不同模型随机性、单次样本）。clean 样本的
用途只有一个：证明 `engine_dirty=false` 时同样能跑通并产出非 null 用量。

## 5. 诚实性说明（这条记录**不**证明什么）

- `status=pass` 只表示**执行路径完成**且 `bash_exec` 返回 0；它**不是**外部
  验收结论，也不代表目标语义被正确理解。goal 文本是刻意选的英文 `process:`
  形式（触发 `GoalDrivenSynthesizer` 的单节点 `bash_exec`），避开 web 依赖。
- 单条样本，`success_rate=1.0` 只是「1/1」；不能推出稳定成功率。此前同一
  本地模型在中文 goal 上与其它场景有失败样本（见 `2026-11-13`、
  `2026-11-14` 归档）。
- 本次运行触发了 Swarm 审计循环与候选评估（bridge 日志可见
  `Starting Swarm Audit Loop` / `Candidate evaluation completed`），但**未**
  单独验证审计循环在成功路径上的判定质量。
- 未覆盖：云端 provider（本机无凭据，BLOCKED）、并发多请求下的归因隔离
  （仅有单测 `scope_usage_isolates_concurrent_scopes` 与 `usage_scope` 并发
  单测，缺 live 对照）。
