"""
Python SDK 单元测试
使用 pytest + mock，模拟 HTTP 请求验证每个客户端方法
"""

import json
from unittest.mock import patch, MagicMock
import pytest
from sisyphus_sdk import (
    LEASE_STATUS_DISABLED,
    LEASE_STATUS_OK,
    ClusterPeer,
    ClusterStatus,
    ControlLease,
    ExperienceShareResult,
    GoalExecuteResult,
    HealthResponse,
    Session,
    SessionDetail,
    SessionSummary,
    SearchResult,
    SkillEntry,
    SisyphusClient,
    SisyphusError,
    SopDetail,
    AnomalyReport,
    SharedExperience,
    ToolExecuteResult,
)


@pytest.fixture
def client():
    """创建一个测试用的客户端实例"""
    return SisyphusClient(base_url="http://test.local")


@pytest.fixture
def mock_urlopen():
    """模拟 urllib.request.urlopen 返回成功响应"""
    with patch("sisyphus_sdk.urlopen") as mock:
        yield mock


def mock_response(data, status=200):
    """创建一个模拟的 HTTP 响应"""
    resp = MagicMock()
    resp.__enter__.return_value = resp
    resp.read.return_value = json.dumps(data).encode()
    resp.status = status
    resp.__exit__.return_value = None
    return resp


# ── Health ─────────────────────────────────────────


class TestHealth:
    """健康检查相关测试"""

    def test_health_returns_health_response(self, client, mock_urlopen):
        """health() 应该返回 HealthResponse 对象"""
        mock_urlopen.return_value = mock_response(
            {"status": "ok", "version": "1.0.0", "uptime_secs": 42}
        )
        result = client.health()
        assert isinstance(result, HealthResponse)
        assert result.status == "ok"
        assert result.version == "1.0.0"
        assert result.uptime_secs == 42

    def test_health_sends_get_request(self, client, mock_urlopen):
        """health() 应该发送 GET 请求到 /health"""
        mock_urlopen.return_value = mock_response(
            {"status": "ok", "version": "1.0", "uptime_secs": 0}
        )
        client.health()
        req = mock_urlopen.call_args[0][0]
        assert req.method == "GET"
        assert req.full_url == "http://test.local/health"


# ── Sessions ───────────────────────────────────────


class TestSessions:
    """会话管理相关测试"""

    def test_list_sessions(self, client, mock_urlopen):
        """list_sessions() 应该返回 Session 列表"""
        mock_urlopen.return_value = mock_response(
            [
                {"id": "s1", "name": "Session 1", "message_count": 5},
                {"id": "s2", "name": "Session 2", "message_count": 3},
            ]
        )
        result = client.list_sessions()
        assert len(result) == 2
        assert all(isinstance(s, Session) for s in result)
        assert result[0].id == "s1"

    def test_get_session_exists(self, client, mock_urlopen):
        """get_session() 存在时应该返回 Session 对象"""
        mock_urlopen.return_value = mock_response(
            {"id": "s1", "name": "Session 1", "message_count": 0}
        )
        result = client.get_session("s1")
        assert result is not None
        assert result.id == "s1"

    def test_get_session_not_found(self, client, mock_urlopen):
        """get_session() 不存在时应该返回 None"""
        from urllib.error import HTTPError

        mock_urlopen.side_effect = HTTPError(
            "http://test.local/api/sessions/bad",
            404,
            "Not Found",
            {},
            None,
        )
        result = client.get_session("bad")
        assert result is None

    def test_create_session(self, client, mock_urlopen):
        """create_session() 应该返回新创建的会话详情"""
        mock_urlopen.return_value = mock_response(
            {
                "id": "s3",
                "name": "New Session",
                "message_count": 0,
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z",
                "messages": [],
            },
            201,
        )
        result = client.create_session("New Session")
        assert isinstance(result, SessionDetail)
        assert result.name == "New Session"
        assert result.messages == []

    def test_add_message(self, client, mock_urlopen):
        """add_message() 应该返回更新后的会话详情"""
        mock_urlopen.return_value = mock_response(
            {
                "id": "s1",
                "name": "Session 1",
                "message_count": 1,
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:01Z",
                "messages": [
                    {
                        "role": "user",
                        "content": "hello",
                        "created_at": "2026-01-01T00:00:01Z",
                    }
                ],
            }
        )
        result = client.add_message("s1", "user", "hello")
        assert isinstance(result, SessionDetail)
        assert result.message_count == 1
        assert result.messages[0].content == "hello"
        req = mock_urlopen.call_args[0][0]
        assert req.full_url.endswith("/api/sessions/s1/messages")


