# Bridge TaskRecord 覆盖扩展 E2E 证据（composite / streaming / unary）

> 日期：2026-11-13
> 引擎提交（记录内）：`02ce4abf94aa`，`engine_dirty=true`（运行时工作区含未提交的覆盖扩展改动；记录如实标记，不冒充干净版本）
> 范围：验证新接线的 bridge 执行入口可产出可聚合 `TaskRecord`：
> HTTP composite（成功路径）、HTTP goal streaming（失败路径）、HTTP unary goal（失败路径）。
> gRPC `execute_sop` / `stream_execution` / `call_jcode` 已接线但**未做 live E2E**（环境无 grpcurl / 可用执行目标）。

## 环境与步骤

- 二进制：`target/debug/sisyphus-bridge`（`cargo build -p sisyphus-bridge`，含覆盖扩展改动）
- 临时配置：`[llm.embedding].allow_dummy_embeddings = true`；LLM 指向本机不可用端点（`request_timeout_secs=5`）
- 启动：

```bash
SISYPHUS_CONFIG=<tmp>/config.toml \
SISYPHUS_BRIDGE_QUALITY_RECORDS_DIR=<tmp>/records \
./target/debug/sisyphus-bridge
```

- 请求：

```bash
# composite（成功路径）
curl -sS -X POST http://127.0.0.1:15493/api/execute/composite \
  -H 'content-type: application/json' \
  -d '{"goals":["alpha goal","beta goal"]}'

# goal streaming（失败路径：无可用 LLM）
curl -sS -N --max-time 20 http://127.0.0.1:15493/api/execute/goal/stream-e2e-goal/stream
```

## 观测结果

### 1) HTTP composite —— 成功路径记录

响应（截断；`results` 为各 goal 执行输出）：

```json
{"execution_order":["goal_1","goal_0"],"results":{"goal_0":"{\"content\":\"Example Domain...\"}","goal_1":"{\"content\":\"Example Domain...\"}"},"sse_sop_id":"composite-790ad63f-68bd-4d1d-9765-f51482b53a7f"}
```

落盘记录 `bridge-composite-composite-790ad63f-...-1794533838054.json`：

```json
{
  "task_id": "bridge-composite-composite-790ad63f-68bd-4d1d-9765-f51482b53a7f-1794533838054",
  "scenario": "bridge-composite-execute",
  "status": "pass",
  "initial_verify_passed": true,
  "final_verify_passed": true,
  "fix_attempts": 0,
  "regressed": false,
  "duration_ms": 32388,
  "prompt_tokens": null,
  "completion_tokens": null,
  "engine_commit": "02ce4abf94aa",
  "engine_dirty": true,
  "recorded_at": "2026-11-13T01:37:18.054109586+00:00"
}
```

### 2) HTTP goal streaming —— 失败路径记录

该环境无可用 LLM，流式执行立即失败（SSE 无输出）；记录仍产出：

```json
{
  "task_id": "bridge-goal-stream-stream-e2e-goal-1794533838210",
  "scenario": "bridge-goal-streaming",
  "status": "fail",
  "initial_verify_passed": false,
  "final_verify_passed": false,
  "fix_attempts": 0,
  "regressed": false,
  "duration_ms": 32,
  "prompt_tokens": null,
  "completion_tokens": null,
  "engine_commit": "02ce4abf94aa",
  "engine_dirty": true,
  "recorded_at": "2026-11-13T01:37:18.210121095+00:00"
}
```

### 3) HTTP unary goal —— 失败路径记录（补充）

同一环境对 `POST /api/execute/goal` 依次发送 3 个 goal，均因 LLM fast-fail 失败；
3 条记录场景均为 `bridge-goal-execute`（`fail`，`5096ms` / `50ms` / `37ms`）。
原始记录见 `runs/bridge-record-coverage/unary-goal/`。

## 聚合

```bash
sisyphus metrics quality --records <tmp>/records --json
```

（composite + streaming 两次请求）

```json
{
  "total": 2,
  "passed": 1,
  "failed": 1,
  "blocked": 0,
  "success_rate": 0.5,
  "fix_rate": 0.0,
  "regression_rate": null,
  "mean_duration_ms": 16210.0,
  "mean_prompt_tokens": null,
  "mean_completion_tokens": null,
  "mean_total_tokens": null,
  "engine_commits": ["02ce4abf94aa+dirty"],
  "engine_unknown": 0
}
```

## 结论与限制

- **结论**：新接线入口在真实 bridge 运行中产出 schema 正确、可被 `sisyphus metrics quality`
  聚合的记录；composite 成功与 streaming/unary 失败路径均有样本，场景标签可区分。
- **`Pass` 语义**：仅表示执行路径完成，**不是**外部客观验收。本次 composite 的合成计划
  实际抓取了 `https://example.com` 的内容 —— 与输入 goal 的意图未必一致；这类执行语义
  由既有合成/执行逻辑决定，本记录只反映其终态。
- **未覆盖**：gRPC 三个入口未做 live E2E（无 grpcurl、无可用执行目标）；HTTP unary
  `execute_goal` 的成功路径仍缺（需要可用 LLM/审计环境）。
- **token usage** 为 `null`（无可得 provider usage，不伪造 0）。
- `fix_rate=0.0` 来自“初次失败且最终失败”样本的既有聚合口径；bridge 无自动修复循环，
  该值不可解读为修复能力。
- 原始记录与聚合输出归档于 `runs/bridge-record-coverage/`。
