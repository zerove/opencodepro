# SDK 参考

Sisyphus 提供 TypeScript、Python 与 Go 官方 SDK。三者都直接对接 bridge 的 REST API，
端点契约以 [`rest-api.md`](rest-api.md) 为准。

## 端点速查（易混淆的两组）

| 端点 | 用途 | 请求体 | 响应体 |
|------|------|--------|--------|
| `POST /api/execute` | **工具**执行 | `{tool_id, args_json, fuel_limit}` | `{status, result_json}` |
| `POST /api/execute/goal` | **SOP/目标**执行 | `{goal, sop_id, project_id?, event_tx?}` | `{output, report}` |
| `GET /api/sops` | SOP 列表 | — | `string[]` |
| `GET /api/sops/{id}` | SOP 详情 | — | `{id, name, version, config}`（需 `X-Sisyphus-Workspace-Id` 头） |
| `GET /api/skills` | 技能列表 | — | `[{name, description, version, author, tags, source}]` |
| `POST /api/skills/install` | 安装技能 | `{skill_id}` | 201，**无响应体** |

SDK 的方法名把这两者区分为 `executeSop`（走 `/api/execute/goal`）与
`executeTool`（走 `/api/execute`）。旧版本 SDK 把 SOP 执行打到了 `/api/execute`，
请求体会被服务端拒绝（该端点要求 `tool_id`）。

## TypeScript SDK

```bash
npm install @sisyphus/sdk
```

```typescript
import { SisyphusClient } from '@sisyphus/sdk';

const client = new SisyphusClient({
  baseUrl: 'http://localhost:8080',
  // 按工作区隔离的端点（如 GET /api/sops/{id}）要求该头
  workspaceId: 'my-project',
});

// 健康检查
const health = await client.health();
console.log(health.status); // "ok"

// 会话
const sessions = await client.listSessions();          // SessionSummary[]
const session = await client.createSession('my-session'); // SessionDetail

// 搜索
const results = await client.vectorSearch('my query', 10);

// 执行 SOP（目标执行）
const result = await client.executeSop('sop-id', 'Analyze code', 'my-project');
console.log(result.report);
console.log(result.output);

// 执行工具（工具执行）
const tool = await client.executeTool('file_read', '{"path":"/tmp/a.txt"}', 1_000_000);
console.log(tool.status, tool.result_json);

// 认知循环异常观测（可省略 sopId 读取全部作用域）
const anomalies = await client.anomalyReports('sop-id');
for (const a of anomalies) {
  console.log(`${a.severity} ${a.node_name}/${a.metric}: ${a.observed} vs ${a.expected}`);
}

// 集群 / 控制面只读观测（ADR-020：无 REST 写入）
const status = await client.clusterStatus();
console.log(status.enabled, status.control_plane.security.effective_armed);

const lease = await client.controlLease('lease:demo');
if (lease.status_code === 'DISABLED') {
  console.log('控制面未启用:', lease.status_message);
}

// 经验共享
const shared = await client.listSharedExperiences();
await client.shareExperience('exp:1700000000');
await client.unshareExperience('exp:1700000000', 'proj-1');
```

## Python SDK

```bash
pip install sisyphus-sdk
```

```python
from sisyphus_sdk import SisyphusClient

client = SisyphusClient(base_url="http://localhost:8080", workspace_id="my-project")

# 健康检查
health = client.health()
print(health.status)  # "ok"

# 会话
sessions = client.list_sessions()          # list[SessionSummary]
session = client.create_session("my-session")  # SessionDetail

# 搜索
results = client.search("my query", top_k=10)

# 执行 SOP（目标执行）
result = client.execute_sop("sop-id", goal="Analyze code", project_id="my-project")
print(result.report, result.output)

# 执行工具（工具执行）
tool = client.execute_tool("file_read", '{"path":"/tmp/a.txt"}', fuel_limit=1_000_000)
print(tool.status, tool.result_json)

# 认知循环异常观测（可省略 sop_id 读取全部作用域）
for a in client.anomaly_reports("sop-id"):
    print(a.severity, a.node_name, a.metric, a.observed, a.expected)

# 集群 / 控制面只读观测
status = client.cluster_status()
print(status.enabled, status.control_plane.security.effective_armed)

lease = client.control_lease("lease:demo")
if lease.status_code == "DISABLED":
    print("控制面未启用:", lease.status_message)

# 经验共享
shared = client.list_shared_experiences()
client.share_experience("exp:1700000000")
client.unshare_experience("exp:1700000000", "proj-1")
```

## Go SDK

```bash
go get github.com/sisyphus/sdk-go
```

