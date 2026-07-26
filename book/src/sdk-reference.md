# SDK Reference

Sisyphus provides official SDKs for TypeScript, Python, and Go.

## TypeScript SDK

```bash
npm install @sisyphus/sdk
```

```typescript
import { SisyphusClient } from '@sisyphus/sdk';

const client = new SisyphusClient({ baseUrl: 'http://localhost:8080' });

// Health check
const health = await client.health();
console.log(health.status); // "ok"

// Sessions
const sessions = await client.listSessions();
const session = await client.createSession('my-session');

// Search
const results = await client.searchMemory('my query', 10);

// Execute SOP
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

# Health check
health = client.health()
print(health.status)  # "ok"

# Sessions
sessions = client.list_sessions()
session = client.create_session("my-session")

# Search
results = client.search("my query", top_k=10)

# Execute SOP
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

    // Health check
    health, _ := client.Health()
    fmt.Println(health.Status) // "ok"

    // Sessions
    sessions, _ := client.ListSessions()
    session, _ := client.CreateSession("my-session")

    // Search
    results, _ := client.SearchMemory("my query", 10)

    // Execute SOP
    result, _ := client.ExecuteSOP("sop-id", nil)
    fmt.Println(result.Status) // "success"
}
```

## API Reference

All SDKs expose the same core methods:

| Method | Description |
|--------|-------------|
| `health()` | Check bridge status |
| `list_sessions()` | List all sessions |
| `create_session(name)` | Create a new session |
| `get_session(id)` | Get session details |
| `add_message(session_id, role, content)` | Add message to session |
| `search(query, top_k)` | Vector search |
| `execute_sop(sop_id, context)` | Execute a SOP |
| `list_sops()` | List available SOPs |
| `get_dag(sop_id)` | Get DAG graph |
| `list_skills()` | List available skills |
| `install_skill(skill_id)` | Install a skill |
| `validate_auth(api_key)` | Validate API key |
