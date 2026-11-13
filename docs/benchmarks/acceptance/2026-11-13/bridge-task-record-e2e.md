# Bridge TaskRecord 落盘 E2E 证据（execute_goal，最小实现）

> 日期：2026-11-13  
> 引擎提交（记录内）：`75055e2b3bf8`（clean）  
> 范围：验证 `POST /api/execute/goal` 在启用 `SISYPHUS_BRIDGE_QUALITY_RECORDS_DIR` 时可产出可聚合的 `TaskRecord`（best-effort 落盘）。本次为**失败路径**样本（环境无可用 LLM，非记录功能缺陷）。

## 环境与步骤

- 二进制：`target/debug/sisyphus-bridge`（含 `feat(bridge): execute_goal 最小接线 TaskRecord 落盘（opt-in）`，已 `cargo build -p sisyphus-bridge`）
- 临时配置：`allow_dummy_embeddings = true`，LLM 指向本地不可用端点（`request_timeout_secs=5`）
- 启动：`SISYPHUS_CONFIG=<tmp>/config.toml SISYPHUS_BRIDGE_QUALITY_RECORDS_DIR=<tmp>/records ./target/debug/sisyphus-bridge`
- 请求：

```bash
curl -sS -X POST http://127.0.0.1:15492/api/execute/goal \
  -H 'content-type: application/json' \
  -d '{"goal":"e2e record test","sop_id":"e2e-sop"}'
```

## 观测结果

HTTP 响应（预期失败，LLM 不可用）：

```json
{"error":"Internal error: Swarm audit failed: Internal error: LLM call failed for agent architect-0: Internal system error: Model qwen2.5-coder:3b unavailable (fast-fail cooling, previous call failed)","hint":"目标驱动路径可能依赖 LLM/审计能力：补齐模型配置与凭据，或先用 `sop run` 验证 SOP/DAG 执行。"}
```

落盘记录（`bridge-goal-e2e-sop-1794530078079.json`）：

```json
{
  "task_id": "bridge-goal-e2e-sop-1794530078079",
  "scenario": "bridge-goal-execute",
  "status": "fail",
  "initial_verify_passed": false,
  "final_verify_passed": false,
  "fix_attempts": 0,
  "regressed": false,
  "duration_ms": 5014,
  "prompt_tokens": null,
  "completion_tokens": null,
  "engine_commit": "75055e2b3bf8",
  "engine_dirty": false,
  "recorded_at": "2026-11-13T00:34:38.079419793+00:00"
}
```

聚合（`sisyphus metrics quality --records <tmp>/records --json`）：

```json
{
  "total": 1,
  "passed": 0,
  "failed": 1,
  "blocked": 0,
  "success_rate": 0.0,
  "fix_rate": 0.0,
  "regression_rate": null,
  "mean_duration_ms": 5014.0,
  "mean_prompt_tokens": null,
  "mean_completion_tokens": null,
  "mean_total_tokens": null,
  "engine_commits": ["75055e2b3bf8"],
  "engine_unknown": 0
}
```

## 结论与限制

- **结论**：bridge `execute_goal` 路径在失败分支可产出 schema 正确、可被 `sisyphus metrics quality` 聚合的 `TaskRecord`，且包含引擎版本归因与耗时。
- **限制**：本次未获得成功路径（`Pass`）的 live 记录（需可执行的 goal/LLM 环境）；token usage 为 `null`；未覆盖 composite/gRPC/streaming 路径。`fix_rate=0.0` 源于无闸门语义下 `initial/final` 同为失败且计入初次失败样本的聚合表现，需结合 `scenario=bridge-goal-execute` 解读（见设计文档口径约束）。
- **后续**：gRPC `execute_sop` live E2E（fail 路径）已于 `bridge-task-record-coverage.md` 归档；`execute_goal` 成功路径、gRPC `stream_execution`/`call_jcode` 与 token usage 提取仍待后续。
