# Sisyphus Go SDK

REST API 客户端 for [Sisyphus 认知计算引擎](https://github.com/zerove/opencodepro).

## 安装

```bash
go get github.com/sisyphus/sdk-go
```

(本仓库 `ide/go-sdk/` 是开发源, 模块路径 `github.com/sisyphus/sdk-go`。)

## 快速开始

```go
package main

import (
    "fmt"
    "log"
    "github.com/sisyphus/sdk-go/sisyphus"
)

func main() {
    client := sisyphus.NewClient("http://localhost:8080", "your-api-key")

    // 健康检查
    health, err := client.Health()
    if err != nil {
        log.Fatal(err)
    }
    fmt.Printf("Status: %s, Version: %s\n", health.Status, health.Version)

    // 列出 SOP
    sops, err := client.ListSops()
    if err != nil {
        log.Fatal(err)
    }
    for _, sop := range sops {
        fmt.Printf("SOP: %s (%s) - %d nodes\n", sop.Name, sop.ID, sop.NodeCount)
    }
}
```

## 完整 API

| 方法 | 端点 | 说明 |
|------|------|------|
| `Health()` | GET /health | 健康检查 |
| `ListSessions()` | GET /api/sessions | 列出所有会话 |
| `GetSession(id)` | GET /api/sessions/{id} | 获取单个会话 |
| `CreateSession(name)` | POST /api/sessions | 创建会话 |
| `AddMessage(id, role, content)` | POST /api/sessions/{id}/messages | 添加消息 |
| `Search(query, topK)` | POST /api/vector/search | 向量搜索 |
| `ExecuteSOP(id, context)` | POST /api/execute | 执行 SOP |
| `ListSops()` | GET /api/sops | 列出所有 SOP |
| `GetDAG(id)` | GET /api/sops/{id} | 获取 DAG |
| `ListSkills()` | GET /api/skills | 列出技能 |
| `InstallSkill(id)` | POST /api/skills/install | 安装技能 |
| `ValidateAuth(key)` | POST /api/auth/validate | 验证 API key |

## 测试

```bash
cd ide/go-sdk
go test ./...
```

## 详细文档

参见 [book/src/sdk-go.md](https://zerove.github.io/opencodepro/sdk-go.html)。
