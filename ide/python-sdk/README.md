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
client = SisyphusClient(
    base_url="http://localhost:8080",
    auth_token="...",
    workspace_id="my-project",  # GET /api/sops/{id} 等端点要求该头
)

# 健康检查
health = client.health()
print(health.status, health.version)

# 会话管理
sessions = client.list_sessions()          # list[SessionSummary]
session = client.create_session(name="my-session")  # SessionDetail
client.add_message(session.id, role="user", content="hello")

# SOP 执行（走 POST /api/execute/goal）
result = client.execute_sop("sop-id", goal="Analyze code", project_id="my-project")
print(result.report, result.output)
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
| `search(query, top_k)` | POST /api/vector/search | 向量搜索 |
| `list_sessions()` | GET /api/sessions | 列出会话（`SessionSummary[]`） |
| `get_session(id)` | GET /api/sessions/{id} | 会话详情（404 → `None`） |
| `create_session(name)` | POST /api/sessions | 创建会话（返回 `SessionDetail`） |
| `add_message(id, role, content)` | POST /api/sessions/{id}/messages | 追加消息（返回更新后的 `SessionDetail`） |
| `execute_sop(sop_id, goal, project_id=None, event_tx=None)` | POST /api/execute/goal | **SOP/目标**执行 → `GoalExecuteResult` |
| `execute_tool(tool_id, args_json, fuel_limit)` | POST /api/execute | **工具**执行 → `ToolExecuteResult` |
| `list_sops()` | GET /api/sops | 列出 SOP（`list[str]`） |
| `get_sop(sop_id)` | GET /api/sops/{id} | SOP 详情（需 `workspace_id`） |
| `get_dag(sop_id)` | GET /api/sops/{id} | `get_sop` 的兼容别名（返回原始 `dict`） |
| `list_skills()` | GET /api/skills | 列出技能（`name/description/version/author/tags/source`） |
| `install_skill(skill_id)` | POST /api/skills/install | 安装技能（201，无响应体 → `None`） |
| `validate_auth(api_key)` | POST /api/auth/validate | 验证 API key（返回 `bool`） |
| `cluster_status()` | GET /api/cluster/status | 集群协调只读快照 |
| `control_lease(key)` | GET /api/cluster/control/leases/{key} | 控制面租约只读查询 |
| `anomaly_reports(sop_id=None)` | GET /api/anomaly | 认知循环异常观测 |
| `list_shared_experiences()` | GET /api/experience/shared | 列出跨项目共享的经验 |
| `share_experience(exp_key)` | POST /api/experience/share | 分享经验 |
| `unshare_experience(exp_key, project_id)` | POST /api/experience/unshare | 取消分享 |

### 两个易错点

1. **`/api/execute` ≠ SOP 执行**：它是工具执行端点（`tool_id`/`args_json`）。
   SOP 执行请用 `execute_sop()`（内部走 `/api/execute/goal`），旧版本的
   `execute_sop(sop_id, context)` 签名已移除。
2. **控制面只读且不报错**：`control_lease()` 在控制面未启用时返回
   `status_code=DISABLED` 且 `found=False`，HTTP 仍为 200。请按
   `status_code`（`OK`/`NOT_FOUND`/`DISABLED`/`UNAVAILABLE`）分支处理。

## 测试

```bash
cd ide/python-sdk
pip install pytest
python -m pytest tests/
```

## 详细文档

参见 [SDK 参考](https://zerove.github.io/opencodepro/sdk-reference.html) 与
[book/src/sdk-reference.md](../../book/src/sdk-reference.md)。
