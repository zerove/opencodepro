# TypeScript SDK

包名 `@sisyphus/sdk`，源码位于 `ide/typescript-sdk/`。

## 安装

```bash
npm install @sisyphus/sdk
```

（本仓库 `ide/typescript-sdk/` 是开发源。）

## 快速开始

```typescript
import { SisyphusClient } from '@sisyphus/sdk';

const client = new SisyphusClient({
  baseUrl: 'http://localhost:8080',
  authToken: 'your-api-key',
  workspaceId: 'my-project',
});

const health = await client.health();
const result = await client.executeSop('sop-id', 'Analyze code', 'my-project');
console.log(result.report, result.output);
```

## 方法速查

| 方法 | 端点 | 说明 |
|------|------|------|
| `health()` | GET /health | 健康检查 |
| `getState/setState/listState` | GET/PUT/GET /api/state… | 状态 CRUD |
| `vectorSearch` | POST /api/vector/search | 向量搜索 |
| `listSessions/getSession/createSession/addMessage` | GET/GET/POST/POST /api/sessions… | 会话 |
| `executeSop` | POST /api/execute/goal | **SOP/目标**执行 → `{output, report}` |
| `executeTool` | POST /api/execute | **工具**执行 → `{status, result_json}` |
| `listSops/getSop` | GET /api/sops… | SOP 列表 / 详情 |
| `listSkills/installSkill` | GET /api/skills、POST /api/skills/install | 技能 |
| `validateAuth` | POST /api/auth/validate | API key 校验 |
| `clusterStatus/controlLease` | GET /api/cluster/status、GET /api/cluster/control/leases/{key} | 集群 / 控制面只读观测 |
| `anomalyReports` | GET /api/anomaly | 认知循环异常 |
| `listSharedExperiences/shareExperience/unshareExperience` | GET/POST/POST /api/experience… | 经验共享 |

## 集群 / 控制面只读观测

```typescript
const status = await client.clusterStatus();
console.log(status.enabled, status.control_plane.security.effective_armed);

const lease = await client.controlLease('lease:demo');
if (lease.status_code === 'DISABLED') console.log(lease.status_message);
```

控制面租约查询（`GET /api/cluster/control/leases/{key}`）是**只读**端点（ADR-020：
REST 写入=否）。控制面未启用 / 租约不存在时，HTTP 状态仍为 200，SDK 以
`status_code`（`OK`/`NOT_FOUND`/`DISABLED`/`UNAVAILABLE`）表达结果，不抛异常。

## 错误

桥接返回非 2xx 时抛出 `SisyphusApiError`（含 `status`），`getState`/`getSession`
在 404 时返回 `null`。

## 测试

```bash
cd ide/typescript-sdk
npm install
npm run build && npm test
```
