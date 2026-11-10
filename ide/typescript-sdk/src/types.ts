// Sisyphus TypeScript SDK — 类型定义
//
// 所有类型与 `crates/sisyphus-bridge/src/http/types.rs` 的服务端结构保持
// 一一对应。新增服务端字段不会破坏解码（TS 侧为结构化子集），但字段名
// 不一致会导致运行期拿到 `undefined` —— 修改前请对照服务端。

/** 健康检查响应 */
export interface HealthResponse {
  status: string;
  version: string;
  uptime_secs: number;
}

/** 状态条目（服务端只返回 `key` / `value`） */
export interface StateEntry {
  key: string;
  value: unknown;
}

/** 语义搜索向量记录 */
export interface VectorRecord {
  id: string;
  content: string;
  embedding: number[];
  metadata: Record<string, string>;
}

/** 向量搜索结果 */
export interface SearchResult {
  id: string;
  score: number;
  content: string;
}

/** 会话中的一条消息（服务端 `MessageDto`） */
export interface MessageDto {
  role: string;
  content: string;
  created_at: string;
  estimated_tokens: number;
}

/** 会话列表条目（`GET /api/sessions`） */
export interface SessionSummary {
  id: string;
  name: string;
  message_count: number;
  created_at: string;
  updated_at: string;
}

/**
 * 兼容别名：旧版本把列表条目命名为 `Session`。
 * @deprecated 请使用 {@link SessionSummary}
 */
export type Session = SessionSummary;

/**
 * 会话详情（`GET /api/sessions/{id}`、`POST /api/sessions`、
 * `POST /api/sessions/{id}/messages` 三者返回同一结构）。
 */
export interface SessionDetail {
  id: string;
  name: string;
  message_count: number;
  created_at: string;
  updated_at: string;
  messages: MessageDto[];
}

/** SOP 节点 */
export interface SOPNode {
  id: string;
  kind: string;
  config: unknown;
}

/** SOP 图 */
export interface SOPGraph {
  nodes: SOPNode[];
  edges: Array<[string, string]>;
}

/** SOP 详情（`GET /api/sops/{id}`） */
export interface SopDetail {
  id: string;
  name: string;
  version?: string | null;
  config: unknown;
}

/**
 * SOP 目标执行结果（`POST /api/execute/goal`）。
 *
 * `output` 是服务端 `NodeOutput`（任意 JSON），`report` 是执行报告文本。
 */
export interface GoalExecuteResult {
  output: unknown;
  report: string;
}

/**
 * 工具执行结果（`POST /api/execute`）。
 *
 * 注意：`/api/execute` 是**工具**执行端点（`tool_id`/`args_json`），
 * 不是 SOP 执行 —— SOP 执行请用 {@link SisyphusClient.executeSop}。
 */
export interface ToolExecuteResult {
  status: string;
  /** 工具输出的 JSON 字符串（需要再解析一次） */
  result_json: string;
}

/** 技能条目（`GET /api/skills`） */
export interface SkillEntry {
  name: string;
  description: string;
  version: string;
  author: string;
  tags: string[];
  source: string;
}

/** 认知循环观测到的一条异常（GET /api/anomaly） */
export interface AnomalyReport {
  node_name: string;
  /** "low" | "medium" | "high" | "critical" */
  severity: string;
  /** 触发异常的外部指标名，例如 "fuel" / "duration" / "error_rate" */
  metric: string;
  observed: number;
  expected: number;
  description: string;
  /** 检测到异常的 Unix 时间戳（秒） */
  timestamp: number;
}

/** 跨项目共享的经验条目（GET /api/experience/shared） */
export interface SharedExperience {
  context_text: string;
  context_hash: string;
  action_id: string;
  project_id?: string;
  outcome_success: boolean;
  deviation_score: number;
  cost: number;
  timestamp: number;
  embedding?: number[];
  visual_embedding?: number[];
  insight?: string;
  /** CRDT 本地版本号 */
  version: number;
  /** CRDT vector clock：node_id → 最近版本 */
  vector_clock: Record<string, number>;
}

/** 经验分享 / 取消分享结果 */
export interface ExperienceShareResult {
  success: boolean;
  /** success=false 时的错误描述，success=true 时为空串 */
  error: string;
}

