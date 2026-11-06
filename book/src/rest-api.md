# REST API 参考

所有端点由 Sisyphus Bridge HTTP 服务器提供服务（默认监听 `[::1]:8080`）。

OpenAPI 规范位于 [`/openapi.json`](/openapi.json)，也可通过 Swagger UI 访问：[`/swagger-ui`](/swagger-ui)。

## 认证

包含 `Authorization: Bearer <token>` 请求头。token 在 `config.auth_token` 中配置。

## 健康检查

### `GET /health`

```json
{ "status": "SERVING", "version": "0.3.0" }
```

## 工具执行

### `POST /api/execute`

按工具 ID 执行工具。请求体：

```json
{ "tool_id": "file_read", "args_json": "{\"path\": \"/tmp/test.txt\"}", "fuel_limit": 1000000 }
```

## 目标执行

### `POST /api/execute/goal`

通过 SOP 执行认知目标。请求体：

```json
{ "goal": "Analyze code", "sop_id": "code-review", "project_id": "my-project" }
```

### `GET /api/execute/goal/{goal_id}/status`

获取当前执行状态。

### `GET /api/execute/goal/{goal_id}/trace`

获取含每节点事件的完整执行轨迹。

### `GET /api/execute/goal/{goal_id}/stream`

执行进度的 Server-Sent Events 流。

### `POST /api/execute/composite`

将多个目标作为组合 DAG 执行。请求体：

```json
{ "goals": ["analyze", "test", "deploy"], "project_id": "my-project" }
```

