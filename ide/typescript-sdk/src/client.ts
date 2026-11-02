// Sisyphus TypeScript SDK — REST API 客户端

import type {
  SisyphusConfig,
  HealthResponse,
  StateEntry,
  SearchResult,
  Session,
  ExecutionResult,
  AnomalyReport,
  SharedExperience,
  ExperienceShareResult,
} from './types';

const DEFAULT_BASE_URL = 'http://localhost:8080';
const DEFAULT_TIMEOUT = 30_000;

export class SisyphusClient {
  private baseUrl: string;
  private timeout: number;
  private authToken?: string;

  constructor(config: SisyphusConfig = {}) {
    this.baseUrl = (config.baseUrl || DEFAULT_BASE_URL).replace(/\/$/, '');
    this.timeout = config.timeoutMs || DEFAULT_TIMEOUT;
    this.authToken = config.authToken;
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

    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeout);

    try {
      const resp = await fetch(`${this.baseUrl}${path}`, {
        method,
        headers,
        body: body ? JSON.stringify(body) : undefined,
        signal: controller.signal,
      });

      if (!resp.ok) {
        const text = await resp.text().catch(() => '');
        throw new Error(
          `Sisyphus API error ${resp.status}: ${text || resp.statusText}`,
        );
      }

      return (await resp.json()) as T;
    } finally {
      clearTimeout(timer);
    }
  }

  // ── 健康检查 ──────────────────────────────────────

  async health(): Promise<HealthResponse> {
    return this.request<HealthResponse>('GET', '/health');
  }

  // ── 状态 CRUD ─────────────────────────────────

  async getState(
    projectId: string,
    key: string,
  ): Promise<StateEntry | null> {
    try {
      return await this.request<StateEntry>(
        'GET',
        `/api/state/${encodeURIComponent(key)}?project_id=${encodeURIComponent(projectId)}`,
      );
    } catch (e: any) {
      if (e.message?.includes('404')) return null;
      throw e;
    }
  }

  async setState(
    projectId: string,
    key: string,
    value: unknown,
  ): Promise<void> {
    await this.request('PUT', `/api/state/${encodeURIComponent(key)}`, {
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

  async listSessions(): Promise<Session[]> {
    return this.request<Session[]>('GET', '/api/sessions');
  }

  async getSession(id: string): Promise<Session | null> {
    try {
      return await this.request<Session>(
        'GET',
        `/api/sessions/${encodeURIComponent(id)}`,
      );
    } catch (e: any) {
      if (e.message?.includes('404')) return null;
      throw e;
    }
  }

  // ── 执行 ───────────────────────────────────

  async executeSop(
    sopId: string,
    context?: Record<string, unknown>,
  ): Promise<ExecutionResult> {
    return this.request<ExecutionResult>('POST', '/api/execute', {
      sop_id: sopId,
      context: context || {},
    });
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
