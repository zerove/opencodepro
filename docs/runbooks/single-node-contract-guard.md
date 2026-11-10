# 单机契约守护清单（P1-F）

> 目标：维护 bridge REST ↔ WebUI ↔ SDK（三端）在单机场景下的契约一致性，防止回归。  
> 原则：契约变更必须同步实现、测试与文档（book/CHANGELOG）；`cluster.control_plane.enabled` 默认 `false`，只读观测边界不变。

## 1. 关键契约对照（单机高频路径）

| 能力 | HTTP 端点 | 请求/响应要点 | WebUI | TS SDK | Python SDK | Go SDK | 测试/证据 |
|---|---|---|---|---|---|---|---|
| 健康 | `GET /health` | JSON 状态/版本 | `/api/health` 代理 | - | - | - | bridge handlers；手册验证 |
| SOP 列表 | `GET /api/sops` | **`string[]`**（SOP ID） | `htmx/sops`, `/api/sops` | `listSops()` | `list_sops()` | `ListSOPs()` | SDK/WebUI 契约测试 |
| SOP 详情 | `GET /api/sops/{id}` | `{id,name,version,config}`（按实现） | DAG 布局来源 | `getSop(id)` | `get_sop(id)` | `GetSOP(id)` | 契约对齐批次 |
| 目标执行（SOP） | `POST /api/execute/goal` | `{goal,sop_id,project_id?,event_tx?}` → `{output,report}` | `htmx/exec` → goal | `executeSop(...)` | `execute_sop(...)` | `ExecuteSOP(...)` | SDK/WebUI 测试 |
| 工具执行 | `POST /api/execute` | `{tool_id,args_json,fuel_limit}` → `{status,result_json}` | `/api/execute` | `executeTool(...)` | `execute_tool(...)` | `ExecuteTool(...)` | 契约对齐 |
| 会话列表 | `GET /api/sessions` | `SessionSummary[]`（无 messages） | sessions 列表 | 相关方法 | 相关方法 | 相关方法 | 契约对齐 |
| 创建会话 | `POST /api/sessions` | 返回 `SessionDetail` | 创建流程 | `createSession` 等 | 对应方法 | 对应方法 | 契约对齐 |
| 追加消息 | `POST /api/sessions/{id}/messages` | 返回更新后的会话（按实现） | htmx/api | `addMessage` 等 | 对应方法 | 对应方法 | 契约对齐 |
| 技能列表 | `GET /api/skills` | `{name,description,version,author,tags,source}` | `/api/skills`, htmx | `listSkills()` | `list_skills()` | `ListSkills()` | 契约对齐 |
| 技能安装 | `POST /api/skills/install` | **201 空响应体**（成功） | `htmx/skills/install` | `installSkill(...)` | `install_skill(...)` | `InstallSkill(...)` | 契约对齐 |
| 集群状态（只读） | `GET /api/cluster/status` | `ClusterStatus`（含 control_plane/security/stats 等） | `/api/cluster/status`, 页面/HTMX | `clusterStatus()` | `cluster_status()` | `GetClusterStatus()` | WebUI/SDK 测试 |
| 租约查询（只读） | `GET /api/cluster/control/leases/{key}` | `ControlLease`（恒 200，`found/status_code` 表达） | HTMX/代理 | `controlLease(key)` | `control_lease(key)` | `GetControlLease(key)` | WebUI/SDK 测试 |
| 工作区头 | 多端点（如 `GET /api/sops/{id}`） | `X-Sisyphus-Workspace-Id`（缺失可能 400） | `WEBUI_WORKSPACE_ID` | `workspaceId` | `workspace_id` | `WorkspaceID` | 统一附加 |

> 注：以实际实现与 OpenAPI 为准；本表用于回归守护，细节变更需同步更新。

## 2. 回归检查流程（建议每次接口/SDK 变更执行）

1. **实现与路由**：确认 `crates/sisyphus-bridge/src/http/*` 与 OpenAPI 一致。
2. **SDK 三端**：方法签名、请求体、响应解析与示例同步；运行 `go test ./...`、`python -m pytest tests/`、`npm run build && npm test`。
3. **WebUI**：HTMX 表单/JSON 提取器匹配（`Form` vs `Json`）、端点路径与响应解析；`cargo test -p sisyphus-webui`。
4. **文档**：`book/src/rest-api.md`、`book/src/sdk-*.md`、`book/src/web-dashboard.md`、相关 runbook 与 `CHANGELOG.md`。
5. **门禁**：`cargo check-all`、`cargo fmt --all --check`、`make audit`、相关 clippy。

## 3. 已知边界

- 控制面 REST **只读**（ADR-020）；不新增写入端点，不改变 `cluster.control_plane.enabled` 默认值。
- 组合执行 `POST /api/execute/composite` 使用 `sse_sop_id` 订阅事件（与 `execute/goal` 不同），变更时需同步文档与客户端指引。

## 4. 证据记录（本批次基线）

- 建议二批次：SDK/WebUI 契约修复与只读观测对齐，三端 SDK 与 WebUI 测试通过（见 CHANGELOG 与提交记录）。
- 单机 P1：持续以本清单为回归依据推进 P1-F。

## 5. 单机契约冒烟（可执行，P1-F 落地）

在具备对应工具链/环境时，按序执行以下检查（记录结果与提交号）：

### 5.1 Rust / WebUI / 门禁
```bash
env -u RUSTUP_TOOLCHAIN cargo check-all
env -u RUSTUP_TOOLCHAIN cargo fmt --all --check
make audit
env -u RUSTUP_TOOLCHAIN cargo test -p sisyphus-webui
env -u RUSTUP_TOOLCHAIN cargo test -p sisyphus-core --lib
```
**预期**：全部通过（或按已知环境限制项单独标注，如 sandboxd 权限受限导致的 bridge lib 特定用例）。

### 5.2 SDK（三端）
```bash
# Go
(cd ide/go-sdk && go test ./...)

# Python（需本地依赖环境）
(cd ide/python-sdk && python -m pytest tests/ -q)

# TypeScript（需 node_modules）
(cd ide/typescript-sdk && npm run build && npm test)
```
**预期**：与契约对齐批次一致（当前基线：Go ok / Python 38 passed / TS 34 passed）。

### 5.3 Bridge 运行时只读冒烟（单机）
```bash
# 启动 bridge（正确顶层 bridge_address/http_address）后：
curl -sf http://127.0.0.1:8080/health
curl -sf http://127.0.0.1:8080/api/sops
curl -sf http://127.0.0.1:8080/api/cluster/status
```
**预期**：`/health` 返回 SERVING JSON；`/api/sops` 返回 `string[]`；`/api/cluster/status` 返回有效只读快照。

### 5.4 CLI 本地冒烟（无 bridge）
```bash
sisyphus read README.md | head
sisyphus exec 'echo ok'
sisyphus --data-dir ./.tmp-sn session create smoke && sisyphus --data-dir ./.tmp-sn session list
```
**预期**：命令成功、输出符合预期（S2 已验证）。

> 将本节结果纳入批次验证记录；任何失败需区分“回归/契约破坏”与“环境限制/外部依赖缺失”。
