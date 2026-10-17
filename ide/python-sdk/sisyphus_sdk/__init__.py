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
