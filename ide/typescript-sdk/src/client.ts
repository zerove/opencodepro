// Sisyphus TypeScript SDK — REST API 客户端
//
// 端点契约以 `crates/sisyphus-bridge/src/http/` 为准。特别注意：
//   POST /api/execute      → 工具执行（tool_id / args_json）
//   POST /api/execute/goal → SOP 目标执行（goal / sop_id）

import type {
  SisyphusConfig,
  HealthResponse,
  StateEntry,
  SearchResult,
  SessionSummary,
  SessionDetail,
  GoalExecuteResult,
  ToolExecuteResult,
  SopDetail,
  SkillEntry,
  AnomalyReport,
  SharedExperience,
  ExperienceShareResult,
  ClusterStatus,
  ControlLease,
} from './types';

const DEFAULT_BASE_URL = 'http://localhost:8080';
const DEFAULT_TIMEOUT = 30_000;

/** 服务端返回非 2xx 时抛出的错误，携带 HTTP 状态码。 */
export class SisyphusApiError extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = 'SisyphusApiError';
    this.status = status;
  }
}

export class SisyphusClient {
  private baseUrl: string;
  private timeout: number;
  private authToken?: string;
  private workspaceId?: string;

  constructor(config: SisyphusConfig = {}) {
    this.baseUrl = (config.baseUrl || DEFAULT_BASE_URL).replace(/\/$/, '');
    this.timeout = config.timeoutMs || DEFAULT_TIMEOUT;
    this.authToken = config.authToken;
    this.workspaceId = config.workspaceId;
  }

