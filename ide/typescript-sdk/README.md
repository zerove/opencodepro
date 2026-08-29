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
});

// 健康检查
const health = await client.health();
console.log(health.status, health.version);

// 列出会话
const sessions = await client.listSessions();

// 执行 SOP
const result = await client.executeSOP('sop-id', { key: 'value' });
console.log(result.status, result.iterations);
```

## 完整 API

| 方法 | 端点 | 说明 |
|------|------|------|
| `health()` | GET /health | 健康检查 |
| `listSessions()` | GET /api/v1/sessions | 列出所有会话 |
| `getSession(id)` | GET /api/v1/sessions/{id} | 获取单个会话 |
| `createSession(name)` | POST /api/v1/sessions | 创建会话 |
| `addMessage(id, role, content)` | POST /api/v1/sessions/{id}/messages | 添加消息 |
| `search(query, topK)` | POST /api/v1/vector/search | 向量搜索 |
| `executeSOP(sopId, context)` | POST /api/v1/execute | 执行 SOP |
| `listSops()` | GET /api/v1/sops | 列出所有 SOP |
| `getDag(sopId)` | GET /api/v1/sops/{id}/dag | 获取 DAG |
| `listSkills()` | GET /api/v1/skills | 列出技能 |
| `installSkill(id)` | POST /api/v1/skills/install | 安装技能 |
| `validateAuth(key)` | POST /api/auth/validate | 验证 API key |

## 测试

```bash
cd ide/typescript-sdk
npm install
npm test
```

## 详细文档

参见 [book/src/sdk-typescript.md](https://zerove.github.io/opencodepro/sdk-typescript.html)。
