# 单机 SOP 端到端指南（P1-E）

> 目标：在单机环境下完成 SOP 的**发现 → 查看 → 执行 → 观测**闭环，明确契约与前置条件。

## 1. 前置条件

- bridge 已启动且 HTTP/gRPC 可达（见 `single-node-operation-and-verification.md`）。
- 目标执行（`/api/execute/goal`）通常依赖 LLM/引擎配置；无配置时应返回可解释错误。
- 需要工作区隔离的端点需附加 `X-Sisyphus-Workspace-Id`（SDK/WebUI 已支持配置）。

## 2. 发现与查看

### 2.1 REST

```bash
# SOP 列表（返回 string[]）
curl -s http://127.0.0.1:8080/api/sops

# SOP 详情（含配置/图定义，按实现）
curl -s -H 'X-Sisyphus-Workspace-Id: <ws>' \
  http://127.0.0.1:8080/api/sops/<sop-id>

# DAG（WebUI/可视化用途）
curl -s http://127.0.0.1:8080/api/sops/<sop-id>/dag
```

### 2.2 CLI（经 bridge gRPC）

```bash
sisyphus --bridge http://127.0.0.1:50051 sop list
sisyphus --bridge http://127.0.0.1:50051 sop run <sop-id>
```

**实测（2026-11-10，单机 bridge）**：
- `sop list` → `verify_sop (2 nodes)`
- `sop run verify_sop` → 返回 `Result`（`Test` 节点为结构化 JSON，含 `status/success/exit_code/stdout` 与可读 `message`，如 `Test Passed: echo hi...`）与 `CognitionReport`（无 LLM 时 critique 不可用但 SOP 执行可完成）

> `sop run` 为直接 SOP 执行路径；执行效果依赖引擎/执行器配置。

## 3. 执行（目标/SOP）

### 3.1 `POST /api/execute/goal`

```bash
curl -s -X POST http://127.0.0.1:8080/api/execute/goal \
  -H 'Content-Type: application/json' \
  -d '{
    "goal": "完成示例目标",
    "sop_id": "<sop-id>",
    "project_id": "default"
  }'
# 预期：{"output": "...", "report": ...} 或可解释错误
```

**说明（实测）**：`execute/goal` 为目标驱动路径，可能触发 LLM 审计/推理；在无可用 LLM 配置时可能返回可解释错误（如模型不可用/审计失败）。与 CLI `sop run`（直接 SOP 执行）是不同路径，验收时需区分。

### 3.2 SDK

- TS: `executeSop(sopId, goal, projectId?, eventTx?)`
- Python: `execute_sop(sop_id, goal, project_id=None, event_tx=None)`
- Go: `ExecuteSOP(sopID, goal, projectID)`

示例见 `../examples/single-node-sdk-examples.md`。

### 3.3 组合执行与 SSE 订阅

- `POST /api/execute/composite` 返回 `sse_sop_id`（`composite-<uuid>`）；订阅事件时必须使用该值（见 `book/src/sse-events.md`、`book/src/rest-api.md`）。
- `POST /api/execute/goal` 的事件归属请求指定的 `sop_id`。

## 4. 观测与追踪

| 能力 | 端点/入口 | 说明 |
|---|---|---|
| 执行状态 | `GET /api/execute/goal/{goal_id}/status` | 当前状态 |
| 执行轨迹 | `GET /api/execute/goal/{goal_id}/trace` | 节点事件/轨迹 |
| 执行流 | `GET /api/execute/goal/{goal_id}/stream` | SSE 进度 |
| 认知追踪 | `/api/cognitive/trace` 等 | 决策/解释性数据（按启用） |
| 认知地图 | `/api/cognitive/map/:sop_id` 等 | 可视化与流式更新（按启用） |
| 集群只读 | `/api/cluster/status`、`/api/cluster/control/leases/{key}` | 单机默认控制面关闭，仅只读观测 |

## 5. 常见问题

- **400（缺工作区头）**：对需要工作区上下文的端点附加 `X-Sisyphus-Workspace-Id`。
- **执行失败但契约正确**：检查 LLM 配置、模型可用性、工具/MCP 依赖与权限。
- **事件未收到**：确认订阅使用正确的 `sop_id` / `sse_sop_id`，并检查 SSE 连接与过滤条件。
- **`AllIterationsPivoted without any real execution result` / StrategicPivot**：认知循环各轮均未产出可接受的真实执行结果。排查：节点命令/工具是否真实成功、`Test` 节点输出是否含显式 `status/success/exit_code`、预测/反思与 LLM 可用性、收敛阈值与监督策略。可先用 `sisyphus sop run <id>`（直接路径）与确定性命令模板验证执行链路，再排查目标驱动 `execute/goal` 依赖。
- **`execute/goal` 返回 LLM/审计相关错误**：目标驱动路径依赖模型与审计能力；缺配置时属可解释失败。先用 `sop run` 验证 SOP/DAG 可执行，补齐 LLM/凭据后重试。

## 6. 关联文档

- `single-node-typical-workflow.md`
- `single-node-contract-guard.md`
- `../examples/single-node-sdk-examples.md`
- `book/src/rest-api.md`、`book/src/sse-events.md`