响应体含 `sse_sop_id`（形如 `composite-<uuid>`）。复合执行内部各图的 `sop_id` 由内核生成、在此不可见，因此订阅 SSE 事件时必须用 `sse_sop_id` 作为 `?sop_id=` 的值，详见 [SSE 事件](sse-events.md#执行端点与事件订阅)。

`sse_sop_id` **无论执行成功或失败都会回传**（失败时与 `error` 并列）。因为该标识由服务端生成、客户端无从推算，而执行期间的事件已经广播到 SSE —— 此时不告知订阅 id，客户端就会陷入「收到了事件却不知道该订阅哪个 sop_id」的困境。

> 对比 `POST /api/execute/goal`：它的 `sop_id` 由客户端在请求中指定，失败时客户端本就知道该订阅什么，因此失败响应无需回传。

## 会话

### `GET /api/sessions`

列出所有会话。

### `POST /api/sessions`

创建新会话。请求体：

```json
{ "name": "my-session" }
```

### `GET /api/sessions/{id}`

获取会话详情（含消息）。

### `POST /api/sessions/{id}/messages`

向会话添加消息。请求体：

```json
{ "role": "user", "content": "Hello" }
```

## 工具与代理

### `GET /api/tools`

列出已注册工具。

### `GET /api/agents`

列出已注册代理。

## 技能

### `GET /api/skills`

列出已安装技能条目。

### `GET /api/skills/available`

列出可用远程技能。

### `POST /api/skills/install`

安装远程技能。请求体：

```json
{ "skill_id": "code-reviewer" }
```

### `GET /api/skills/installed`

列出已安装的远程技能。

### `DELETE /api/skills/remote/{id}`

卸载远程技能。

## 工作区

### `GET /api/workspaces`

列出所有工作区。

### `POST /api/workspaces`

创建工作区。请求体：

```json
{ "name": "dev", "project_id": "my-project", "root_dir": "/home/user/project" }
```

### `POST /api/workspaces/switch`

切换当前工作区。请求体：

```json
{ "id": "ws-1" }
```

### `GET /api/workspaces/current`

获取当前活动工作区。

## SOP 管理

### `GET /api/sops`

列出当前工作区的 SOP。传递 `X-Sisyphus-Workspace-Id` 请求头。

### `POST /api/sops`

保存或创建 SOP。请求体：

```json
{ "id": "review-sop", "name": "Code Review", "config": { "nodes": [...], "edges": [...] } }
```

### `GET /api/sops/{id}`

获取 SOP 详情（含 DAG 配置）。

### `DELETE /api/sops/{id}`

删除 SOP。

## 认知地图

### `GET /api/cognitive/map/{sop_id}`

获取运行中 SOP 的认知地图。

## 经验引擎

### `POST /api/experience/similar`

按语义相似度查询相似经验。请求体：

```json
{ "query_text": "error during build", "action_id": "compile", "limit": 5 }
```

## 联邦策略权重

### `GET /api/strategy/weights`

返回集群级策略权重聚合视图（ADR-017）。每个节点按
`cluster.strategy_sync_interval_secs` 周期发布本地 `MetaLearner` 的策略统计，
对端按 `generation` 合并；本端点读取的是合并后的共享视图。

响应：

```json
{
  "node_count": 2,
  "nodes": [
    { "node_id": "node-a", "generation": 1762345678901,
      "strategies": { "depth-first": { "uses": 2, "successes": 2, "avg_duration_secs": 1.5 } } },
    { "node_id": "node-b", "generation": 1762345678950,
      "strategies": { "depth-first": { "uses": 1, "successes": 0, "avg_duration_secs": 3.0 } } }
  ],
  "aggregate": {
    "depth-first": { "uses": 3, "successes": 2, "success_rate": 0.6667, "avg_duration_secs": 2.0 }
  },
  "best": { "strategy": "depth-first", "success_rate": 0.6667, "uses": 3 }
}
```

- 未启用集群或尚无节点上报时：`node_count = 0`、`nodes = []`、
  `aggregate = {}`、`best = null`（不是 404）。
- `aggregate` 的 `uses` / `successes` 是**跨节点求和**，因此不等于集群里
  真实执行次数（同一策略在 3 个节点各跑 10 次会显示 30）。
- `best` 由 `best_strategy(1)` 计算：`min_uses = 1`，成功率相同时按策略名
  字典序取前。它只是展示，执行路径尚未消费该推荐。
- **`nodes` 只含当前已知的节点**：收到 `PeerLeft`（对端断流或 TTL 过期）时该
  节点的槽位会被**丢弃**，而不是标记离线。因此分区期间 `node_count` 与
  `aggregate` 会**先变小**，重连后由该节点下一拍快照加回。视图回答的是
  「当前集群有哪些策略有效」，不是历史流水。
- 节点重启后槽位从初始 `generation` 重新开始，不会因为历史 `generation`
  更大而永久被拒。

## 路线图

### `GET /api/roadmap/project/{id}`

获取项目路线图详情。

### `POST /api/roadmap/project/{id}`

创建新项目。请求体：

```json
{ "id": "my-project", "name": "My Project", "description": "..." }
```

### `POST /api/roadmap/milestone`

向项目添加里程碑。请求体：

```json
{ "project_id": "my-project", "milestone": { "id": "m1", "title": "Alpha", "status": "pending" } }
```

### `POST /api/roadmap/phase`

向里程碑添加阶段。请求体：

```json
{ "project_id": "my-project", "milestone_id": "m1", "phase": { "id": "p1", "title": "Design" } }
```

### `PATCH /api/roadmap/goal/{goal_id}`

更新目标状态。

### `GET /api/roadmap/health/{project_id}`

评估项目健康分数。

## 异常观测

### `GET /api/anomaly?sop_id={sop_id}`

读取认知循环观测到的异常（来源：`StrategicManager.anomaly_log`，有界 256 条）。

- `sop_id` 可省略：省略时返回全部作用域；未知作用域返回空数组（不是错误）。

响应为数组：

```json
[
  {
    "node_name": "node_compile",
    "severity": "high",
    "metric": "duration",
    "observed": 42.0,
    "expected": 10.0,
    "description": "duration deviation",
    "timestamp": 1700000000
  }
]
```

`severity` 取值：`low` / `medium` / `high` / `critical`。

## 经验共享

### `POST /api/experience/share`

```json
{ "exp_key": "exp:1700000000" }
```

响应：`{ "success": true, "error": "" }`。

### `POST /api/experience/unshare`

`project_id` **必填**（缺失返回 400），用于限定取消共享的作用域：

```json
{ "exp_key": "exp:1700000000", "project_id": "proj-1" }
```

### `GET /api/experience/shared`

响应：`{ "experiences": [ ... ] }`（`ExperienceTuple` 数组）。

## 指标

### `GET /api/metrics`

Prometheus 文本格式（`text/plain; version=0.0.4`）。除 HTTP / SSE / chaos 计数外，还暴露认知循环与执行器的实时 gauge：

| 指标 | 类型 | 说明 |
|--------|------|-------------|
| `sisyphus_anomaly_log_entries` | gauge | 异常日志当前保留条数 |
| `sisyphus_token_budget_usage` | gauge | LLM 阶段累计消耗的估算 token |
| `sisyphus_token_budget_capped` | gauge | 是否配置了 token 上限（1/0） |
| `sisyphus_token_budget_exhausted` | gauge | 剩余预算是否低于保留阈值（1/0） |
| `sisyphus_token_budget_remaining` | gauge | 距上限的剩余 token（仅配置上限时出现） |
| `sisyphus_executor_concurrency_limit` | gauge | 节点并发上限（支持热重载） |
| `sisyphus_executor_available_permits` | gauge | 执行器信号量中的空闲许可 |
