// Sisyphus TypeScript SDK — REST API 客户端

import type {
  SisyphusConfig,
  HealthResponse,
  StateEntry,
  SearchResult,
  Session,
  ExecutionResult,
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
        `/api/v1/state/${encodeURIComponent(key)}?project_id=${encodeURIComponent(projectId)}`,
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
    await this.request('PUT', `/api/v1/state/${encodeURIComponent(key)}`, {
      project_id: projectId,
      value,
    });
  }

  async listState(projectId: string): Promise<StateEntry[]> {
    return this.request<StateEntry[]>(
      'GET',
      `/api/v1/state?project_id=${encodeURIComponent(projectId)}`,
    );
  }

  // ── 向量搜索 ───────────────────────────────

  async vectorSearch(
    query: string,
    topK: number = 5,
  ): Promise<SearchResult[]> {
    return this.request<SearchResult[]>('POST', '/api/v1/vector/search', {
      query,
      top_k: topK,
    });
  }

  // ── 会话 ────────────────────────────────────

  async listSessions(): Promise<Session[]> {
    return this.request<Session[]>('GET', '/api/v1/sessions');
  }

  async getSession(id: string): Promise<Session | null> {
    try {
      return await this.request<Session>(
        'GET',
        `/api/v1/sessions/${encodeURIComponent(id)}`,
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
    return this.request<ExecutionResult>('POST', '/api/v1/execute', {
      sop_id: sopId,
      context: context || {},
    });
  }
}

export default SisyphusClient;