# ── Search ─────────────────────────────────────────


class TestSearch:
    """向量搜索相关测试"""

    def test_search_returns_search_results(self, client, mock_urlopen):
        """search() 应该返回 SearchResult 列表"""
        mock_urlopen.return_value = mock_response(
            [
                {"id": "r1", "score": 0.95, "content": "result 1"},
                {"id": "r2", "score": 0.85, "content": "result 2"},
            ]
        )
        result = client.search("query", top_k=5)
        assert len(result) == 2
        assert all(isinstance(r, SearchResult) for r in result)
        assert result[0].score == 0.95

    def test_search_sends_post_request(self, client, mock_urlopen):
        """search() 应该发送 POST 请求到 /api/vector/search"""
        mock_urlopen.return_value = mock_response([])
        client.search("my query", top_k=3)
        req = mock_urlopen.call_args[0][0]
        assert req.method == "POST"
        assert "/api/vector/search" in req.full_url
        body = json.loads(req.data.decode())
        assert body["query"] == "my query"
        assert body["top_k"] == 3


# ── Execution ───────────────────────────────────


class TestExecution:
    """SOP 执行相关测试"""

    def test_execute_sop_posts_to_goal_endpoint(self, client, mock_urlopen):
        """execute_sop() 必须走 POST /api/execute/goal（不是 /api/execute）"""
        mock_urlopen.return_value = mock_response(
            {"output": {"answer": 42}, "report": "ok"}
        )
        result = client.execute_sop("sop1", goal="Analyze code", project_id="proj1")
        assert isinstance(result, GoalExecuteResult)
        assert result.report == "ok"
        assert result.output == {"answer": 42}

        req = mock_urlopen.call_args[0][0]
        assert req.method == "POST"
        assert req.full_url.endswith("/api/execute/goal")
        assert json.loads(req.data.decode()) == {
            "sop_id": "sop1",
            "goal": "Analyze code",
            "project_id": "proj1",
        }

    def test_execute_sop_omits_optional_project(self, client, mock_urlopen):
        """未指定 project_id / event_tx 时不应发送这两个字段"""
        mock_urlopen.return_value = mock_response({"output": None, "report": ""})
        client.execute_sop("sop1", goal="goal")
        req = mock_urlopen.call_args[0][0]
        assert json.loads(req.data.decode()) == {"sop_id": "sop1", "goal": "goal"}

    def test_execute_tool_posts_to_tool_endpoint(self, client, mock_urlopen):
        """execute_tool() 走 POST /api/execute，args_json 保持字符串"""
        mock_urlopen.return_value = mock_response(
            {"status": "SUCCESS", "result_json": '{"ok":true}'}
        )
        args = '{"path":"/tmp/a.txt"}'
        result = client.execute_tool("file_read", args, fuel_limit=1000)
        assert isinstance(result, ToolExecuteResult)
        assert result.status == "SUCCESS"
        assert result.result_json == '{"ok":true}'

        req = mock_urlopen.call_args[0][0]
        assert req.full_url.endswith("/api/execute")
        body = json.loads(req.data.decode())
        assert body == {
            "tool_id": "file_read",
            "args_json": args,
            "fuel_limit": 1000,
        }


# ── SOPs ─────────────────────────────────────────


class TestSops:
    """SOP 管理相关测试"""

    def test_list_sops_returns_id_list(self, client, mock_urlopen):
        """list_sops() 返回服务端实际契约：SOP ID 字符串数组"""
        mock_urlopen.return_value = mock_response(["sop1", "sop2"])
        result = client.list_sops()
        assert result == ["sop1", "sop2"]

    def test_get_sop_returns_detail(self, client, mock_urlopen):
        """get_sop() 应该返回 SopDetail"""
        mock_urlopen.return_value = mock_response(
            {
                "id": "sop1",
                "name": "SOP 1",
                "version": "1.0.0",
                "config": {"nodes": []},
            }
        )
        result = client.get_sop("sop1")
        assert isinstance(result, SopDetail)
        assert result.id == "sop1"
        assert result.version == "1.0.0"
        assert result.config == {"nodes": []}

    def test_get_dag_returns_dict(self, client, mock_urlopen):
        """get_dag()（兼容别名）应该返回原始字典"""
        mock_urlopen.return_value = mock_response({"nodes": [], "edges": []})
        result = client.get_dag("sop1")
        assert isinstance(result, dict)
        assert "nodes" in result


# ── Skills ────────────────────────────────────────