// ── 集群 / 控制面只读观测 ─────────────────────────────
//
// ADR-020：这些端点**只读**，不提供 REST 写入路径。控制面未启用时
// 依然返回 200，请以 `status_code` 判定。

/** 集群 peer 条目 */
export interface ClusterPeer {
  node_id: string;
  grpc_addr: string;
  joined_at: number;
  /** 未知（刚加入/无心跳）时为 null */
  last_seen_secs_ago: number | null;
}

/** 经验同步的累计计数 */
export interface ExperienceSyncStats {
  broadcasts: number;
  inbound_delivered: number;
  inbound_applied: number;
  inbound_rejected: number;
  inbound_rejected_deserialize: number;
  inbound_rejected_quality: number;
  inbound_rejected_apply: number;
}

/** peer manager 的只读视图 */
export interface PeerManagerView {
  self_node_id: string | null;
  leader_id: string | null;
  is_leader: boolean;
  cluster_size: number;
  state_proposal_broadcasts: number;
  experience_sync: ExperienceSyncStats;
}

/** consensus trait 视图 */
export interface ConsensusView {
  leader_id: string | null;
  is_leader: boolean;
}

/** `[cluster.control_plane]` 的生效配置快照 */
export interface ControlPlaneConfigView {
  enabled: boolean;
  propose_timeout_ms: number;
  connect_timeout_ms: number;
  lease_ttl_min_secs: number;
  lease_ttl_max_secs: number;
}

/** `[cluster]` 的生效配置快照 */
export interface ClusterConfigView {
  node_id: string;
  peer_ttl_secs: number;
  join_timeout_secs: number;
  strategy_sync_interval_secs: number;
  static_peers: string[];
  control_plane_enabled: boolean;
  control_plane: ControlPlaneConfigView;
}

/** 控制面安全装配的运行时视图（ADR-019） */
export interface ControlPlaneSecurityView {
  tls_enabled_config: boolean;
  tls_wired: boolean;
  submit_auth_mode_config: string;
  submit_auth_enforced: boolean;
  app_layer_submit_auth_enforced: boolean;
  /** 是否真正 arm（未 arm 时拒绝所有提交） */
  effective_armed: boolean;
  allowed_submit_subjects_count: number;
  note: string;
}

/** 控制面运行时状态（只读） */
export interface ControlPlaneStatus {
  configured: boolean;
  coordinator_configured: boolean;
  security: ControlPlaneSecurityView;
  /** 控制面统计快照；未 arm 时为 null */
  stats: unknown | null;
}

/** `GET /api/cluster/status` 的响应 */
export interface ClusterStatus {
  enabled: boolean;
  cluster_name: string;
  config: ClusterConfigView;
  control_plane: ControlPlaneStatus;
  peer_manager: PeerManagerView;
  consensus: ConsensusView;
  peers: ClusterPeer[];
}

/** 租约查询状态码 */
export type LeaseStatusCode = 'OK' | 'NOT_FOUND' | 'DISABLED' | 'UNAVAILABLE';

/**
 * `GET /api/cluster/control/leases/{key}` 的响应。
 *
 * 该端点只读；控制面未启用时 `found=false` 且 `status_code="DISABLED"`，
 * HTTP 状态仍为 200。
 */
export interface ControlLease {
  found: boolean;
  key: string;
  owner: string;
  generation: number;
  expires_at_unix_ms: number;
  last_request_id: string;
  status_code: LeaseStatusCode;
  status_message: string;
}

/** SDK 配置 */
export interface SisyphusConfig {
  /** Sisyphus REST API 基础 URL (默认: http://localhost:8080) */
  baseUrl?: string;
  /** 请求超时（毫秒）(默认: 30000) */
  timeoutMs?: number;
  /** 认证用 Bearer token */
  authToken?: string;
  /**
   * 工作区标识，会作为 `X-Sisyphus-Workspace-Id` 请求头发送。
   *
   * `GET /api/sops/{id}` 等服务端按工作区隔离的端点**要求**该头，
   * 缺失时服务端返回 400。
   */
  workspaceId?: string;
}

export type { SisyphusConfig as Config };
