# Sisyphus Go SDK

REST API 客户端 for [Sisyphus 认知计算引擎](https://github.com/zerove/opencodepro).

## 安装

```bash
go get github.com/sisyphus/sdk-go
```

(本仓库 `ide/go-sdk/` 是开发源, 模块路径 `github.com/sisyphus/sdk-go`, 包名 `sisyphus`。)

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
    // GET /api/sops/{id} 等按工作区隔离的端点要求该头
    client.WorkspaceID = "my-project"

    // 健康检查
    health, err := client.Health()
    if err != nil {
        log.Fatal(err)
    }
    fmt.Printf("Status: %s, Version: %s\n", health.Status, health.Version)

    // 列出 SOP（服务端返回 ID 字符串数组）
    sops, err := client.ListSOPs()
    if err != nil {
        log.Fatal(err)
    }
    for _, id := range sops {
        fmt.Println("SOP:", id)
    }

    // 执行 SOP —— 走 POST /api/execute/goal
    result, err := client.ExecuteSOP("sop-id", "Analyze code", "my-project")
    if err != nil {
        log.Fatal(err)
    }
    fmt.Println(result.Report)
}
```

## 完整 API

| 方法 | 端点 | 说明 |
|------|------|------|
| `Health()` | GET /health | 健康检查 |
| `SearchMemory(query, topK)` | POST /api/vector/search | 向量搜索 |
| `ListSessions()` | GET /api/sessions | 列出会话（`[]Session`） |
| `GetSession(id)` | GET /api/sessions/{id} | 会话详情 |
| `CreateSession(name)` | POST /api/sessions | 创建会话（返回 `*SessionDetail`） |
| `AddMessage(id, role, content)` | POST /api/sessions/{id}/messages | 追加消息（返回更新后的 `*SessionDetail`） |
| `ExecuteSOP(sopID, goal, projectID)` | POST /api/execute/goal | **SOP/目标**执行 → `*GoalExecuteResult` |
| `ExecuteTool(toolID, argsJSON, fuelLimit)` | POST /api/execute | **工具**执行 → `*ToolExecuteResult` |
| `ListSOPs()` | GET /api/sops | 列出 SOP（`[]string`） |
| `GetSOP(sopID)` | GET /api/sops/{id} | SOP 详情（需 `WorkspaceID`） |
| `ListSkills()` | GET /api/skills | 列出技能（`name/description/version/author/tags/source`） |
| `InstallSkill(skillID)` | POST /api/skills/install | 安装技能（201，无响应体） |
| `ValidateAuth(apiKey)` | POST /api/auth/validate | 验证 API key（返回 `bool`） |
| `GetClusterStatus()` | GET /api/cluster/status | 集群协调只读快照 |
| `GetControlLease(key)` | GET /api/cluster/control/leases/{key} | 控制面租约只读查询 |
| `AnomalyReports(sopID)` | GET /api/anomaly | 认知循环异常观测（`sopID` 空 = 全部作用域） |
| `ListSharedExperiences()` | GET /api/experience/shared | 列出跨项目共享的经验 |
| `ShareExperience(expKey)` | POST /api/experience/share | 分享经验 |
| `UnshareExperience(expKey, projectID)` | POST /api/experience/unshare | 取消分享 |

### 两个易错点

1. **`/api/execute` ≠ SOP 执行**：它是工具执行端点（`tool_id`/`args_json`）。
   SOP 执行请用 `ExecuteSOP()`（内部走 `/api/execute/goal`），旧版本的
   `ExecuteSOP(sopID, context map[string]interface{})` 签名已移除。
2. **控制面只读且不报错**：`GetControlLease()` 在控制面未启用时返回
   `StatusCode == LeaseStatusDisabled` 且 `Found == false`，HTTP 仍为 200。
   请按 `StatusCode`（`LeaseStatusOK`/`LeaseStatusNotFound`/
   `LeaseStatusDisabled`/`LeaseStatusUnavailable`）分支处理。

## 测试

```bash
cd ide/go-sdk
go test ./...
```

## 详细文档

参见 [SDK 参考](https://zerove.github.io/opencodepro/sdk-reference.html) 与
[book/src/sdk-reference.md](../../book/src/sdk-reference.md)。
