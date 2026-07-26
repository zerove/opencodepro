"""
Sisyphus Python SDK — REST API client for the cognitive engine.

Usage:
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
    """Base error for Sisyphus SDK."""
    pass


class SisyphusClient:
    """REST API client for the Sisyphus cognitive engine."""

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

    # ── Health ─────────────────────────────────

    def health(self) -> HealthResponse:
        data = self._request("GET", "/health")
        return HealthResponse(**data)

    # ── Sessions ───────────────────────────────

    def list_sessions(self) -> list[Session]:
        data = self._request("GET", "/api/v1/sessions")
        return [Session(**s) for s in data]

    def get_session(self, session_id: str) -> Optional[Session]:
        try:
            data = self._request("GET", f"/api/v1/sessions/{session_id}")
            return Session(**data)
        except SisyphusError:
            return None

    def create_session(self, name: str) -> Session:
        data = self._request("POST", "/api/v1/sessions", {"name": name})
        return Session(**data)

    def add_message(self, session_id: str, role: str, content: str) -> dict:
        return self._request("POST", f"/api/v1/sessions/{session_id}/messages",
                             {"role": role, "content": content})

    # ── Search ─────────────────────────────────

    def search(self, query: str, top_k: int = 10) -> list[SearchResult]:
        data = self._request("POST", "/api/v1/vector/search",
                             {"query": query, "top_k": top_k})
        return [SearchResult(**r) for r in data]

    # ── Execution ──────────────────────────────

    def execute_sop(self, sop_id: str,
                    context: Optional[dict] = None) -> ExecuteResult:
        data = self._request("POST", "/api/v1/execute",
                             {"sop_id": sop_id, "context": context or {}})
        return ExecuteResult(**data)

    # ── SOPs ───────────────────────────────────

    def list_sops(self) -> list[SopInfo]:
        data = self._request("GET", "/api/v1/sops")
        return [SopInfo(**s) for s in data]

    def get_dag(self, sop_id: str) -> dict:
        return self._request("GET", f"/api/v1/sops/{sop_id}/dag")

    # ── Skills ─────────────────────────────────

    def list_skills(self) -> list[SkillEntry]:
        data = self._request("GET", "/api/v1/skills")
        return [SkillEntry(**s) for s in data]

    def install_skill(self, skill_id: str) -> dict:
        return self._request("POST", "/api/v1/skills/install",
                             {"skill_id": skill_id})

    # ── Auth ───────────────────────────────────

    def validate_auth(self, api_key: str) -> bool:
        data = self._request("POST", "/api/auth/validate",
                             {"api_key": api_key})
        return data.get("valid", False)


# ── Convenience async wrapper ──────────────────

async def async_health(base_url: str = "http://localhost:8080") -> HealthResponse:
    """Non-blocking health check using aiohttp (optional dependency)."""
    try:
        import aiohttp
        async with aiohttp.ClientSession() as session:
            async with session.get(f"{base_url}/health") as resp:
                data = await resp.json()
                return HealthResponse(**data)
    except ImportError:
        # Fallback to sync
        return SisyphusClient(base_url).health()