class TestSkills:
    """技能管理相关测试"""

    def test_list_skills_uses_server_fields(self, client, mock_urlopen):
        """list_skills() 必须按服务端字段解码（无 id/downloads/rating）"""
        mock_urlopen.return_value = mock_response(
            [
                {
                    "name": "Skill 1",
                    "description": "Test",
                    "version": "1.0",
                    "author": "author",
                    "tags": ["demo"],
                    "source": "builtin",
                }
            ]
        )
        result = client.list_skills()
        assert len(result) == 1
        assert isinstance(result[0], SkillEntry)
        assert result[0].name == "Skill 1"
        assert result[0].tags == ["demo"]
        assert result[0].source == "builtin"

    def test_list_skills_tolerates_extra_server_fields(self, client, mock_urlopen):
        """服务端新增字段不应让 SDK 抛 TypeError"""
        mock_urlopen.return_value = mock_response(
            [{"name": "Skill 1", "future_field": "ignored"}]
        )
        result = client.list_skills()
        assert result[0].name == "Skill 1"

    def test_install_skill_accepts_empty_201_body(self, client, mock_urlopen):
        """install_skill() 成功时服务端返回 201 且无响应体"""
        resp = MagicMock()
        resp.__enter__.return_value = resp
        resp.read.return_value = b""
        resp.status = 201
        resp.__exit__.return_value = None
        mock_urlopen.return_value = resp

        assert client.install_skill("sk1") is None
        req = mock_urlopen.call_args[0][0]
        assert req.full_url.endswith("/api/skills/install")
        assert json.loads(req.data.decode()) == {"skill_id": "sk1"}


# ── Auth ─────────────────────────────────────────


class TestAuth:
    """认证相关测试"""

    def test_validate_auth_valid(self, client, mock_urlopen):
        """validate_auth() 对有效 key 应该返回 True"""
        mock_urlopen.return_value = mock_response(
            {"valid": True, "message": "ok"}
        )
        assert client.validate_auth("valid-key") is True

    def test_validate_auth_invalid(self, client, mock_urlopen):
        """validate_auth() 对无效 key 应该返回 False"""
        mock_urlopen.return_value = mock_response(
            {"valid": False, "message": "invalid"}
        )
        assert client.validate_auth("bad-key") is False


# ── 认知循环可观测性 ──────────────────────────────


class TestAnomalyReports:
    """异常观测端点测试"""

    def test_anomaly_reports_without_scope(self, client, mock_urlopen):
        """不带 sop_id 时不应该拼查询参数"""
        mock_urlopen.return_value = mock_response(
            [
                {
                    "node_name": "node_a",
                    "severity": "high",
                    "metric": "duration",
                    "observed": 42.0,
                    "expected": 10.0,
                    "description": "duration deviation",
                    "timestamp": 1_700_000_000,
                }
            ]
        )
        reports = client.anomaly_reports()
        assert len(reports) == 1
        assert isinstance(reports[0], AnomalyReport)
        assert reports[0].node_name == "node_a"
        assert reports[0].severity == "high"
        assert reports[0].observed == 42.0
        req = mock_urlopen.call_args[0][0]
        assert req.full_url.endswith("/api/anomaly")

    def test_anomaly_reports_with_scope_encodes_sop_id(self, client, mock_urlopen):
        """sop_id 应该被 URL 编码后作为查询参数"""
        mock_urlopen.return_value = mock_response([])
        reports = client.anomaly_reports("sop live/1")
        assert reports == []
        req = mock_urlopen.call_args[0][0]
        assert "sop_id=sop%20live%2F1" in req.full_url


# ── 经验共享 ──────────────────────────────────────


