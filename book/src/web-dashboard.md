# Web 仪表盘

Web 仪表盘（`webui/`：axum + HTMX + SSE + WebSocket）通过 REST 直连 bridge
（`WEBUI_BRIDGE_URL`，默认 `http://[::1]:8080`）。

## 配置

| 环境变量 | 说明 |
|----------|------|
| `WEBUI_BRIDGE_URL` | Bridge REST API 基础 URL（默认 `http://[::1]:8080`） |
| `WEBUI_LISTEN_ADDR` | 监听地址（默认 `0.0.0.0:3000`） |
| `WEBUI_API_KEY` | 可选 bearer token，保护除 events/ws/metrics 外的所有路由 |
| `WEBUI_WORKSPACE_ID` | 可选工作区标识，`X-Sisyphus-Workspace-Id` 发给 bridge |

## 端点

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/health` | bridge 健康 |
| GET | `/api/sessions` | 会话列表 |
| GET | `/api/sessions/{id}` | 会话详情 |
| POST | `/api/sessions` | 创建会话 |
| POST | `/api/sessions/{id}/messages` | 追加消息 |
| GET | `/htmx/sessions` | HTMX 会话列表 |
| GET | `/htmx/session/{id}/detail` | HTMX 会话详情 |
| GET | `/htmx/dag?sop_id=...` | DAG 可视化（`GET /api/sops/{id}` → SOP 配置布局） |
| GET | `/htmx/sops` | SOP 列表（服务端返回 `string[]`） |
| GET | `/api/sops` | SOP 列表 |
| GET | `/api/sops/{id}/dag` | SOP DAG |
| POST | `/htmx/exec` | 目标执行（`POST /api/execute/goal`，body 为 urlencoded） |
| POST | `/api/execute` | 工具执行 |
| GET | `/htmx/skills` | 本机技能列表 |
| POST | `/htmx/skills/install` | 安装远端技能（`POST /api/skills/install`，成功 201 无响应体） |
| GET | `/api/skills` | 本机技能列表 |
| GET | `/api/cluster/status` | 集群/控制面只读观测（含 control_plane/security/stats） |
| GET | `/api/cluster/control/leases/{key}` | 控制面租约只读查询（`found/status_code`，HTTP 200） |
| GET | `/cluster` | WebUI 集群观测页面（导航 `cluster`） |
| GET | `/htmx/cluster/status` | HTMX 集群状态面板（bridge 不可达时渲染默认快照） |
| GET | `/htmx/cluster/control-lease?key=...` | HTMX 控制面租约只读查询（只读，`found/status_code`） |

## 集群观测页面

WebUI 侧提供集群只读观测入口（与 bridge 只读边界一致，ADR-020 Accepted）：

- 页面 `/cluster` 挂载「集群状态」（HTMX 加载 `/htmx/cluster/status`）与
  「控制面租约」（HTMX GET `/htmx/cluster/control-lease?key=...`）两个只读面板；
- 面板复用 bridge 直连 helper，统一附加 `X-Sisyphus-Workspace-Id`；
- bridge 不可达时面板返回渲染默认快照（HTTP 200），不产生写入路径。

## 已知修复（2026-11-10）

- **HTMX 与 JSON 提取器**：`/htmx/exec`、`/htmx/session/{id}/messages`、
  `/htmx/skills/install` 等浏览器/htmx 提交 body（urlencoded）之前用 `Json<T>`
  提取器，直接 415；现已改为 `Form<T>`。
- **SOP 执行**：之前打 `POST /api/execute`（工具执行端点）；现打
  `POST /api/execute/goal`，body `{sop_id, goal}`，响应 `{output, report}`。
- **SOP 列表**：服务端返回 `string[]`，前端按 `SopInfo` 解析已改。
- **技能列表**：服务端返回 `name/description/version/author/tags/source`，
  前端不展示不存在的 `downloads`/`rating`。
- **技能安装**：之前只在前端显示「已安装」而不请求 bridge；现真实调用
  `POST /api/skills/install`（无响应体 201 → 成功）。
