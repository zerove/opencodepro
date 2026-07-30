// TypeScript SDK 单元测试
// 使用 vitest + 模拟 fetch 请求，验证每个客户端方法

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { SisyphusClient } from '../client';

// 创建一个模拟的 fetch 响应
function createMockResponse(responseBody: unknown, status = 200) {
  return {
    ok: status >= 200 && status < 300,
    status,
    statusText: status === 404 ? 'Not Found' : 'OK',
    json: () => Promise.resolve(responseBody),
    text: () =>
      Promise.resolve(
        typeof responseBody === 'string'
          ? responseBody
          : JSON.stringify(responseBody),
      ),
  };
}

describe('SisyphusClient', () => {
  let client: SisyphusClient;

  beforeEach(() => {
    // 重置所有 mock 和全局 stub
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    // 使用默认配置，不传 authToken 以简化测试
    client = new SisyphusClient({ baseUrl: 'http://test.local' });
  });

  // ── 构造器 ──────────────────────────────────────

  it('应该使用默认 baseUrl 构造', () => {
    const c = new SisyphusClient();
    expect(c).toBeInstanceOf(SisyphusClient);
  });

  it('应该接受自定义 baseUrl 并去除尾部斜杠', () => {
    const c = new SisyphusClient({ baseUrl: 'http://example.com/' });
    const mockFn = vi.fn().mockResolvedValue(
      createMockResponse({ status: 'ok', version: '1.0', uptime_secs: 100 }),
    );
    vi.stubGlobal('fetch', mockFn);
    c.health();
    expect(mockFn).toHaveBeenCalledWith(
      'http://example.com/health',
      expect.any(Object),
    );
  });

  // ── Health ─────────────────────────────────────

  it('health() 应该返回健康状态', async () => {
    const fakeHealth = { status: 'ok', version: '1.0.0', uptime_secs: 42 };
    const mockFn = vi.fn().mockResolvedValue(createMockResponse(fakeHealth));
    vi.stubGlobal('fetch', mockFn);

    const result = await client.health();
    expect(result).toEqual(fakeHealth);
    expect(mockFn).toHaveBeenCalledWith(
      'http://test.local/health',
      expect.objectContaining({ method: 'GET' }),
    );
  });

  // ── State ──────────────────────────────────────

  it('getState() 应该返回状态条目', async () => {
    const fakeState = { project_id: 'proj1', key: 'k1', value: 'v1' };
    const mockFn = vi.fn().mockResolvedValue(createMockResponse(fakeState));
    vi.stubGlobal('fetch', mockFn);

    const result = await client.getState('proj1', 'k1');
    expect(result).toEqual(fakeState);
  });

  it('getState() 在 404 时应该返回 null', async () => {
    const mockFn = vi.fn().mockResolvedValue(
      createMockResponse({ message: 'not found' }, 404),
    );
    vi.stubGlobal('fetch', mockFn);

    const result = await client.getState('proj1', 'nonexistent');
    expect(result).toBeNull();
  });

  it('setState() 应该发送 PUT 请求', async () => {
    const mockFn = vi.fn().mockResolvedValue(createMockResponse(null, 200));
    vi.stubGlobal('fetch', mockFn);

    await client.setState('proj1', 'k1', { foo: 'bar' });
    expect(mockFn).toHaveBeenCalledWith(
      expect.stringContaining('/api/v1/state/k1'),
      expect.objectContaining({ method: 'PUT' }),
    );
  });

  it('listState() 应该返回状态列表', async () => {
    const fakeList = [
      { project_id: 'proj1', key: 'k1', value: 'v1' },
    ];
    const mockFn = vi.fn().mockResolvedValue(createMockResponse(fakeList));
    vi.stubGlobal('fetch', mockFn);

    const result = await client.listState('proj1');
    expect(result).toHaveLength(1);
  });

  // ── 向量搜索 ─────────────────────────────────

  it('vectorSearch() 应该返回搜索结果', async () => {
    const fakeResults = [{ id: 'r1', score: 0.95, content: 'test' }];
    const mockFn = vi.fn().mockResolvedValue(createMockResponse(fakeResults));
    vi.stubGlobal('fetch', mockFn);

    const result = await client.vectorSearch('query', 3);
    expect(result).toEqual(fakeResults);
    // 验证请求体包含 query 和 top_k
    expect(mockFn).toHaveBeenCalledWith(
      expect.stringContaining('/api/v1/vector/search'),
      expect.objectContaining({
        method: 'POST',
        body: expect.stringContaining('"query"'),
      }),
    );
  });

  // ── 会话 ─────────────────────────────────────

  it('listSessions() 应该返回会话列表', async () => {
    const fakeSessions = [
      {
        id: 's1',
        name: 'Session 1',
        created_at: '2024-01-01T00:00:00Z',
        updated_at: '2024-01-01T00:00:00Z',
        message_count: 0,
        messages: [],
        metadata: {},
      },
    ];
    const mockFn = vi.fn().mockResolvedValue(createMockResponse(fakeSessions));
    vi.stubGlobal('fetch', mockFn);

    const result = await client.listSessions();
    expect(result).toHaveLength(1);
    expect(result[0].id).toBe('s1');
  });

  it('getSession() 应该返回单个会话', async () => {
    const fakeSession = {
      id: 's1',
      name: 'Session 1',
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
      message_count: 0,
      messages: [],
      metadata: {},
    };
    const mockFn = vi.fn().mockResolvedValue(createMockResponse(fakeSession));
    vi.stubGlobal('fetch', mockFn);

    const result = await client.getSession('s1');
    expect(result).toBeTruthy();
  });

  it('getSession() 在 404 时应该返回 null', async () => {
    const mockFn = vi.fn().mockResolvedValue(
      createMockResponse({ message: 'not found' }, 404),
    );
    vi.stubGlobal('fetch', mockFn);

    const result = await client.getSession('nonexistent');
    expect(result).toBeNull();
  });

  // ── 执行 ─────────────────────────────────────

  it('executeSop() 应该返回执行结果', async () => {
    const fakeResult = {
      sop_id: 'sop1',
      status: 'completed',
      output: { answer: 42 },
      error: undefined,
      iterations: 3,
      deviation: 0.1,
    };
    const mockFn = vi.fn().mockResolvedValue(createMockResponse(fakeResult));
    vi.stubGlobal('fetch', mockFn);

    const result = await client.executeSop('sop1', { input: 'hello' });
    expect(result.sop_id).toBe('sop1');
    expect(result.status).toBe('completed');
  });

  // ── 错误处理 ─────────────────────────────────

  it('非 404 错误应该抛出异常', async () => {
    const mockFn = vi.fn().mockResolvedValue(
      createMockResponse({ error: 'internal error' }, 500),
    );
    vi.stubGlobal('fetch', mockFn);

    await expect(client.health()).rejects.toThrow('Sisyphus API error 500');
  });

  // ── 认证 token ───────────────────────────────

  it('如果配置了 authToken，请求应该携带 Authorization header', async () => {
    const authedClient = new SisyphusClient({
      baseUrl: 'http://test.local',
      authToken: 'my-token',
    });
    const mockFn = vi.fn().mockResolvedValue(
      createMockResponse({ status: 'ok', version: '1.0', uptime_secs: 100 }),
    );
    vi.stubGlobal('fetch', mockFn);

    await authedClient.health();
    expect(mockFn).toHaveBeenCalledWith(
      expect.any(String),
      expect.objectContaining({
        headers: expect.objectContaining({
          Authorization: 'Bearer my-token',
        }),
      }),
    );
  });
});
