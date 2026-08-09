# SDK 参考

Sisyphus 提供 TypeScript、Python 与 Go 官方 SDK。

## TypeScript SDK

```bash
npm install @sisyphus/sdk
```

```typescript
import { SisyphusClient } from '@sisyphus/sdk';

const client = new SisyphusClient({ baseUrl: 'http://localhost:8080' });

// 健康检查
const health = await client.health();
console.log(health.status); // "ok"

// 会话
const sessions = await client.listSessions();
const session = await client.createSession('my-session');

// 搜索
const results = await client.searchMemory('my query', 10);

// 执行 SOP
const result = await client.executeSop('sop-id');
console.log(result.status); // "success"
```

## Python SDK

```bash
pip install sisyphus-sdk
```

```python
from sisyphus_sdk import SisyphusClient

client = SisyphusClient(base_url="http://localhost:8080")

# 健康检查
health = client.health()
print(health.status)  # "ok"

# 会话
sessions = client.list_sessions()
session = client.create_session("my-session")

# 搜索
results = client.search("my query", top_k=10)

# 执行 SOP
result = client.execute_sop("sop-id")
print(result.status)  # "success"
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

    // 健康检查
    health, _ := client.Health()
    fmt.Println(health.Status) // "ok"

    // 会话
    sessions, _ := client.ListSessions()
    session, _ := client.CreateSession("my-session")

    // 搜索
    results, _ := client.SearchMemory("my query", 10)

    // 执行 SOP
    result, _ := client.ExecuteSOP("sop-id", nil)
    fmt.Println(result.Status) // "success"
}
```

## API 参考

所有 SDK 暴露相同的核心方法：

| 方法 | 说明 |
|--------|-------------|
| `health()` | 检查 bridge 状态 |
| `list_sessions()` | 列出所有会话 |
| `create_session(name)` | 创建新会话 |
| `get_session(id)` | 获取会话详情 |
| `add_message(session_id, role, content)` | 向会话添加消息 |
| `search(query, top_k)` | 向量搜索 |
| `execute_sop(sop_id, context)` | 执行 SOP |
| `list_sops()` | 列出可用 SOP |
| `get_dag(sop_id)` | 获取 DAG 图 |
| `list_skills()` | 列出可用技能 |
| `install_skill(skill_id)` | 安装技能 |
| `validate_auth(api_key)` | 验证 API 密钥 |