```go
package main

import (
    "fmt"
    sisyphus "github.com/sisyphus/sdk-go"
)

func main() {
    client := sisyphus.NewClient("http://localhost:8080", "")
    client.WorkspaceID = "my-project"

    // 健康检查
    health, _ := client.Health()
    fmt.Println(health.Status) // "ok"

    // 会话
    sessions, _ := client.ListSessions()
    session, _ := client.CreateSession("my-session")

    // 搜索
    results, _ := client.SearchMemory("my query", 10)

    // 执行 SOP（目标执行）
    result, _ := client.ExecuteSOP("sop-id", "Analyze code", "my-project")
    fmt.Println(result.Report)

    // 执行工具（工具执行）
    tool, _ := client.ExecuteTool("file_read", `{"path":"/tmp/a.txt"}`, 1_000_000)
    fmt.Println(tool.Status, tool.ResultJSON)

    // 认知循环异常观测（sopID 传空表示全部作用域）
    anomalies, _ := client.AnomalyReports("sop-id")
    for _, a := range anomalies {
        fmt.Println(a.Severity, a.NodeName, a.Metric, a.Observed, a.Expected)
    }

    // 集群 / 控制面只读观测
    status, _ := client.GetClusterStatus()
    fmt.Println(status.Enabled, status.ControlPlane.Security.EffectiveArmed)

    lease, _ := client.GetControlLease("lease:demo")
    if lease.StatusCode == sisyphus.LeaseStatusDisabled {
        fmt.Println("控制面未启用:", lease.StatusMessage)
    }

    // 经验共享
    shared, _ := client.ListSharedExperiences()
    client.ShareExperience("exp:1700000000")
    client.UnshareExperience("exp:1700000000", "proj-1")
}
```

## API 参考

所有 SDK 暴露相同的核心方法（TS 用 camelCase，Go 用 PascalCase，Python 用 snake_case）：

| 方法 | 说明 |
|------|------|
| `health()` | 检查 bridge 状态 |
| `list_sessions()` | 列出所有会话（`SessionSummary[]`） |
| `create_session(name)` | 创建会话（返回 `SessionDetail`） |
| `get_session(id)` | 获取会话详情（404 时返回 `null` / `None`） |
| `add_message(session_id, role, content)` | 追加消息（返回更新后的 `SessionDetail`） |
| `search(query, top_k)` / `vector_search` | 向量搜索 |
| `execute_sop(sop_id, goal, project_id?)` | **SOP/目标**执行（`POST /api/execute/goal`） |
| `execute_tool(tool_id, args_json, fuel_limit?)` | **工具**执行（`POST /api/execute`） |
| `list_sops()` | 列出可用 SOP（`string[]`） |
| `get_sop(sop_id)` | 获取 SOP 详情（需工作区头） |
| `get_dag(sop_id)` | `get_sop` 的兼容别名（仅 Python，返回原始 dict） |
| `list_skills()` | 列出本机技能（`name/description/version/author/tags/source`） |
| `install_skill(skill_id)` | 安装远端技能（201，无响应体；返回 `void`/`None`/`error`） |
| `validate_auth(api_key)` | 验证 API 密钥（返回 `bool`） |
| `cluster_status()` | 集群协调只读快照（含控制面安全装配视图） |
| `control_lease(key)` | 控制面租约只读查询（`status_code`：`OK`/`NOT_FOUND`/`DISABLED`/`UNAVAILABLE`） |
| `anomaly_reports(sop_id?)` | 读取认知循环观测到的异常（`sop_id` 可省略 = 全部作用域） |
| `list_shared_experiences()` | 列出已跨项目共享的经验 |
| `share_experience(exp_key)` | 把本地经验标记为跨项目共享 |
| `unshare_experience(exp_key, project_id)` | 取消指定项目的经验共享（`project_id` 必填） |

### 集群观测的两个约定

1. **只读**：ADR-020 明确不提供 REST 写入路径；SDK 也没有 `acquire`/`renew`/`release`。
2. **不是错误**：控制面未启用（`cluster.control_plane.enabled=false`）、租约不存在、
   状态未装配时，端点仍返回 HTTP 200，用 `status_code` 区分。SDK 不会抛异常，
   调用方应按 `status_code` / `found` 分支处理。

### 工作区头

`GET /api/sops/{id}`（以及其它按工作区隔离的端点）要求请求头
`X-Sisyphus-Workspace-Id`。三个 SDK 都在构造时接收该值并自动附加：

- TypeScript：`new SisyphusClient({ workspaceId })`
- Python：`SisyphusClient(workspace_id=...)`
- Go：`client.WorkspaceID = ...`

缺失时服务端返回 400 `Missing X-Sisyphus-Workspace-Id header`。
