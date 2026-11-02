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

// 搜索
const results = await client.vectorSearch('my query', 10);

// 执行 SOP
const result = await client.executeSop('sop-id');
console.log(result.status); // "success"

// 认知循环异常观测（可省略 sopId 读取全部作用域）
const anomalies = await client.anomalyReports('sop-id');
for (const a of anomalies) {
  console.log(`${a.severity} ${a.node_name}/${a.metric}: ${a.observed} vs ${a.expected}`);
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

# 认知循环异常观测（可省略 sop_id 读取全部作用域）
for a in client.anomaly_reports("sop-id"):
    print(a.severity, a.node_name, a.metric, a.observed, a.expected)

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

    // 认知循环异常观测（sopID 传空表示全部作用域）
    anomalies, _ := client.AnomalyReports("sop-id")
    for _, a := range anomalies {
        fmt.Println(a.Severity, a.NodeName, a.Metric, a.Observed, a.Expected)
    }

    // 经验共享
    shared, _ := client.ListSharedExperiences()
    client.ShareExperience("exp:1700000000")
    client.UnshareExperience("exp:1700000000", "proj-1")
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
| `anomaly_reports(sop_id?)` | 读取认知循环观测到的异常（`sop_id` 可省略 = 全部作用域） |
| `list_shared_experiences()` | 列出已跨项目共享的经验 |
| `share_experience(exp_key)` | 把本地经验标记为跨项目共享 |
| `unshare_experience(exp_key, project_id)` | 取消指定项目的经验共享（`project_id` 必填） |

> TypeScript / Go 使用 camelCase / PascalCase：`anomalyReports`、`AnomalyReports`；其余同理。