  private async request<T>(
    method: string,
    path: string,
    body?: unknown,
  ): Promise<T> {
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
    };
    if (this.authToken) {
      headers['Authorization'] = `Bearer ${this.authToken}`;
    }
    if (this.workspaceId) {
      headers['X-Sisyphus-Workspace-Id'] = this.workspaceId;
    }

    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeout);

    try {
      const resp = await fetch(`${this.baseUrl}${path}`, {
        method,
        headers,
        body: body === undefined ? undefined : JSON.stringify(body),
        signal: controller.signal,
      });

      if (!resp.ok) {
        const text = await resp.text().catch(() => '');
        throw new SisyphusApiError(
          resp.status,
          `Sisyphus API error ${resp.status}: ${text || resp.statusText}`,
        );
      }

      // 201/204 等成功响应可能没有响应体（例如技能安装）。
      const text = await resp.text();
      return (text ? JSON.parse(text) : undefined) as T;
    } finally {
      clearTimeout(timer);
    }
  }

  // ── 健康检查 ──────────────────────────────────────

  async health(): Promise<HealthResponse> {
    return this.request<HealthResponse>('GET', '/health');
  }

  // ── 状态 CRUD ─────────────────────────────────

  async getState(projectId: string, key: string): Promise<StateEntry | null> {
    try {
      return await this.request<StateEntry>(
        'GET',
        `/api/state/${encodeURIComponent(key)}?project_id=${encodeURIComponent(projectId)}`,
      );
    } catch (e) {
      if (e instanceof SisyphusApiError && e.status === 404) return null;
      throw e;
    }
  }

  /** 写入状态（服务端返回 204，无响应体）。 */
  async setState(
    projectId: string,
    key: string,
    value: unknown,
  ): Promise<void> {
    await this.request<undefined>('PUT', `/api/state/${encodeURIComponent(key)}`, {
      project_id: projectId,
      value,
    });
  }

  async listState(projectId: string): Promise<StateEntry[]> {
    return this.request<StateEntry[]>(
      'GET',
      `/api/state?project_id=${encodeURIComponent(projectId)}`,
    );
  }

  // ── 向量搜索 ───────────────────────────────

  async vectorSearch(
    query: string,
    topK: number = 5,
  ): Promise<SearchResult[]> {
    return this.request<SearchResult[]>('POST', '/api/vector/search', {
      query,
      top_k: topK,
    });
  }

  // ── 会话 ────────────────────────────────────

  async listSessions(): Promise<SessionSummary[]> {
    return this.request<SessionSummary[]>('GET', '/api/sessions');
  }

  async getSession(id: string): Promise<SessionDetail | null> {
    try {
      return await this.request<SessionDetail>(
        'GET',
        `/api/sessions/${encodeURIComponent(id)}`,
      );
    } catch (e) {
      if (e instanceof SisyphusApiError && e.status === 404) return null;
      throw e;
    }
  }

  async createSession(name: string): Promise<SessionDetail> {
    return this.request<SessionDetail>('POST', '/api/sessions', { name });
  }

  /** 向会话追加一条消息，返回更新后的会话详情。 */
  async addMessage(
    sessionId: string,
    role: string,
    content: string,
  ): Promise<SessionDetail> {
    return this.request<SessionDetail>(
      'POST',
      `/api/sessions/${encodeURIComponent(sessionId)}/messages`,
      { role, content },
    );
  }

  // ── 执行 ───────────────────────────────────

  /**
   * 通过 SOP 执行认知目标（`POST /api/execute/goal`）。
   *
   * 注意：`/api/execute` 是**工具**执行端点，不是 SOP 执行；旧版本 SDK 把
   * 两者混为一谈，请求体会被服务端拒绝。
   */
  async executeSop(
    sopId: string,
    goal: string,
    projectId?: string,
    eventTx?: boolean,
  ): Promise<GoalExecuteResult> {
    const body: Record<string, unknown> = { sop_id: sopId, goal };
    if (projectId !== undefined) body['project_id'] = projectId;
    if (eventTx !== undefined) body['event_tx'] = eventTx;
    return this.request<GoalExecuteResult>('POST', '/api/execute/goal', body);
  }

  /**
   * 按工具 ID 执行工具（`POST /api/execute`）。
   *
   * `argsJson` 是工具参数的 JSON **字符串**；`fuelLimit` 传 0 或省略表示
   * 使用服务端默认值。
   */
  async executeTool(
    toolId: string,
    argsJson: string = '{}',
    fuelLimit: number = 0,
  ): Promise<ToolExecuteResult> {
    const body: Record<string, unknown> = {
      tool_id: toolId,
      args_json: argsJson,
    };
    if (fuelLimit > 0) body['fuel_limit'] = fuelLimit;
    return this.request<ToolExecuteResult>('POST', '/api/execute', body);
  }

  // ── SOP 管理 ───────────────────────────────

  /** 返回可用的 SOP ID 列表（服务端返回字符串数组）。 */
  async listSops(): Promise<string[]> {
    return this.request<string[]>('GET', '/api/sops');
  }

  /**
   * 获取单个 SOP 的配置（`GET /api/sops/{id}`）。
   *
   * 该端点按工作区隔离：服务端要求 `X-Sisyphus-Workspace-Id` 头，
   * 因此在构造器中配置 `workspaceId`（或改用支持传工作区的调用方式），
   * 否则服务端返回 400。
   */
  async getSop(sopId: string): Promise<SopDetail> {
    return this.request<SopDetail>(
      'GET',
      `/api/sops/${encodeURIComponent(sopId)}`,
    );
  }

  // ── 技能 ─────────────────────────────────

  async listSkills(): Promise<SkillEntry[]> {
    return this.request<SkillEntry[]>('GET', '/api/skills');
  }

  /** 安装远端技能（`POST /api/skills/install`）。成功时服务端返回 201 且无响应体。 */
  async installSkill(skillId: string): Promise<void> {
    await this.request<undefined>('POST', '/api/skills/install', {
      skill_id: skillId,
    });
  }

  // ── 认证 ─────────────────────────────────

  async validateAuth(apiKey: string): Promise<boolean> {
    const resp = await this.request<{ valid: boolean; message: string }>(
      'POST',
      '/api/auth/validate',
      { api_key: apiKey },
    );
    return resp?.valid === true;
  }

  // ── 集群 / 控制面只读观测 ─────────────────

  /** 读取集群协调的只读快照（`GET /api/cluster/status`）。 */
  async clusterStatus(): Promise<ClusterStatus> {
    return this.request<ClusterStatus>('GET', '/api/cluster/status');
  }

  /**
   * 读取指定 key 的控制面租约（`GET /api/cluster/control/leases/{key}`）。
   *
   * 该端点只读。控制面未启用 / 租约不存在时服务端仍返回 200，
   * 请以 `status_code` 判定（OK / NOT_FOUND / DISABLED / UNAVAILABLE）。
   */
  async controlLease(key: string): Promise<ControlLease> {
    return this.request<ControlLease>(
      'GET',
      `/api/cluster/control/leases/${encodeURIComponent(key)}`,
    );
  }

  // ── 认知循环可观测性 ──────────────────────────

  /**
   * 读取认知循环观测到的异常。
   *
   * `sopId` 为空时返回全部作用域内的异常；服务端对未知作用域返回空数组，
   * 不会报错。
   */
  async anomalyReports(sopId?: string): Promise<AnomalyReport[]> {
    const query = sopId ? `?sop_id=${encodeURIComponent(sopId)}` : '';
    return this.request<AnomalyReport[]>('GET', `/api/anomaly${query}`);
  }

  // ── 经验共享 ────────────────────────────────

  /** 列出所有已跨项目共享的经验。 */
  async listSharedExperiences(): Promise<SharedExperience[]> {
    const resp = await this.request<{ experiences: SharedExperience[] }>(
      'GET',
      '/api/experience/shared',
    );
    return resp.experiences ?? [];
  }

  /** 把一条本地经验标记为跨项目共享。 */
  async shareExperience(expKey: string): Promise<ExperienceShareResult> {
    return this.request<ExperienceShareResult>(
      'POST',
      '/api/experience/share',
      { exp_key: expKey },
    );
  }

  /**
   * 取消某项目的经验共享。
   *
   * `projectId` 必填（服务端缺少时返回 400），用于限定取消共享的作用域。
   */
  async unshareExperience(
    expKey: string,
    projectId: string,
  ): Promise<ExperienceShareResult> {
    return this.request<ExperienceShareResult>(
      'POST',
      '/api/experience/unshare',
      { exp_key: expKey, project_id: projectId },
    );
  }
}

export default SisyphusClient;
