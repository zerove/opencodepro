# 单机会话与任务流指南（P1-A）

> 目标：在单机场景下清晰区分并使用**本地 CLI 会话**与**bridge 服务端会话/执行**，形成一致的任务流体验。

## 1. 两类会话模型

| 类型 | 存储/作用域 | 典型入口 | 适用场景 |
|---|---|---|---|
| **CLI 本地会话** | 本地数据目录（`--data-dir`，如 `.sisyphus-data`） | `sisyphus session create/list/get/add-message/export/...` | 离线记录、脚本化管理、快速本地流转 |
| **Bridge 服务端会话** | bridge 侧会话存储（经 REST/gRPC） | `GET/POST /api/sessions*`、WebUI `/sessions`、SDK 会话方法 | 与执行/观测/多端接入联动的服务端状态 |

> 注意：两者是不同存储与接口面；选择时以“是否需要 bridge/多端共享/执行关联”为准。

## 2. CLI 本地会话（无 bridge）

```bash
# 指定数据目录（推荐显式指定，便于隔离）
sisyphus --data-dir ./.sisyphus-data session create "demo"
sisyphus --data-dir ./.sisyphus-data session list
sisyphus --data-dir ./.sisyphus-data session get <id>
sisyphus --data-dir ./.sisyphus-data session add-message <id> user "hello"
sisyphus --data-dir ./.sisyphus-data session export <id> ./session.json
sisyphus --data-dir ./.sisyphus-data session import ./session.json
```

**验收（已验证）**：`create/list` 在本地数据目录下可用（S2 基线）。

### 2.1 会话回放页（HTML）

`session export <id> <out.html> --format html` 产出**离线单文件**回放页，用于事后复盘与分享：

```bash
sisyphus --data-dir ./.sisyphus-data session export <id> ./replay.html --format html
```

页面能力：时间轴序号 + 角色着色、`<tool_call>`/```tool 折叠、```diff 着色、超长消息折叠、
`metadata` 中 `trace*`/`sop*` 键渲染为关联徽标；所有内容 HTML 转义（离线、无脚本）。

**断言方式**（无需浏览器）：`python3` 检查导出文件包含 `<title>Session: <name></title>`、
`#1/N`、`&lt;script&gt;`（转义），且不含 `http://`/`https://`/`<script`/`<link `。

## 3. Bridge 服务端会话（需 bridge）

REST 典型流程（契约以实现/OpenAPI 为准）：

```bash
# 创建
curl -s -X POST http://127.0.0.1:8080/api/sessions \
  -H 'Content-Type: application/json' -d '{"name":"demo"}'

# 列表 / 详情
curl -s http://127.0.0.1:8080/api/sessions
curl -s http://127.0.0.1:8080/api/sessions/<id>

# 追加消息
curl -s -X POST http://127.0.0.1:8080/api/sessions/<id>/messages \
  -H 'Content-Type: application/json' -d '{"role":"user","content":"hello"}'
```

WebUI：`/sessions` 页面与 HTMX 端点提供可视化管理与详情查看。

## 4. 任务/执行流（单机）

1. **准备**：启动 bridge（可选 WebUI），确认 `/health` 可达。
2. **发现**：`GET /api/sops` / `GET /api/skills` 或 CLI `sisyphus sop list`（需 `--bridge`）。
3. **执行**：`POST /api/execute/goal`（SOP/目标）或 SDK `executeSop/execute_sop/ExecuteSOP`；工具执行用 `POST /api/execute`。
4. **观测**：执行状态/轨迹/流（`/api/execute/goal/{id}/status|trace|stream`）、认知追踪/地图（按启用）、集群只读状态 `/api/cluster/status`。
5. **反馈**：失败时区分契约错误、连通性错误与依赖/配置缺失（LLM/工具/MCP），按 `single-node-tools-and-dependencies.md` 分层处理。

## 5. 错误与一致性（P1-A/P1-D）

- **bridge 不可达**：CLI 已提供可操作提示（确认启动、检查地址/配置）；WebUI 只读面板降级为默认快照。
- **配置/依赖缺失**：执行类能力应返回明确错误，不静默成功。
- **契约一致性**：以 `single-node-contract-guard.md` 为回归依据，变更需同步三端与文档。

## 6. 关联文档

- `single-node-typical-workflow.md`
- `single-node-operation-and-verification.md`
- `single-node-contract-guard.md`
