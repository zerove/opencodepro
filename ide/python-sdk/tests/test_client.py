"""
Python SDK 单元测试
使用 pytest + mock，模拟 HTTP 请求验证每个客户端方法
"""

import json
from unittest.mock import patch, MagicMock
import pytest
from sisyphus_sdk import (
    SisyphusClient,
    HealthResponse,
    Session,
    SearchResult,
    ExecuteResult,
    SopInfo,
    SkillEntry,
    SisyphusError,
    AnomalyReport,
    SharedExperience,
    ExperienceShareResult,
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
        """create_session() 应该返回新创建的 Session"""
        mock_urlopen.return_value = mock_response(
            {"id": "s3", "name": "New Session", "message_count": 0}
        )
        result = client.create_session("New Session")
        assert isinstance(result, Session)
        assert result.name == "New Session"

    def test_add_message(self, client, mock_urlopen):
        """add_message() 应该返回消息字典"""
        mock_urlopen.return_value = mock_response(
            {"role": "user", "content": "hello", "id": "m1"}
        )
        result = client.add_message("s1", "user", "hello")
        assert result["role"] == "user"
        assert result["content"] == "hello"


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

    def test_execute_sop(self, client, mock_urlopen):
        """execute_sop() 应该返回 ExecuteResult"""
        mock_urlopen.return_value = mock_response(
            {
                "sop_id": "sop1",
                "status": "completed",
                "iterations": 3,
                "deviation": 0.1,
            }
        )
        result = client.execute_sop("sop1", {"input": "hello"})
        assert isinstance(result, ExecuteResult)
        assert result.sop_id == "sop1"
        assert result.status == "completed"
        assert result.iterations == 3


# ── SOPs ─────────────────────────────────────────


class TestSops:
    """SOP 管理相关测试"""

    def test_list_sops(self, client, mock_urlopen):
        """list_sops() 应该返回 SopInfo 列表"""
        mock_urlopen.return_value = mock_response(
            [
                {
                    "id": "sop1",
                    "name": "SOP 1",
                    "node_count": 5,
                    "description": "Test SOP",
                }
            ]
        )
        result = client.list_sops()
        assert len(result) == 1
        assert isinstance(result[0], SopInfo)
        assert result[0].id == "sop1"

    def test_get_dag(self, client, mock_urlopen):
        """get_dag() 应该返回字典"""
        mock_urlopen.return_value = mock_response(
            {"nodes": [], "edges": []}
        )
        result = client.get_dag("sop1")
        assert isinstance(result, dict)
        assert "nodes" in result


# ── Skills ────────────────────────────────────────


class TestSkills:
    """技能管理相关测试"""

    def test_list_skills(self, client, mock_urlopen):
        """list_skills() 应该返回 SkillEntry 列表"""
        mock_urlopen.return_value = mock_response(
            [
                {
                    "id": "sk1",
                    "name": "Skill 1",
                    "description": "Test",
                    "author": "author",
                    "version": "1.0",
                    "downloads": 100,
                    "rating": 4.5,
                }
            ]
        )
        result = client.list_skills()
        assert len(result) == 1
        assert isinstance(result[0], SkillEntry)
        assert result[0].name == "Skill 1"

    def test_install_skill(self, client, mock_urlopen):
        """install_skill() 应该返回字典"""
        mock_urlopen.return_value = mock_response(
            {"success": True, "skill_id": "sk1"}
        )
        result = client.install_skill("sk1")
        assert result["success"] is True


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
