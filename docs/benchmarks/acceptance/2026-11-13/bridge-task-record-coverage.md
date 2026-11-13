# Bridge TaskRecord 覆盖扩展 E2E 证据（composite / streaming / unary / gRPC）

> 日期：2026-11-13
> 引擎提交（记录内）：`02ce4abf94aa`，`engine_dirty=true`（运行时工作区含未提交的覆盖扩展改动；记录如实标记，不冒充干净版本）
> 范围：验证新接线的 bridge 执行入口可产出可聚合 `TaskRecord`：
> HTTP composite（成功路径）、HTTP goal streaming（失败路径）、HTTP unary goal（失败路径）、
> gRPC `execute_sop`（失败路径，LLM 能力边界）。
> gRPC `stream_execution` / `call_jcode` 已接线但**未做 live E2E**（环境无 grpcurl / 可用执行目标）。
> 另记录一次「本地 LLM 可用但模型能力不足」的 unary 失败样本，证明记录链路在真实执行下仍如实落盘。

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

### 4) HTTP unary goal —— 有可用本地 LLM 时仍失败（模型能力边界）

后续确认本机 ollama `qwen2.5-coder:3b` 可用后，以宽超时（`request_timeout_secs=240`）
重试 `POST /api/execute/goal`（goal：`读取 README.md 的前 5 行并原样输出`）：

- 执行耗时 **133877ms** 后失败：`Swarm audit failed: Failed to parse proposal JSON:
  invalid type: map, expected a tuple of size 2`；
- 3B 模型生成的 SOP 节点 `args` 形态不符合 `SOPGraphConfig` 反序列化要求，
  属**模型能力边界**，不是记录链路问题；
- 记录如实落盘：`status=fail`、`engine_commit=3977830110a8`、`engine_dirty=false`
  （此时工作区已提交干净），见
  `runs/bridge-record-coverage/unary-goal-with-llm/`。

> 失败样本被保留（失败是成功率分母），不因「想要成功样本」而丢弃。

### 5) gRPC `execute_sop` —— live E2E（失败路径，LLM 能力边界）

环境就绪后，通过 CLI 的 gRPC `execute_sop` 入口（`sisyphus --bridge <addr> sop run <sop>`）调起
真实 bridge，并先经 REST `POST /api/sops` 保存一个确定性的 `Test` 节点 SOP
（`grpc_sop_e2e_smoke`，`echo grpc-sop-ok`），随后由 gRPC 执行：
（执行过程走到 `LLM reflection failed: Failed to parse LLM reflection JSON`，
属模型能力边界；**记录链路本身照常产出并聚合通过**）

- 结果：`ExecuteSOP RPC failed`，HTTP 500 返回；
- 落盘记录 `bridge-sop-grpc_sop_e2e_smoke-...-1794538467075.json`（场景
  `bridge-sop-execute`，`status=fail`，`duration_ms=49784`）；
  当前引擎版本归因 `engine_commit=44085aef998e`、`engine_dirty=true`；
- 聚合（该样本）：

```json
{
  "total": 1,
  "passed": 0,
  "failed": 1,
  "blocked": 0,
  "success_rate": 0.0,
  "fix_rate": 0.0,
  "regression_rate": null,
  "mean_duration_ms": 49784.0,
  "mean_prompt_tokens": null,
  "mean_completion_tokens": null,
  "mean_total_tokens": null,
  "engine_commits": ["44085aef998e+dirty"],
  "engine_unknown": 0
}
```

原始记录与聚合输出归档于 `runs/bridge-record-coverage/grpc-sop/`。

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
  聚合的记录；composite 成功与 streaming/unary/gRPC 失败路径均有样本，场景标签可区分。
- **`Pass` 语义**：仅表示执行路径完成，**不是**外部客观验收。本次 composite 的合成计划
  实际抓取了 `https://example.com` 的内容 —— 与输入 goal 的意图未必一致；这类执行语义
  由既有合成/执行逻辑决定，本记录只反映其终态。
- **未覆盖**：gRPC `stream_execution` / `call_jcode` live E2E；HTTP unary `execute_goal` 的成功
  路径仍缺（需要能产出合法 SOP JSON 的模型/可用云端凭据）。
  `qwen2.5-coder:3b` 在有 LLM 的情况下仍因 proposal/reflection JSON 解析失败（见上），
  `gemma4:e4b` 在本机 CPU 上 290s 内无响应，均不足以完成成功路径验收。
- **token usage**：`209e4d2` 后已接线 best-effort 独占租约采集。初次 live 探测（HTTP unary goal / gRPC `execute_sop` / composite，clean build）记录仍为 `null`（归档 `runs/bridge-record-coverage/token-probe/`），但后续对 HTTP `execute_goal` 的独占 live 运行（本机 ollama `qwen2.5-coder:3b`，执行因模型 proposal JSON 解析失败；运行时构建含临时观测日志，故记录标记 `engine_dirty=true`）成功采集并落盘 **prompt_tokens=538 / completion_tokens=427 / total=965**（`calls=2` 的累计差值），聚合 `mean_*_tokens` 正确（`runs/bridge-record-coverage/token-captured/`，`sisyphus metrics quality --json`）。**结论**：机制可在 provider 上报 usage 且租约独占时产出真实增量；无 usage/非独占/无法归因时仍按口径保持 `null`（未知 ≠ 0，不伪造）。(delta 单测 2 项 + quality_record 6 passed)
- `fix_rate=0.0` 来自“初次失败且最终失败”样本的既有聚合口径；bridge 无自动修复循环，
  该值不可解读为修复能力。
- 原始记录与聚合输出归档于 `runs/bridge-record-coverage/`（含 `grpc-sop/`、`token-probe/` 等子目录）。
