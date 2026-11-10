# Go SDK

模块 `github.com/sisyphus/sdk-go`（包名 `sisyphus`），源码位于 `ide/go-sdk/`。

## 安装

```bash
go get github.com/sisyphus/sdk-go
```

## 快速开始

```go
package main

import (
    "fmt"
    "log"

    sisyphus "github.com/sisyphus/sdk-go"
)

func main() {
    client := sisyphus.NewClient("http://localhost:8080", "your-api-key")
    client.WorkspaceID = "my-project" // GET /api/sops/{id} 要求该头

    health, err := client.Health()
    if err != nil { log.Fatal(err) }
    fmt.Println(health.Status)

    result, err := client.ExecuteSOP("sop-id", "Analyze code", "my-project")
    if err != nil { log.Fatal(err) }
    fmt.Println(result.Report)
}
```

## 方法速查

| 方法 | 端点 | 说明 |
|------|------|------|
| `Health` | GET /health | 健康检查 |
| `SearchMemory` | POST /api/vector/search | 向量搜索 |
| `ListSessions/GetSession/CreateSession/AddMessage` | GET/GET/POST/POST /api/sessions… | 会话 |
| `ExecuteSOP` | POST /api/execute/goal | **SOP/目标**执行 → `*GoalExecuteResult` |
| `ExecuteTool` | POST /api/execute | **工具**执行 → `*ToolExecuteResult` |
| `ListSOPs/GetSOP` | GET /api/sops… | SOP 列表 / 详情 |
| `ListSkills/InstallSkill` | GET /api/skills、POST /api/skills/install | 技能 |
| `ValidateAuth` | POST /api/auth/validate | API key 校验 |
| `GetClusterStatus` | GET /api/cluster/status | 集群 / 控制面只读观测 |
| `GetControlLease` | GET /api/cluster/control/leases/{key} | 控制面租约只读查询 |
| `AnomalyReports` | GET /api/anomaly | 认知循环异常 |
| `ListSharedExperiences/ShareExperience/UnshareExperience` | GET/POST/POST /api/experience… | 经验共享 |

## 集群 / 控制面只读观测

```go
status, err := client.GetClusterStatus()
if err != nil { log.Fatal(err) }
fmt.Println(status.Enabled, status.ControlPlane.Security.EffectiveArmed)

lease, err := client.GetControlLease("lease:demo")
if err != nil { log.Fatal(err) }
if lease.StatusCode == sisyphus.LeaseStatusDisabled { fmt.Println(lease.StatusMessage) }
```

租约查询是只读端点（ADR-020）。控制面未启用时仍返回 200，`Found=false`、
`StatusCode="DISABLED"`，调用方按 `StatusCode` 分支处理。

## 错误

HTTP 4xx/5xx 时返回 `*sisyphus.APIError`（含 `Status`、`Message`、可选 `Hint`、`Raw`）；网络错误返回普通 `error`。当服务端返回 `{ "error": "...", "hint": "..." }` 时，`Error()` 会附带 hint，调用方也可通过类型断言读取结构化字段。控制面租约查询在未启用/未找到等场景下仍返回 200，请按 `StatusCode` 判定。

## 测试

```bash
cd ide/go-sdk
go test ./...
```