class TestExperienceSharing:
    """经验共享端点测试"""

    def test_list_shared_experiences_unwraps_experiences(self, client, mock_urlopen):
        """list_shared_experiences() 应该解包 experiences 字段"""
        mock_urlopen.return_value = mock_response(
            {
                "experiences": [
                    {
                        "context_text": "ctx",
                        "context_hash": "hash",
                        "action_id": "act",
                        "outcome_success": True,
                        "deviation_score": 0.0,
                        "cost": 12,
                        "timestamp": 1_700_000_000,
                        "version": 1,
                        "vector_clock": {"node_a": 1},
                    }
                ]
            }
        )
        experiences = client.list_shared_experiences()
        assert len(experiences) == 1
        assert isinstance(experiences[0], SharedExperience)
        assert experiences[0].action_id == "act"
        assert experiences[0].vector_clock == {"node_a": 1}

    def test_list_shared_experiences_empty(self, client, mock_urlopen):
        """服务端返回空数组时不应该报错"""
        mock_urlopen.return_value = mock_response({"experiences": []})
        assert client.list_shared_experiences() == []

    def test_share_experience_posts_exp_key(self, client, mock_urlopen):
        """share_experience() 应该 POST exp_key"""
        mock_urlopen.return_value = mock_response(
            {"success": True, "error": ""}
        )
        result = client.share_experience("exp:1")
        assert isinstance(result, ExperienceShareResult)
        assert result.success is True
        req = mock_urlopen.call_args[0][0]
        assert req.full_url.endswith("/api/experience/share")
        assert json.loads(req.data.decode()) == {"exp_key": "exp:1"}

    def test_unshare_experience_sends_project_id(self, client, mock_urlopen):
        """unshare_experience() 应该同时发送 exp_key 与 project_id"""
        mock_urlopen.return_value = mock_response(
            {"success": True, "error": ""}
        )
        client.unshare_experience("exp:1", "proj1")
        req = mock_urlopen.call_args[0][0]
        assert req.full_url.endswith("/api/experience/unshare")
        assert json.loads(req.data.decode()) == {
            "exp_key": "exp:1",
            "project_id": "proj1",
        }

    def test_unshare_experience_failure_is_reported(self, client, mock_urlopen):
        """失败时 success=False 且 error 非空"""
        mock_urlopen.return_value = mock_response(
            {"success": False, "error": "experience not found"}
        )
        result = client.unshare_experience("missing", "proj1")
        assert result.success is False
        assert result.error == "experience not found"


# ── 集群 / 控制面只读观测 ─────────────────────────


class TestClusterObservation:
    """集群与控制面只读端点测试"""

    def test_cluster_status_parses_nested_views(self, client, mock_urlopen):
        """cluster_status() 应该解析嵌套的配置/安全/peer 视图"""
        mock_urlopen.return_value = mock_response(
            {
                "enabled": True,
                "cluster_name": "demo",
                "config": {
                    "node_id": "node-1",
                    "peer_ttl_secs": 30,
                    "join_timeout_secs": 5,
                    "strategy_sync_interval_secs": 10,
                    "static_peers": ["node-2:50051"],
                    "control_plane_enabled": True,
                    "control_plane": {
                        "enabled": True,
                        "propose_timeout_ms": 2000,
                        "connect_timeout_ms": 1000,
                        "lease_ttl_min_secs": 5,
                        "lease_ttl_max_secs": 300,
                    },
                },
                "control_plane": {
                    "configured": True,
                    "coordinator_configured": True,
                    "security": {
                        "tls_enabled_config": True,
                        "tls_wired": True,
                        "submit_auth_mode_config": "mtls_subject_allowlist",
                        "submit_auth_enforced": True,
                        "app_layer_submit_auth_enforced": True,
                        "effective_armed": True,
                        "allowed_submit_subjects_count": 3,
                        "note": "ADR-019",
                    },
                    "stats": {"accepts": 2},
                },
                "peer_manager": {
                    "self_node_id": "node-1",
                    "leader_id": "node-1",
                    "is_leader": True,
                    "cluster_size": 2,
                    "state_proposal_broadcasts": 7,
                    "experience_sync": {
                        "broadcasts": 1,
                        "inbound_delivered": 2,
                        "inbound_applied": 2,
                        "inbound_rejected": 0,
                        "inbound_rejected_deserialize": 0,
                        "inbound_rejected_quality": 0,
                        "inbound_rejected_apply": 0,
                    },
                },
                "consensus": {"leader_id": "node-1", "is_leader": True},
                "peers": [
                    {
                        "node_id": "node-1",
                        "grpc_addr": "http://127.0.0.1:50051",
                        "joined_at": 0,
                        "last_seen_secs_ago": 0,
                    },
                    {
                        "node_id": "node-2",
                        "grpc_addr": "http://127.0.0.1:50052",
                        "joined_at": 0,
                        "last_seen_secs_ago": None,
                    },
                ],
            }
        )
        status = client.cluster_status()
        assert isinstance(status, ClusterStatus)
        assert status.enabled is True
        assert status.cluster_name == "demo"

        # 嵌套 dataclass 而不是原始 dict（否则调用方要自己拆 JSON）
        assert status.config.node_id == "node-1"
        assert status.config.control_plane.lease_ttl_max_secs == 300
        assert status.control_plane.security.tls_wired is True
        assert status.control_plane.security.allowed_submit_subjects_count == 3
        assert status.control_plane.stats == {"accepts": 2}
        assert status.peer_manager.experience_sync.inbound_applied == 2
        assert status.peer_manager.cluster_size == 2
        assert status.consensus.is_leader is True

        assert len(status.peers) == 2
        assert isinstance(status.peers[0], ClusterPeer)
        assert status.peers[1].last_seen_secs_ago is None

        req = mock_urlopen.call_args[0][0]
        assert req.method == "GET"
        assert req.full_url.endswith("/api/cluster/status")

    def test_cluster_status_tolerates_disabled_and_missing_fields(self, client, mock_urlopen):
        """单节点/未启用时字段缺失不应抛异常"""
        mock_urlopen.return_value = mock_response({"enabled": False, "peers": []})
        status = client.cluster_status()
        assert status.enabled is False
        assert status.peers == []
        assert status.config.control_plane.enabled is False
        assert status.control_plane.configured is False

    def test_control_lease_disabled_is_not_an_error(self, client, mock_urlopen):
        """控制面关闭时返回 200 + DISABLED（不是异常）"""
        mock_urlopen.return_value = mock_response(
            {
                "found": False,
                "key": "lease:demo",
                "owner": "",
                "generation": 0,
                "expires_at_unix_ms": 0,
                "last_request_id": "",
                "status_code": LEASE_STATUS_DISABLED,
                "status_message": "cluster.control_plane.enabled is false (ADR-019 gate)",
            }
        )
        lease = client.control_lease("lease:demo")
        assert isinstance(lease, ControlLease)
        assert lease.found is False
        assert lease.status_code == LEASE_STATUS_DISABLED

    def test_control_lease_encodes_key(self, client, mock_urlopen):
        """key 必须被 URL 编码后拼进路径"""
        mock_urlopen.return_value = mock_response(
            {
                "found": True,
                "key": "lease/demo",
                "owner": "node-1",
                "generation": 4,
                "expires_at_unix_ms": 1_700_000_000_000,
                "last_request_id": "req-1",
                "status_code": LEASE_STATUS_OK,
                "status_message": "",
            }
        )
        lease = client.control_lease("lease/demo")
        assert lease.found is True
        assert lease.owner == "node-1"
        assert lease.generation == 4
        assert lease.status_code == LEASE_STATUS_OK

        req = mock_urlopen.call_args[0][0]
        assert "/api/cluster/control/leases/lease%2Fdemo" in req.full_url


