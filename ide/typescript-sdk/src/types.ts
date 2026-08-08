// Sisyphus TypeScript SDK — 类型定义

/** 健康检查响应 */
export interface HealthResponse {
  status: string;
  version: string;
  uptime_secs: number;
}

/** 状态条目 */
export interface StateEntry {
  project_id: string;
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

/** 会话消息 */
export interface Message {
  role: string;
  content: string;
  created_at: string;
  estimated_tokens: number;
  is_compressed: boolean;
}

/** Session */
export interface Session {
  id: string;
  name: string;
  created_at: string;
  updated_at: string;
  message_count: number;
  messages: Message[];
  metadata: Record<string, string>;
  parent_session_id?: string;
  child_session_ids?: string[];
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

/** 执行结果 */
export interface ExecutionResult {
  sop_id: string;
  status: string;
  output?: unknown;
  error?: string;
  iterations: number;
  deviation: number;
}

/** SDK 配置 */
export interface SisyphusConfig {
  /** Sisyphus REST API 基础 URL (默认: http://localhost:8080) */
  baseUrl?: string;
  /** 请求超时（毫秒）(默认: 30000) */
  timeoutMs?: number;
  /** 认证用 Bearer token */
  authToken?: string;
}

export type { SisyphusConfig as Config };
