// TypeScript SDK 单元测试
// 使用 vitest + 模拟 fetch 请求，验证每个客户端方法

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { SisyphusClient, SisyphusApiError } from '../client';

// 创建一个模拟的 fetch 响应。
// responseBody === undefined 表示 201/204 之类的空响应体。
function createMockResponse(responseBody: unknown, status = 200) {
  return {
    ok: status >= 200 && status < 300,
    status,
    statusText: status === 404 ? 'Not Found' : 'OK',
    json: () => Promise.resolve(responseBody),
    text: () =>
      Promise.resolve(
        responseBody === undefined
          ? ''
          : typeof responseBody === 'string'
            ? responseBody
            : JSON.stringify(responseBody),
      ),
  };
}

/** 取出第 n 次 fetch 调用的 JSON 请求体 */
function requestBody(mockFn: ReturnType<typeof vi.fn>, n = 0): unknown {
  const init = mockFn.mock.calls[n]?.[1] as { body?: string };
  return init.body === undefined ? undefined : JSON.parse(init.body);
}

/** 取出第 n 次 fetch 调用的 URL */
function requestUrl(mockFn: ReturnType<typeof vi.fn>, n = 0): string {
  return mockFn.mock.calls[n]?.[0] as string;
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
    // 服务端 StateEntry 只有 key / value
    const fakeState = { key: 'k1', value: 'v1' };
    const mockFn = vi.fn().mockResolvedValue(createMockResponse(fakeState));
    vi.stubGlobal('fetch', mockFn);

    const result = await client.getState('proj1', 'k1');
    expect(result).toEqual(fakeState);
    expect(requestUrl(mockFn)).toContain('project_id=proj1');
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
    const mockFn = vi.fn().mockResolvedValue(createMockResponse(null, 204));
    vi.stubGlobal('fetch', mockFn);

    await client.setState('proj1', 'k1', { foo: 'bar' });
    expect(mockFn).toHaveBeenCalledWith(
      expect.stringContaining('/api/state/k1'),
      expect.objectContaining({ method: 'PUT' }),
    );
    expect(requestBody(mockFn)).toEqual({
      project_id: 'proj1',
      value: { foo: 'bar' },
    });
  });

  it('setState() 应该容忍空响应体（204）', async () => {
    const mockFn = vi
      .fn()
      .mockResolvedValue(createMockResponse(undefined, 204));
    vi.stubGlobal('fetch', mockFn);

    await expect(client.setState('proj1', 'k1', null)).resolves.toBeUndefined();
  });

  it('listState() 应该返回状态列表', async () => {
    const fakeList = [{ key: 'k1', value: 'v1' }];
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
    expect(requestUrl(mockFn)).toContain('/api/vector/search');
    expect(requestBody(mockFn)).toEqual({ query: 'query', top_k: 3 });
  });

  // ── 会话 ─────────────────────────────────────

  it('listSessions() 应该返回会话摘要列表', async () => {
    const fakeSessions = [
      {
        id: 's1',
        name: 'Session 1',
        message_count: 2,
        created_at: '2024-01-01T00:00:00Z',
        updated_at: '2024-01-01T00:00:01Z',
      },
    ];
    const mockFn = vi.fn().mockResolvedValue(createMockResponse(fakeSessions));
    vi.stubGlobal('fetch', mockFn);

    const result = await client.listSessions();
    expect(result).toHaveLength(1);
    expect(result[0]?.id).toBe('s1');
    expect(result[0]?.message_count).toBe(2);
  });

  it('getSession() 应该返回会话详情（含 messages）', async () => {
    const fakeSession = {
      id: 's1',
      name: 'Session 1',
      message_count: 1,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:01Z',
      messages: [
        {
          role: 'user',
          content: 'hi',
          created_at: '2024-01-01T00:00:01Z',
          estimated_tokens: 1,
        },
      ],
    };
    const mockFn = vi.fn().mockResolvedValue(createMockResponse(fakeSession));
    vi.stubGlobal('fetch', mockFn);

    const result = await client.getSession('s1');
    expect(result?.messages[0]?.content).toBe('hi');
  });

  it('getSession() 在 404 时应该返回 null', async () => {
    const mockFn = vi.fn().mockResolvedValue(
      createMockResponse({ message: 'not found' }, 404),
    );
    vi.stubGlobal('fetch', mockFn);

    const result = await client.getSession('nonexistent');
    expect(result).toBeNull();
  });

  it('createSession() 应该 POST name 并返回会话详情', async () => {
    const created = {
      id: 's3',
      name: 'New Session',
      message_count: 0,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
      messages: [],
    };
    const mockFn = vi.fn().mockResolvedValue(createMockResponse(created, 201));
    vi.stubGlobal('fetch', mockFn);

    const result = await client.createSession('New Session');
    expect(result.id).toBe('s3');
    expect(requestUrl(mockFn)).toMatch(/\/api\/sessions$/);
    expect(requestBody(mockFn)).toEqual({ name: 'New Session' });
  });

  it('addMessage() 应该返回更新后的会话详情', async () => {
    const updated = {
      id: 's1',
      name: 'Session 1',
      message_count: 2,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:02Z',
      messages: [],
    };
    const mockFn = vi.fn().mockResolvedValue(createMockResponse(updated));
    vi.stubGlobal('fetch', mockFn);

    const result = await client.addMessage('s1', 'user', 'hello');
    expect(result.message_count).toBe(2);
    expect(requestUrl(mockFn)).toContain('/api/sessions/s1/messages');
    expect(requestBody(mockFn)).toEqual({ role: 'user', content: 'hello' });
  });

  // ── 执行 ─────────────────────────────────────

  it('executeSop() 必须走 /api/execute/goal', async () => {
    const fakeResult = { output: { answer: 42 }, report: 'ok' };
    const mockFn = vi.fn().mockResolvedValue(createMockResponse(fakeResult));
    vi.stubGlobal('fetch', mockFn);

    const result = await client.executeSop('sop1', 'Analyze code', 'proj1');
    expect(result.report).toBe('ok');
    expect(result.output).toEqual({ answer: 42 });
    expect(requestUrl(mockFn)).toMatch(/\/api\/execute\/goal$/);
    expect(requestBody(mockFn)).toEqual({
      sop_id: 'sop1',
      goal: 'Analyze code',
      project_id: 'proj1',
    });
  });

  it('executeSop() 未指定 projectId 时不应发送该字段', async () => {
    const mockFn = vi
      .fn()
      .mockResolvedValue(createMockResponse({ output: null, report: '' }));
    vi.stubGlobal('fetch', mockFn);

    await client.executeSop('sop1', 'goal');
    expect(requestBody(mockFn)).toEqual({ sop_id: 'sop1', goal: 'goal' });
  });

  it('executeTool() 必须走 /api/execute 并保持 args_json 为字符串', async () => {
    const mockFn = vi.fn().mockResolvedValue(
      createMockResponse({
        status: 'SUCCESS',
        result_json: '{"ok":true}',
      }),
    );
    vi.stubGlobal('fetch', mockFn);

    const args = '{"path":"/tmp/a.txt"}';
    const result = await client.executeTool('file_read', args, 1000);
    expect(result.status).toBe('SUCCESS');
    expect(result.result_json).toBe('{"ok":true}');
    expect(requestUrl(mockFn)).toMatch(/\/api\/execute$/);
    expect(requestBody(mockFn)).toEqual({
      tool_id: 'file_read',
      args_json: args,
      fuel_limit: 1000,
    });
  });

  // ── SOP 管理 ────────────────────────────────

  it('listSops() 应该返回 SOP ID 字符串数组', async () => {
    const mockFn = vi
      .fn()
      .mockResolvedValue(createMockResponse(['sop1', 'sop2']));
    vi.stubGlobal('fetch', mockFn);

    const result = await client.listSops();
    expect(result).toEqual(['sop1', 'sop2']);
  });

  it('getSop() 应该返回 SOP 详情', async () => {
    const fakeSop = {
      id: 'sop1',
      name: 'SOP 1',
      version: '1.0.0',
      config: { nodes: [] },
    };
    const mockFn = vi.fn().mockResolvedValue(createMockResponse(fakeSop));
    vi.stubGlobal('fetch', mockFn);

    const result = await client.getSop('sop1');
    expect(result.id).toBe('sop1');
    expect(result.config).toEqual({ nodes: [] });
    expect(requestUrl(mockFn)).toMatch(/\/api\/sops\/sop1$/);
  });

  // ── 技能 ────────────────────────────────────

  it('listSkills() 应该按服务端字段解码', async () => {
    const fakeSkills = [
      {
        name: 'Skill 1',
        description: 'Test',
        version: '1.0',
        author: 'author',
        tags: ['demo'],
        source: 'builtin',
      },
    ];
    const mockFn = vi.fn().mockResolvedValue(createMockResponse(fakeSkills));
    vi.stubGlobal('fetch', mockFn);

    const result = await client.listSkills();
    expect(result[0]?.name).toBe('Skill 1');
    expect(result[0]?.tags).toEqual(['demo']);
    expect(result[0]?.source).toBe('builtin');
  });

  it('installSkill() 应该容忍 201 空响应体', async () => {
    const mockFn = vi
      .fn()
      .mockResolvedValue(createMockResponse(undefined, 201));
    vi.stubGlobal('fetch', mockFn);

    await expect(client.installSkill('sk1')).resolves.toBeUndefined();
    expect(requestUrl(mockFn)).toMatch(/\/api\/skills\/install$/);
    expect(requestBody(mockFn)).toEqual({ skill_id: 'sk1' });
  });

  // ── 认证 ────────────────────────────────────

  it('validateAuth() 应该返回服务端的 valid 标志', async () => {
    const mockFn = vi
      .fn()
      .mockResolvedValue(createMockResponse({ valid: true, message: 'ok' }));
    vi.stubGlobal('fetch', mockFn);

    expect(await client.validateAuth('key')).toBe(true);
    expect(requestBody(mockFn)).toEqual({ api_key: 'key' });
  });

  // ── 集群 / 控制面只读观测 ─────────────────────

  it('clusterStatus() 应该解析只读快照', async () => {
    const fakeStatus = {
      enabled: true,
      cluster_name: 'demo',
      config: {
        node_id: 'node-1',
        peer_ttl_secs: 30,
        join_timeout_secs: 5,
        strategy_sync_interval_secs: 10,
        static_peers: ['node-2:50051'],
        control_plane_enabled: true,
        control_plane: {
          enabled: true,
          propose_timeout_ms: 2000,
          connect_timeout_ms: 1000,
          lease_ttl_min_secs: 5,
          lease_ttl_max_secs: 300,
        },
      },
      control_plane: {
        configured: true,
        coordinator_configured: true,
        security: {
          tls_enabled_config: true,
          tls_wired: true,
          submit_auth_mode_config: 'mtls_subject_allowlist',
          submit_auth_enforced: true,
          app_layer_submit_auth_enforced: true,
          effective_armed: true,
          allowed_submit_subjects_count: 3,
          note: 'ADR-019',
        },
        stats: { accepts: 2 },
      },
      peer_manager: {
        self_node_id: 'node-1',
        leader_id: 'node-1',
        is_leader: true,
        cluster_size: 2,
        state_proposal_broadcasts: 7,
        experience_sync: {
          broadcasts: 1,
          inbound_delivered: 2,
          inbound_applied: 2,
          inbound_rejected: 0,
          inbound_rejected_deserialize: 0,
          inbound_rejected_quality: 0,
          inbound_rejected_apply: 0,
        },
      },
      consensus: { leader_id: 'node-1', is_leader: true },
      peers: [
        {
          node_id: 'node-1',
          grpc_addr: 'http://127.0.0.1:50051',
          joined_at: 0,
          last_seen_secs_ago: 0,
        },
        {
          node_id: 'node-2',
          grpc_addr: 'http://127.0.0.1:50052',
          joined_at: 0,
          last_seen_secs_ago: null,
        },
      ],
    };
    const mockFn = vi.fn().mockResolvedValue(createMockResponse(fakeStatus));
    vi.stubGlobal('fetch', mockFn);

    const status = await client.clusterStatus();
    expect(status.enabled).toBe(true);
    expect(status.config.control_plane.lease_ttl_max_secs).toBe(300);
    expect(status.control_plane.security.allowed_submit_subjects_count).toBe(3);
    expect(status.peer_manager.experience_sync.inbound_applied).toBe(2);
    expect(status.peers[1]?.last_seen_secs_ago).toBeNull();
    expect(requestUrl(mockFn)).toMatch(/\/api\/cluster\/status$/);
  });

  it('controlLease() 在控制面关闭时返回 DISABLED 而不是抛错', async () => {
    const mockFn = vi.fn().mockResolvedValue(
      createMockResponse({
        found: false,
        key: 'lease:demo',
        owner: '',
        generation: 0,
        expires_at_unix_ms: 0,
        last_request_id: '',
        status_code: 'DISABLED',
        status_message: 'cluster.control_plane.enabled is false (ADR-019 gate)',
      }),
    );
    vi.stubGlobal('fetch', mockFn);

    const lease = await client.controlLease('lease:demo');
    expect(lease.found).toBe(false);
    expect(lease.status_code).toBe('DISABLED');
  });

  it('controlLease() 应该对 key 做 URL 编码', async () => {
    const mockFn = vi.fn().mockResolvedValue(
      createMockResponse({
        found: true,
        key: 'lease/demo',
        owner: 'node-1',
        generation: 4,
        expires_at_unix_ms: 1_700_000_000_000,
        last_request_id: 'req-1',
        status_code: 'OK',
        status_message: '',
      }),
    );
    vi.stubGlobal('fetch', mockFn);

    const lease = await client.controlLease('lease/demo');
    expect(lease.status_code).toBe('OK');
    expect(lease.generation).toBe(4);
    expect(requestUrl(mockFn)).toContain(
      '/api/cluster/control/leases/lease%2Fdemo',
    );
  });

  // ── 认知循环可观测性 ─────────────────────────

  it('anomalyReports() 应该带 sop_id 查询参数并返回异常列表', async () => {
    const fakeReports = [
      {
        node_name: 'node_a',
        severity: 'high',
        metric: 'duration',
        observed: 42,
        expected: 10,
        description: 'duration deviation',
        timestamp: 1_700_000_000,
      },
    ];
    const mockFn = vi.fn().mockResolvedValue(createMockResponse(fakeReports));
    vi.stubGlobal('fetch', mockFn);

    const result = await client.anomalyReports('sop-live');
    expect(result).toEqual(fakeReports);
    expect(mockFn).toHaveBeenCalledWith(
      expect.stringContaining('/api/anomaly?sop_id=sop-live'),
      expect.objectContaining({ method: 'GET' }),
    );
  });

  it('anomalyReports() 不带 sopId 时不应该拼查询参数', async () => {
    const mockFn = vi.fn().mockResolvedValue(createMockResponse([]));
    vi.stubGlobal('fetch', mockFn);

    const result = await client.anomalyReports();
    expect(result).toEqual([]);
    expect(requestUrl(mockFn)).toMatch(/\/api\/anomaly$/);
  });

  // ── 经验共享 ────────────────────────────────

  it('listSharedExperiences() 应该解包 experiences 字段', async () => {
    const fakeExperience = {
      context_text: 'ctx',
      context_hash: 'hash',
      action_id: 'act',
      outcome_success: true,
      deviation_score: 0.0,
      cost: 12,
      timestamp: 1_700_000_000,
      version: 1,
      vector_clock: { node_a: 1 },
    };
    const mockFn = vi
      .fn()
      .mockResolvedValue(createMockResponse({ experiences: [fakeExperience] }));
    vi.stubGlobal('fetch', mockFn);

    const result = await client.listSharedExperiences();
    expect(result).toHaveLength(1);
    expect(result[0]?.action_id).toBe('act');
    expect(mockFn).toHaveBeenCalledWith(
      expect.stringContaining('/api/experience/shared'),
      expect.objectContaining({ method: 'GET' }),
    );
  });

  it('shareExperience() 应该 POST exp_key', async () => {
    const mockFn = vi
      .fn()
      .mockResolvedValue(createMockResponse({ success: true, error: '' }));
    vi.stubGlobal('fetch', mockFn);

    const result = await client.shareExperience('exp:1');
    expect(result.success).toBe(true);
    expect(mockFn).toHaveBeenCalledWith(
      expect.stringContaining('/api/experience/share'),
      expect.objectContaining({
        method: 'POST',
        body: expect.stringContaining('"exp:1"'),
      }),
    );
  });

  it('unshareExperience() 应该同时发送 exp_key 与 project_id', async () => {
    const mockFn = vi
      .fn()
      .mockResolvedValue(createMockResponse({ success: true, error: '' }));
    vi.stubGlobal('fetch', mockFn);

    await client.unshareExperience('exp:1', 'proj1');
    expect(requestBody(mockFn)).toEqual({
      exp_key: 'exp:1',
      project_id: 'proj1',
    });
  });

  // ── 错误处理 ─────────────────────────────────

  it('非 404 错误应该抛出 SisyphusApiError', async () => {
    const mockFn = vi.fn().mockResolvedValue(
      createMockResponse({ error: 'internal error' }, 500),
    );
    vi.stubGlobal('fetch', mockFn);

    await expect(client.health()).rejects.toThrow('Sisyphus API error 500');
    await expect(client.health()).rejects.toBeInstanceOf(SisyphusApiError);
  });

  // ── 认证 token / workspace header ────────────

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

  it('如果配置了 workspaceId，请求应该携带 X-Sisyphus-Workspace-Id header', async () => {
    const workspaceClient = new SisyphusClient({
      baseUrl: 'http://test.local',
      workspaceId: 'proj1',
    });
    const mockFn = vi.fn().mockResolvedValue(
      createMockResponse({ id: 'sop1', name: 'SOP', config: {} }),
    );
    vi.stubGlobal('fetch', mockFn);

    await workspaceClient.getSop('sop1');
    expect(mockFn).toHaveBeenCalledWith(
      expect.any(String),
      expect.objectContaining({
        headers: expect.objectContaining({
          'X-Sisyphus-Workspace-Id': 'proj1',
        }),
      }),
    );
  });
});