# ── 错误处理 ──────────────────────────────────────


class TestErrors:
    """错误处理相关测试"""

    def test_http_error_raises_sisyphus_error(self, client, mock_urlopen):
        """HTTP 错误应该抛出 SisyphusError"""
        from urllib.error import HTTPError

        mock_urlopen.side_effect = HTTPError(
            "http://test.local/health",
            500,
            "Internal Server Error",
            {},
            None,
        )
        with pytest.raises(SisyphusError, match="500"):
            client.health()

    def test_connection_error_raises_sisyphus_error(self, client, mock_urlopen):
        """网络错误应该抛出 SisyphusError"""
        mock_urlopen.side_effect = Exception("Connection refused")
        with pytest.raises(SisyphusError, match="Connection refused"):
            client.health()


# ── 构造器 ───────────────────────────────────────


class TestConstructor:
    """构造器相关测试"""

    def test_default_base_url(self):
        """默认 base_url 应该是 http://localhost:8080"""
        c = SisyphusClient()
        assert c.base_url == "http://localhost:8080"

    def test_strips_trailing_slash(self):
        """构造器应该去除 base_url 尾部斜杠"""
        c = SisyphusClient(base_url="http://test.com/")
        assert c.base_url == "http://test.com"

    def test_auth_token_used_in_header(self, mock_urlopen):
        """auth_token 应该在请求中发送 Authorization header"""
        c = SisyphusClient(base_url="http://test.local", auth_token="my-token")
        mock_urlopen.return_value = mock_response(
            {"status": "ok", "version": "1.0", "uptime_secs": 100}
        )
        c.health()
        req = mock_urlopen.call_args[0][0]
        assert req.headers["Authorization"] == "Bearer my-token"

    def test_workspace_id_used_in_header(self, mock_urlopen):
        """workspace_id 应该作为 X-Sisyphus-Workspace-Id 头发送"""
        c = SisyphusClient(base_url="http://test.local", workspace_id="proj1")
        mock_urlopen.return_value = mock_response(
            {"id": "sop1", "name": "SOP 1", "config": {}}
        )
        c.get_sop("sop1")
        req = mock_urlopen.call_args[0][0]
        assert req.headers["X-sisyphus-workspace-id"] == "proj1"
