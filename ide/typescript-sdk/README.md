# Sisyphus TypeScript SDK

REST API 客户端 for [Sisyphus 认知计算引擎](https://github.com/zerove/opencodepro).

纯 TypeScript, 零外部依赖（仅 `fetch` API）。Node 18+ 和现代浏览器原生支持。

## 安装

```bash
npm install @sisyphus/sdk
```

(本仓库 `ide/typescript-sdk/` 是开发源。)

## 快速开始

```typescript
import { SisyphusClient } from '@sisyphus/sdk';

const client = new SisyphusClient({
  baseUrl: 'http://localhost:8080',
  authToken: 'your-api-key',
  // GET /api/sops/{id} 等按工作区隔离的端点要求该头
  workspaceId: 'my-project',
});

// 健康检查
const health = await client.health();
console.log(health.status, health.version);

// 列出会话（SessionSummary[]，不含 messages）
const sessions = await client.listSessions();

// 执行 SOP —— 走 POST /api/execute/goal
const result = await client.executeSop('sop-id', 'Analyze code', 'my-project');
console.log(result.report, result.output);
```

## 完整 API

| 方法 | 端点 | 说明 |
|------|------|------|
| `health()` | GET /health | 健康检查 |
| `getState(projectId, key)` | GET /api/state/{key} | 读取状态（404 → `null`） |
| `setState(projectId, key, value)` | PUT /api/state/{key} | 写入状态（204 无响应体） |
| `listState(projectId)` | GET /api/state | 列出状态 |
| `vectorSearch(query, topK)` | POST /api/vector/search | 向量搜索 |
| `listSessions()` | GET /api/sessions | 列出会话（`SessionSummary[]`） |
| `getSession(id)` | GET /api/sessions/{id} | 会话详情（404 → `null`） |
| `createSession(name)` | POST /api/sessions | 创建会话（返回 `SessionDetail`） |
| `addMessage(id, role, content)` | POST /api/sessions/{id}/messages | 追加消息（返回更新后的 `SessionDetail`） |
| `executeSop(sopId, goal, projectId?, eventTx?)` | POST /api/execute/goal | **SOP/目标**执行 → `{output, report}` |
| `executeTool(toolId, argsJson, fuelLimit?)` | POST /api/execute | **工具**执行 → `{status, result_json}` |
| `listSops()` | GET /api/sops | 列出 SOP（`string[]`） |
| `getSop(sopId)` | GET /api/sops/{id} | SOP 详情（需 `workspaceId`） |
| `listSkills()` | GET /api/skills | 列出技能（`name/description/version/author/tags/source`） |
| `installSkill(skillId)` | POST /api/skills/install | 安装技能（201，无响应体） |
| `validateAuth(apiKey)` | POST /api/auth/validate | 验证 API key（返回 `boolean`） |
| `clusterStatus()` | GET /api/cluster/status | 集群协调只读快照 |
| `controlLease(key)` | GET /api/cluster/control/leases/{key} | 控制面租约只读查询 |
| `anomalyReports(sopId?)` | GET /api/anomaly | 认知循环异常观测 |
| `listSharedExperiences()` | GET /api/experience/shared | 列出跨项目共享的经验 |
| `shareExperience(expKey)` | POST /api/experience/share | 分享经验 |
| `unshareExperience(expKey, projectId)` | POST /api/experience/unshare | 取消分享 |

### 两个易错点

1. **`/api/execute` ≠ SOP 执行**：它是工具执行端点（`tool_id`/`args_json`）。
   SOP 执行请用 `executeSop()`（内部走 `/api/execute/goal`），旧版本的
   `executeSOP(sopId, context)` 签名已移除。
2. **控制面只读且不报错**：`controlLease()` 在控制面未启用时返回
   `status_code: 'DISABLED'` 且 `found: false`，HTTP 仍为 200。请按
   `status_code`（`OK`/`NOT_FOUND`/`DISABLED`/`UNAVAILABLE`）分支处理。

## 测试

```bash
cd ide/typescript-sdk
npm install
npm run build && npm test
```

## 详细文档

参见 [SDK 参考](https://zerove.github.io/opencodepro/sdk-reference.html) 与
[book/src/sdk-reference.md](../../book/src/sdk-reference.md)。
