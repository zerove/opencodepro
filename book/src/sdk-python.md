# Python SDK

包名 `sisyphus-sdk`，源码位于 `ide/python-sdk/`。

## 安装

```bash
pip install sisyphus-sdk
```

（本仓库 `ide/python-sdk/` 是开发源。生产发布待 PyPI 上线。）

## 快速开始

```python
from sisyphus_sdk import SisyphusClient

client = SisyphusClient(
    base_url="http://localhost:8080",
    auth_token="...",
    workspace_id="my-project",  # GET /api/sops/{id} 等端点要求该头
)

health = client.health()
result = client.execute_sop("sop-id", goal="Analyze code", project_id="my-project")
```

## 方法速查

| 方法 | 端点 | 说明 |
|------|------|------|
| `health()` | GET /health | 健康检查 |
| `search` | POST /api/vector/search | 向量搜索 |
| `list_sessions/get_session/create_session/add_message` | GET/GET/POST/POST /api/sessions… | 会话 |
| `execute_sop` | POST /api/execute/goal | **SOP/目标**执行 → `GoalExecuteResult` |
| `execute_tool` | POST /api/execute | **工具**执行 → `ToolExecuteResult` |
| `list_sops/get_sop/get_dag` | GET /api/sops… | SOP 列表 / 详情 / 别名 |
| `list_skills/install_skill` | GET /api/skills、POST /api/skills/install | 技能 |
| `validate_auth` | POST /api/auth/validate | API key 校验 |
| `cluster_status/control_lease` | GET /api/cluster/status、GET /api/cluster/control/leases/{key} | 集群 / 控制面只读观测 |
| `anomaly_reports` | GET /api/anomaly | 认知循环异常 |
| `list_shared_experiences/share_experience/unshare_experience` | GET/POST/POST /api/experience… | 经验共享 |

## 集群 / 控制面只读观测

```python
status = client.cluster_status()
print(status.enabled, status.control_plane.security.effective_armed)

lease = client.control_lease("lease:demo")
if lease.status_code == "DISABLED":
    print(lease.status_message)
```

租约查询是只读端点（ADR-020）。控制面未启用时仍返回 200，`found=False`、
`status_code=DISABLED`，调用方按 `status_code` 分支处理，SDK 不抛异常。

## 错误

网络 / HTTP 错误统一抛出 `SisyphusError`（含 API error 信息）。

## 测试

```bash
cd ide/python-sdk
pip install pytest
python -m pytest tests/
```
