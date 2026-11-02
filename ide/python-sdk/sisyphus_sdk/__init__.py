"""
Sisyphus Python SDK — 认知引擎 REST API 客户端。

用法:
    from sisyphus_sdk import SisyphusClient

    client = SisyphusClient(base_url="http://localhost:8080")
    health = client.health()
    sessions = client.list_sessions()
    results = client.search("my query")
    result = client.execute_sop("sop-id")
"""

import json
from dataclasses import dataclass, field
from typing import Any, Optional
from urllib.request import Request, urlopen
from urllib.parse import quote
from urllib.error import HTTPError


@dataclass
class HealthResponse:
    status: str
    version: str
    uptime_secs: int


@dataclass
class Session:
    id: str
    name: str
    message_count: int
    created_at: Optional[str] = None
    parent_session_id: Optional[str] = None
    messages: list = field(default_factory=list)


@dataclass
class SearchResult:
    id: str
    score: float
    content: str


@dataclass
class ExecuteResult:
    sop_id: str
    status: str
    iterations: int
    deviation: float


@dataclass
class SopInfo:
    id: str
    name: str
    node_count: int
    description: str


@dataclass
class SkillEntry:
    id: str
    name: str
    description: str
    author: str
    version: str
    downloads: int
    rating: float


@dataclass
class AnomalyReport:
    """认知循环观测到的一条异常（GET /api/anomaly）。"""
    node_name: str
    severity: str
    metric: str
    observed: float
    expected: float
    description: str
    timestamp: int


@dataclass
class SharedExperience:
    """跨项目共享的经验条目（GET /api/experience/shared）。"""
    context_text: str
    context_hash: str
    action_id: str
    outcome_success: bool
    deviation_score: float
    cost: int
    timestamp: int
    version: int = 0
    vector_clock: dict = field(default_factory=dict)
    project_id: Optional[str] = None
    embedding: Optional[list] = None
    visual_embedding: Optional[list] = None
    insight: Optional[str] = None


@dataclass
class ExperienceShareResult:
    """经验分享 / 取消分享结果。"""
    success: bool
    error: str = ""


class SisyphusError(Exception):
    """Sisyphus SDK 基础错误。"""
    pass


class SisyphusClient:
    """Sisyphus 认知引擎 REST API 客户端。"""

    def __init__(self, base_url: str = "http://localhost:8080",
                 auth_token: Optional[str] = None,
                 timeout: int = 30):
        self.base_url = base_url.rstrip("/")
        self.auth_token = auth_token
        self.timeout = timeout

    def _request(self, method: str, path: str,
                 body: Optional[dict] = None) -> Any:
        url = f"{self.base_url}{path}"
        headers = {"Content-Type": "application/json"}
        if self.auth_token:
            headers["Authorization"] = f"Bearer {self.auth_token}"

        data = json.dumps(body).encode() if body else None
        req = Request(url, data=data, headers=headers, method=method)

        try:
            with urlopen(req, timeout=self.timeout) as resp:
                return json.loads(resp.read().decode())
        except HTTPError as e:
            raise SisyphusError(f"API error {e.code}: {e.read().decode()}")
        except Exception as e:
            raise SisyphusError(f"Request failed: {e}")

    # ── 健康检查 ─────────────────────────────────

    def health(self) -> HealthResponse:
        data = self._request("GET", "/health")
        return HealthResponse(**data)

    # ── 会话 ───────────────────────────────────

    def list_sessions(self) -> list[Session]:
        data = self._request("GET", "/api/sessions")
        return [Session(**s) for s in data]

    def get_session(self, session_id: str) -> Optional[Session]:
        try:
            data = self._request("GET", f"/api/sessions/{session_id}")
            return Session(**data)
        except SisyphusError:
            return None

    def create_session(self, name: str) -> Session:
        data = self._request("POST", "/api/sessions", {"name": name})
        return Session(**data)

    def add_message(self, session_id: str, role: str, content: str) -> dict:
        return self._request("POST", f"/api/sessions/{session_id}/messages",
                             {"role": role, "content": content})

    # ── 搜索 ─────────────────────────────────

    def search(self, query: str, top_k: int = 10) -> list[SearchResult]:
        data = self._request("POST", "/api/vector/search",
                             {"query": query, "top_k": top_k})
        return [SearchResult(**r) for r in data]

    # ── 执行 ──────────────────────────────────

    def execute_sop(self, sop_id: str,
                    context: Optional[dict] = None) -> ExecuteResult:
        data = self._request("POST", "/api/execute",
                             {"sop_id": sop_id, "context": context or {}})
        return ExecuteResult(**data)

    # ── SOP 管理 ───────────────────────────────

    def list_sops(self) -> list[SopInfo]:
        data = self._request("GET", "/api/sops")
        return [SopInfo(**s) for s in data]

    def get_dag(self, sop_id: str) -> dict:
        return self._request("GET", f"/api/sops/{sop_id}")

    # ── 技能 ─────────────────────────────────

    def list_skills(self) -> list[SkillEntry]:
        data = self._request("GET", "/api/skills")
        return [SkillEntry(**s) for s in data]

    def install_skill(self, skill_id: str) -> dict:
        return self._request("POST", "/api/skills/install",
                             {"skill_id": skill_id})

    # ── 认证 ───────────────────────────────────

    def validate_auth(self, api_key: str) -> bool:
        data = self._request("POST", "/api/auth/validate",
                             {"api_key": api_key})
        return data.get("valid", False)

    # ── 认知循环可观测性 ─────────────────────────

    def anomaly_reports(self, sop_id: Optional[str] = None) -> list[AnomalyReport]:
        """读取认知循环观测到的异常。

        `sop_id` 为空时返回全部作用域；服务端对未知作用域返回空列表，
        不会报错。
        """
        path = "/api/anomaly"
        if sop_id:
            path = f"{path}?sop_id={quote(sop_id, safe='')}"
        data = self._request("GET", path)
        return [AnomalyReport(**r) for r in data]

    # ── 经验共享 ────────────────────────────────

    def list_shared_experiences(self) -> list[SharedExperience]:
        """列出所有已跨项目共享的经验。"""
        data = self._request("GET", "/api/experience/shared")
        experiences = data.get("experiences", []) if isinstance(data, dict) else []
        return [SharedExperience(**exp) for exp in experiences]

    def share_experience(self, exp_key: str) -> ExperienceShareResult:
        """把一条本地经验标记为跨项目共享。"""
        data = self._request("POST", "/api/experience/share",
                             {"exp_key": exp_key})
        return ExperienceShareResult(**data)

    def unshare_experience(self, exp_key: str,
                           project_id: str) -> ExperienceShareResult:
        """取消某项目的经验共享。

        `project_id` 必填（服务端缺少时返回 400），用于限定取消共享的作用域。
        """
        data = self._request("POST", "/api/experience/unshare",
                             {"exp_key": exp_key, "project_id": project_id})
        return ExperienceShareResult(**data)


# ── 便捷异步封装 ──────────────────

async def async_health(base_url: str = "http://localhost:8080") -> HealthResponse:
    """使用 aiohttp 的非阻塞健康检查（可选依赖）。"""
    try:
        import aiohttp
        async with aiohttp.ClientSession() as session:
            async with session.get(f"{base_url}/health") as resp:
                data = await resp.json()
                return HealthResponse(**data)
    except ImportError:
        # 回退到同步实现
        return SisyphusClient(base_url).health()
