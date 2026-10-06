# REST API 参考

所有端点由 Sisyphus Bridge HTTP 服务器提供服务（默认监听 `[::1]:8080`）。

OpenAPI 规范位于 [`/openapi.json`](/openapi.json)，也可通过 Swagger UI 访问：[`/swagger-ui`](/swagger-ui)。

## 认证

包含 `Authorization: Bearer <token>` 请求头。token 在 `config.auth_token` 中配置。

## 健康检查

### `GET /health`

```json
{ "status": "SERVING", "version": "0.1.0" }
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