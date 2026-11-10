"""
Sisyphus Python SDK — 认知引擎 REST API 客户端。

用法:
    from sisyphus_sdk import SisyphusClient

    client = SisyphusClient(base_url="http://localhost:8080")
    health = client.health()
    sessions = client.list_sessions()
    results = client.search("my query")
    result = client.execute_sop("sop-id", goal="Analyze code")
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
class SessionSummary:
    """会话列表条目（GET /api/sessions）。"""
    id: str
    name: str
    message_count: int = 0
    created_at: str = ""
    updated_at: str = ""


#: 兼容别名：旧版本把列表条目命名为 `Session`。
Session = SessionSummary


@dataclass
class MessageDto:
    """会话中的一条消息。"""
    role: str
    content: str
    created_at: str = ""
    estimated_tokens: int = 0


@dataclass
class SessionDetail:
    """会话详情（GET /api/sessions/{id}、POST /api/sessions、
    POST /api/sessions/{id}/messages 的共同结构）。"""
    id: str
    name: str
    message_count: int = 0
    created_at: str = ""
    updated_at: str = ""
    messages: list = field(default_factory=list)


@dataclass
class SearchResult:
    id: str
    score: float
    content: str


@dataclass
class GoalExecuteResult:
    """SOP 目标执行结果（POST /api/execute/goal）。

    `output` 是服务端 `NodeOutput` 的 JSON（任意结构），
    `report` 是执行报告文本。
    """
    output: Any
    report: str


@dataclass
class ToolExecuteResult:
    """工具执行结果（POST /api/execute）。

    注意：`/api/execute` 是**工具**执行端点（tool_id/args_json），
    不是 SOP 执行 —— SOP 执行请用 `execute_sop()`（走 /api/execute/goal）。
    """
    status: str
    result_json: str


@dataclass
class SopDetail:
    """SOP 详情（GET /api/sops/{id}）。"""
    id: str
    name: str = ""
    version: Optional[str] = None
    config: Any = None


@dataclass
class SkillEntry:
    """技能条目（GET /api/skills）。

    服务端字段为 name/description/version/author/tags/source；
    没有 id/downloads/rating。
    """
    name: str
    description: str = ""
    version: str = ""
    author: str = ""
    tags: list = field(default_factory=list)
    source: str = ""


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


# ── 集群 / 控制面只读观测（GET /api/cluster/status） ────────────


@dataclass
class ClusterPeer:
    """集群 peer 条目。"""
    node_id: str
    grpc_addr: str = ""
    joined_at: int = 0
    last_seen_secs_ago: Optional[int] = None


@dataclass
class ExperienceSyncStats:
    """经验同步的累计计数。"""
    broadcasts: int = 0
    inbound_delivered: int = 0
    inbound_applied: int = 0
    inbound_rejected: int = 0
    inbound_rejected_deserialize: int = 0
    inbound_rejected_quality: int = 0
    inbound_rejected_apply: int = 0


@dataclass
class PeerManagerView:
    """peer manager 的只读视图。"""
    self_node_id: Optional[str] = None
    leader_id: Optional[str] = None
    is_leader: bool = False
    cluster_size: int = 1
    state_proposal_broadcasts: int = 0
    experience_sync: ExperienceSyncStats = field(default_factory=ExperienceSyncStats)


@dataclass
class ConsensusView:
    """consensus trait 视图。"""
    leader_id: Optional[str] = None
    is_leader: bool = False


@dataclass
class ControlPlaneConfigView:
    """`[cluster.control_plane]` 的生效配置快照。"""
    enabled: bool = False
    propose_timeout_ms: int = 0
    connect_timeout_ms: int = 0
    lease_ttl_min_secs: int = 0
    lease_ttl_max_secs: int = 0


@dataclass
class ClusterConfigView:
    """`[cluster]` 的生效配置快照。"""
    node_id: str = ""
    peer_ttl_secs: int = 0
    join_timeout_secs: int = 0
    strategy_sync_interval_secs: int = 0
    static_peers: list = field(default_factory=list)
    control_plane_enabled: bool = False
    control_plane: ControlPlaneConfigView = field(default_factory=ControlPlaneConfigView)


@dataclass
class ControlPlaneSecurityView:
    """控制面安全装配的运行时视图（ADR-019）。"""
    tls_enabled_config: bool = False
    tls_wired: bool = False
    submit_auth_mode_config: str = ""
    submit_auth_enforced: bool = False
    app_layer_submit_auth_enforced: bool = False
    effective_armed: bool = False
    allowed_submit_subjects_count: int = 0
    note: str = ""


@dataclass
class ControlPlaneStatus:
    """控制面运行时状态（只读）。"""
    configured: bool = False
    coordinator_configured: bool = False
    security: ControlPlaneSecurityView = field(default_factory=ControlPlaneSecurityView)
    stats: Any = None


@dataclass
class ClusterStatus:
    """GET /api/cluster/status 的响应。"""
    enabled: bool = False
    cluster_name: str = ""
    config: ClusterConfigView = field(default_factory=ClusterConfigView)
    control_plane: ControlPlaneStatus = field(default_factory=ControlPlaneStatus)
    peer_manager: PeerManagerView = field(default_factory=PeerManagerView)
    consensus: ConsensusView = field(default_factory=ConsensusView)
    peers: list = field(default_factory=list)


#: 租约查询的状态码（GET /api/cluster/control/leases/{key}）。
LEASE_STATUS_OK = "OK"
LEASE_STATUS_NOT_FOUND = "NOT_FOUND"
LEASE_STATUS_DISABLED = "DISABLED"
LEASE_STATUS_UNAVAILABLE = "UNAVAILABLE"


@dataclass
class ControlLease:
    """GET /api/cluster/control/leases/{key} 的响应。

    该端点只读（ADR-020：REST 写入=否）。控制面未启用时返回
    `found=False` + `status_code=DISABLED`，而不是 HTTP 错误。
    """
    found: bool = False
    key: str = ""
    owner: str = ""
    generation: int = 0
    expires_at_unix_ms: int = 0
    last_request_id: str = ""
    status_code: str = ""
    status_message: str = ""


class SisyphusError(Exception):
    """Sisyphus SDK 基础错误。"""

    def __init__(
        self,
        message: str,
        status: Optional[int] = None,
        hint: Optional[str] = None,
        body: Any = None,
    ):
        super().__init__(message)
        self.status = status
        self.hint = hint
        self.body = body


def _build(cls, data):
    """按 dataclass 已知字段构造实例，忽略服务端新增字段。

    服务端会随版本新增字段（例如 `updated_at`）；用 `**data` 直接构造会
    让 SDK 在服务端升级后抛 `TypeError`。这里只取 dataclass 声明的字段，
    新字段被忽略 —— 对上层是兼容性收益，而不是静默错误。
    """
    names = getattr(cls, "__dataclass_fields__", {})
    kwargs = {k: v for k, v in data.items() if k in names}
    return cls(**kwargs)


def _session_detail(data):
    """构造 SessionDetail，并把 messages 逐条转成 MessageDto。"""
    detail = _build(SessionDetail, data)
    detail.messages = [_build(MessageDto, m) for m in (data.get("messages") or [])]
    return detail


class SisyphusClient:
    """Sisyphus 认知引擎 REST API 客户端。"""

    def __init__(self, base_url: str = "http://localhost:8080",
                 auth_token: Optional[str] = None,
                 timeout: int = 30,
                 workspace_id: Optional[str] = None):
        self.base_url = base_url.rstrip("/")
        self.auth_token = auth_token
        self.timeout = timeout
        #: 工作区标识，作为 X-Sisyphus-Workspace-Id 头发送。
        #: `GET /api/sops/{id}` 等服务端按工作区隔离的端点要求该头，缺失时 400。
        self.workspace_id = workspace_id

    def _request(self, method: str, path: str,
                 body: Optional[dict] = None) -> Any:
        url = f"{self.base_url}{path}"
        headers = {"Content-Type": "application/json"}
        if self.auth_token:
            headers["Authorization"] = f"Bearer {self.auth_token}"
        if self.workspace_id:
            headers["X-Sisyphus-Workspace-Id"] = self.workspace_id

        data = json.dumps(body).encode() if body else None
        req = Request(url, data=data, headers=headers, method=method)

        try:
            with urlopen(req, timeout=self.timeout) as resp:
                raw = resp.read()
                # 201/204 等成功响应可能没有响应体（例如技能安装）。
                if not raw:
                    return None
                return json.loads(raw.decode())
        except HTTPError as e:
            raw_body = e.read().decode(errors="replace")
            err_msg = raw_body
            hint = None
            parsed = None
            if raw_body:
                try:
                    parsed = json.loads(raw_body)
                    if isinstance(parsed, dict):
                        if isinstance(parsed.get("error"), str):
                            err_msg = parsed["error"]
                        if isinstance(parsed.get("hint"), str):
                            hint = parsed["hint"]
                except Exception:
                    parsed = None
            msg = f"API error {e.code}: {err_msg}"
            if hint:
                msg += f" (hint: {hint})"
            raise SisyphusError(msg, status=e.code, hint=hint, body=parsed if parsed is not None else raw_body)
        except SisyphusError:
            raise
        except Exception as e:
            raise SisyphusError(f"Request failed: {e}")

    # ── 健康检查 ─────────────────────────────────

    def health(self) -> HealthResponse:
        data = self._request("GET", "/health")
        return _build(HealthResponse, data)

    # ── 会话 ───────────────────────────────────

    def list_sessions(self) -> list[SessionSummary]:
        data = self._request("GET", "/api/sessions")
        return [_build(SessionSummary, s) for s in data]

    def get_session(self, session_id: str) -> Optional[SessionDetail]:
        try:
            data = self._request("GET", f"/api/sessions/{quote(session_id, safe='')}")
            return _session_detail(data)
        except SisyphusError:
            return None

    def create_session(self, name: str) -> SessionDetail:
        data = self._request("POST", "/api/sessions", {"name": name})
        return _session_detail(data)

    def add_message(self, session_id: str, role: str, content: str) -> SessionDetail:
        data = self._request(
            "POST",
            f"/api/sessions/{quote(session_id, safe='')}/messages",
            {"role": role, "content": content},
        )
        return _session_detail(data)

    # ── 搜索 ─────────────────────────────────

    def search(self, query: str, top_k: int = 10) -> list[SearchResult]:
        data = self._request("POST", "/api/vector/search",
                             {"query": query, "top_k": top_k})
        return [_build(SearchResult, r) for r in data]

    # ── 执行 ──────────────────────────────────

    def execute_sop(self, sop_id: str, goal: str,
                    project_id: Optional[str] = None,
                    event_tx: Optional[bool] = None) -> GoalExecuteResult:
        """通过 SOP 执行认知目标（POST /api/execute/goal）。

        注意：`/api/execute` 是工具执行端点，不是 SOP 执行；旧版本 SDK 把
        两者混为一谈，请求体会被服务端拒绝。
        """
        body: dict = {"sop_id": sop_id, "goal": goal}
        if project_id is not None:
            body["project_id"] = project_id
        if event_tx is not None:
            body["event_tx"] = event_tx
        data = self._request("POST", "/api/execute/goal", body)
        return _build(GoalExecuteResult, data)

    def execute_tool(self, tool_id: str, args_json: str = "{}",
                     fuel_limit: int = 0) -> ToolExecuteResult:
        """按工具 ID 执行工具（POST /api/execute）。"""
        body: dict = {"tool_id": tool_id, "args_json": args_json}
        if fuel_limit > 0:
            body["fuel_limit"] = fuel_limit
        data = self._request("POST", "/api/execute", body)
        return _build(ToolExecuteResult, data)

    # ── SOP 管理 ───────────────────────────────

    def list_sops(self) -> list[str]:
        """返回可用的 SOP ID 列表（服务端返回字符串数组）。"""
        return self._request("GET", "/api/sops")

    def get_sop(self, sop_id: str) -> SopDetail:
        """获取单个 SOP 的配置（GET /api/sops/{id}）。

        该端点按工作区隔离：服务端要求 X-Sisyphus-Workspace-Id 头，
        因此需在构造器中配置 `workspace_id`，否则服务端返回 400。
        """
        data = self._request("GET", f"/api/sops/{quote(sop_id, safe='')}")
        return _build(SopDetail, data)

    def get_dag(self, sop_id: str) -> dict:
        """已废弃别名：返回 SOP 详情（同 `get_sop`）。"""
        return self._request("GET", f"/api/sops/{quote(sop_id, safe='')}")

    # ── 技能 ─────────────────────────────────

    def list_skills(self) -> list[SkillEntry]:
        data = self._request("GET", "/api/skills")
        return [_build(SkillEntry, s) for s in data]

    def install_skill(self, skill_id: str) -> None:
        """安装远端技能（POST /api/skills/install；成功返回 201，无响应体）。"""
        self._request("POST", "/api/skills/install", {"skill_id": skill_id})

    # ── 集群 / 控制面只读观测 ─────────────────

    def cluster_status(self) -> ClusterStatus:
        """读取集群协调的只读快照（GET /api/cluster/status）。"""
        data = self._request("GET", "/api/cluster/status") or {}
        config_raw = data.get("config") or {}
        config = _build(ClusterConfigView, config_raw)
        config.control_plane = _build(
            ControlPlaneConfigView, config_raw.get("control_plane") or {}
        )
        control_plane_raw = data.get("control_plane") or {}
        control_plane = _build(ControlPlaneStatus, control_plane_raw)
        control_plane.security = _build(
            ControlPlaneSecurityView, control_plane_raw.get("security") or {}
        )
        peer_manager_raw = data.get("peer_manager") or {}
        peer_manager = _build(PeerManagerView, peer_manager_raw)
        peer_manager.experience_sync = _build(
            ExperienceSyncStats, peer_manager_raw.get("experience_sync") or {}
        )
        return ClusterStatus(
            enabled=bool(data.get("enabled", False)),
            cluster_name=data.get("cluster_name", ""),
            config=config,
            control_plane=control_plane,
            peer_manager=peer_manager,
            consensus=_build(ConsensusView, data.get("consensus") or {}),
            peers=[_build(ClusterPeer, p) for p in (data.get("peers") or [])],
        )

    def control_lease(self, key: str) -> ControlLease:
        """读取指定 key 的控制面租约（只读）。

        控制面未启用 / 租约不存在时服务端仍返回 200，请以
        `status_code` 判定（OK / NOT_FOUND / DISABLED / UNAVAILABLE）。
        """
        data = self._request(
            "GET", f"/api/cluster/control/leases/{quote(key, safe='')}"
        ) or {}
        return _build(ControlLease, data)

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
        return [_build(AnomalyReport, r) for r in data]

    # ── 经验共享 ────────────────────────────────

    def list_shared_experiences(self) -> list[SharedExperience]:
        """列出所有已跨项目共享的经验。"""
        data = self._request("GET", "/api/experience/shared")
        experiences = data.get("experiences", []) if isinstance(data, dict) else []
        return [_build(SharedExperience, exp) for exp in experiences]

    def share_experience(self, exp_key: str) -> ExperienceShareResult:
        """把一条本地经验标记为跨项目共享。"""
        data = self._request("POST", "/api/experience/share",
                             {"exp_key": exp_key})
        return _build(ExperienceShareResult, data)

    def unshare_experience(self, exp_key: str,
                           project_id: str) -> ExperienceShareResult:
        """取消某项目的经验共享。

        `project_id` 必填（服务端缺少时返回 400），用于限定取消共享的作用域。
        """
        data = self._request("POST", "/api/experience/unshare",
                             {"exp_key": exp_key, "project_id": project_id})
        return _build(ExperienceShareResult, data)


# ── 便捷异步封装 ──────────────────

async def async_health(base_url: str = "http://localhost:8080") -> HealthResponse:
    """使用 aiohttp 的非阻塞健康检查（可选依赖）。"""
    try:
        import aiohttp
        async with aiohttp.ClientSession() as session:
            async with session.get(f"{base_url}/health") as resp:
                data = await resp.json()
                return _build(HealthResponse, data)
    except ImportError:
        # 回退到同步实现
        return SisyphusClient(base_url).health()
