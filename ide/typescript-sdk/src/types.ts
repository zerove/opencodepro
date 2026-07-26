// Sisyphus TypeScript SDK — Type Definitions

/** Health check response */
export interface HealthResponse {
  status: string;
  version: string;
  uptime_secs: number;
}

/** State entry */
export interface StateEntry {
  project_id: string;
  key: string;
  value: unknown;
}

/** Vector record for semantic search */
export interface VectorRecord {
  id: string;
  content: string;
  embedding: number[];
  metadata: Record<string, string>;
}

/** Vector search result */
export interface SearchResult {
  id: string;
  score: number;
  content: string;
}

/** Session message */
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

/** SOP node */
export interface SOPNode {
  id: string;
  kind: string;
  config: unknown;
}

/** SOP graph */
export interface SOPGraph {
  nodes: SOPNode[];
  edges: Array<[string, string]>;
}

/** Execution result */
export interface ExecutionResult {
  sop_id: string;
  status: string;
  output?: unknown;
  error?: string;
  iterations: number;
  deviation: number;
}

/** SDK configuration */
export interface SisyphusConfig {
  /** Base URL of the Sisyphus REST API (default: http://localhost:8080) */
  baseUrl?: string;
  /** Request timeout in milliseconds (default: 30000) */
  timeoutMs?: number;
  /** Bearer token for authentication */
  authToken?: string;
}

export type { SisyphusConfig as Config };
