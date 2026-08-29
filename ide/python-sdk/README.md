# Sisyphus Python SDK

REST API 客户端 for [Sisyphus 认知计算引擎](https://github.com/zerove/opencodepro).

零依赖 — 使用 Python 标准库 `urllib`。可选 `aiohttp` 支持异步。

## 安装

```bash
pip install sisyphus-sdk
```

(本仓库 `ide/python-sdk/` 是开发源。生产发布待 PyPI 上线。)

## 快速开始

```python
from sisyphus_sdk import SisyphusClient

# 默认 http://localhost:8080
client = SisyphusClient(base_url="http://localhost:8080", auth_token="...")

# 健康检查
health = client.health()
print(health.status, health.version)

# 会话管理
sessions = client.list_sessions()
session = client.create_session(name="my-session")
client.add_message(session.id, role="user", content="hello")

# SOP 执行
result = client.execute_sop("sop-id", context={"key": "value"})
print(result.status, result.iterations, result.deviation)
```

## 异步支持

```python
from sisyphus_sdk import async_health

health = await async_health("http://localhost:8080")
```

需要 `aiohttp` (可选依赖): `pip install aiohttp`

## 完整 API

| 方法 | 端点 | 说明 |
|------|------|------|
| `health()` | GET /health | 健康检查 |
| `list_sessions()` | GET /api/v1/sessions | 列出所有会话 |
| `get_session(id)` | GET /api/v1/sessions/{id} | 获取单个会话 |
| `create_session(name)` | POST /api/v1/sessions | 创建会话 |
| `add_message(id, role, content)` | POST /api/v1/sessions/{id}/messages | 添加消息 |
| `search(query, top_k)` | POST /api/v1/vector/search | 向量搜索 |
| `execute_sop(sop_id, context)` | POST /api/v1/execute | 执行 SOP |
| `list_sops()` | GET /api/v1/sops | 列出所有 SOP |
| `get_dag(sop_id)` | GET /api/v1/sops/{id}/dag | 获取 DAG |
| `list_skills()` | GET /api/v1/skills | 列出技能 |
| `install_skill(skill_id)` | POST /api/v1/skills/install | 安装技能 |
| `validate_auth(api_key)` | POST /api/auth/validate | 验证 API key |

## 测试

```bash
cd ide/python-sdk
pip install pytest
python -m pytest tests/
```

## 详细文档

参见 [book/src/sdk-python.md](https://zerove.github.io/opencodepro/sdk-python.html)。
