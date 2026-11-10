// Package sisyphus 提供 Sisyphus 认知引擎 REST API 的 Go 客户端。
package sisyphus

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"time"
)

// Client 是 Sisyphus REST API 客户端。
type Client struct {
	BaseURL   string
	AuthToken string
	// WorkspaceID 作为 X-Sisyphus-Workspace-Id 头发送。
	//
	// GET /api/sops/{id} 等服务端按工作区隔离的端点要求该头，缺失时 400。
	WorkspaceID string
	HTTP        *http.Client
}

// NewClient 创建新的 Sisyphus 客户端。
func NewClient(baseURL, authToken string) *Client {
	return &Client{
		BaseURL:   baseURL,
		AuthToken: authToken,
		HTTP:      &http.Client{Timeout: 30 * time.Second},
	}
}

// HealthResponse 来自 /health 端点。
type HealthResponse struct {
	Status     string `json:"status"`
	Version    string `json:"version"`
	UptimeSecs uint64 `json:"uptime_secs"`
}

// Session 表示一个对话会话（GET /api/sessions 的条目）。
type Session struct {
	ID              string  `json:"id"`
	Name            string  `json:"name"`
	MessageCount    uint64  `json:"message_count"`
	CreatedAt       *string `json:"created_at,omitempty"`
	UpdatedAt       *string `json:"updated_at,omitempty"`
	ParentSessionID *string `json:"parent_session_id,omitempty"`
}

// MessageDto 是会话中的一条消息。
type MessageDto struct {
	Role            string `json:"role"`
	Content         string `json:"content"`
	CreatedAt       string `json:"created_at"`
	EstimatedTokens uint64 `json:"estimated_tokens"`
}

// SessionDetail 来自 GET /api/sessions/{id}、POST /api/sessions
// 与 POST /api/sessions/{id}/messages（三者返回同一结构）。
type SessionDetail struct {
	ID           string       `json:"id"`
	Name         string       `json:"name"`
	MessageCount uint64       `json:"message_count"`
	CreatedAt    string       `json:"created_at"`
	UpdatedAt    string       `json:"updated_at"`
	Messages     []MessageDto `json:"messages"`
}

// SearchResult 来自向量搜索。
type SearchResult struct {
	ID      string  `json:"id"`
	Score   float64 `json:"score"`
	Content string  `json:"content"`
}

// GoalExecuteResult 来自 POST /api/execute/goal（SOP 目标执行）。
//
// Output 是任意 JSON（服务端 `NodeOutput`），因此用 json.RawMessage 原样保留。
type GoalExecuteResult struct {
	Output json.RawMessage `json:"output"`
	Report string          `json:"report"`
}

// ToolExecuteResult 来自 POST /api/execute（按工具 ID 执行）。
type ToolExecuteResult struct {
	Status     string `json:"status"`
	ResultJSON string `json:"result_json"`
}

// SopDetail 来自 GET /api/sops/{id}。
type SopDetail struct {
	ID      string          `json:"id"`
	Name    string          `json:"name"`
	Version *string         `json:"version,omitempty"`
	Config  json.RawMessage `json:"config"`
}

// SkillEntry 是 GET /api/skills 的条目。
//
// 注意：服务端返回的字段是 name/description/version/author/tags/source，
// 没有 id/downloads/rating —— 旧版本 SDK 的字段名与服务端不一致。
type SkillEntry struct {
	Name        string   `json:"name"`
	Description string   `json:"description"`
	Version     string   `json:"version"`
	Author      string   `json:"author"`
	Tags        []string `json:"tags"`
	Source      string   `json:"source"`
}

// AnomalyReport 是认知循环观测到的一条异常（GET /api/anomaly）。
type AnomalyReport struct {
	NodeName    string  `json:"node_name"`
	Severity    string  `json:"severity"`
	Metric      string  `json:"metric"`
	Observed    float64 `json:"observed"`
	Expected    float64 `json:"expected"`
	Description string  `json:"description"`
	// Timestamp 是检测到异常的 Unix 时间戳（秒）。
	Timestamp uint64 `json:"timestamp"`
}

// SharedExperience 是跨项目共享的经验条目（GET /api/experience/shared）。
type SharedExperience struct {
	ContextText     string    `json:"context_text"`
	ContextHash     string    `json:"context_hash"`
	ActionID        string    `json:"action_id"`
	ProjectID       *string   `json:"project_id,omitempty"`
	OutcomeSuccess  bool      `json:"outcome_success"`
	DeviationScore  float64   `json:"deviation_score"`
	Cost            uint64    `json:"cost"`
	Timestamp       uint64    `json:"timestamp"`
	Embedding       []float32 `json:"embedding,omitempty"`
	VisualEmbedding []float32 `json:"visual_embedding,omitempty"`
	Insight         *string   `json:"insight,omitempty"`
	// Version 是 CRDT 本地版本号。
	Version uint64 `json:"version"`
	// VectorClock 是 CRDT vector clock：node_id → 最近版本。
	VectorClock map[string]uint64 `json:"vector_clock"`
}

// sharedExperiencesResponse 是 /api/experience/shared 的响应包装。
type sharedExperiencesResponse struct {
	Experiences []SharedExperience `json:"experiences"`
}

// ExperienceShareResult 是经验分享 / 取消分享结果。
type ExperienceShareResult struct {
	Success bool   `json:"success"`
	Error   string `json:"error"`
}

// ── 集群 / 控制面只读观测（GET /api/cluster/status） ────────────

// ClusterPeer 是集群 peer 条目。
type ClusterPeer struct {
	NodeID          string  `json:"node_id"`
	GRPCAddr        string  `json:"grpc_addr"`
	JoinedAt        uint64  `json:"joined_at"`
	LastSeenSecsAgo *uint64 `json:"last_seen_secs_ago"`
}

// ExperienceSyncStats 是经验同步的累计计数。
type ExperienceSyncStats struct {
	Broadcasts                 uint64 `json:"broadcasts"`
	InboundDelivered           uint64 `json:"inbound_delivered"`
	InboundApplied             uint64 `json:"inbound_applied"`
	InboundRejected            uint64 `json:"inbound_rejected"`
	InboundRejectedDeserialize uint64 `json:"inbound_rejected_deserialize"`
	InboundRejectedQuality     uint64 `json:"inbound_rejected_quality"`
	InboundRejectedApply       uint64 `json:"inbound_rejected_apply"`
}

// PeerManagerView 是 peer manager 的只读视图。
type PeerManagerView struct {
	SelfNodeID              *string             `json:"self_node_id"`
	LeaderID                *string             `json:"leader_id"`
	IsLeader                bool                `json:"is_leader"`
	ClusterSize             int                 `json:"cluster_size"`
	StateProposalBroadcasts uint64              `json:"state_proposal_broadcasts"`
	ExperienceSync          ExperienceSyncStats `json:"experience_sync"`
}

// ConsensusView 是 consensus trait 视图。
type ConsensusView struct {
	LeaderID *string `json:"leader_id"`
	IsLeader bool    `json:"is_leader"`
}

// ControlPlaneConfigView 是 `[cluster.control_plane]` 的生效配置快照。
type ControlPlaneConfigView struct {
	Enabled          bool   `json:"enabled"`
	ProposeTimeoutMS uint64 `json:"propose_timeout_ms"`
	ConnectTimeoutMS uint64 `json:"connect_timeout_ms"`
	LeaseTTLMinSecs  uint64 `json:"lease_ttl_min_secs"`
	LeaseTTLMaxSecs  uint64 `json:"lease_ttl_max_secs"`
}

// ClusterConfigView 是 `[cluster]` 的生效配置快照。
type ClusterConfigView struct {
	NodeID                   string                 `json:"node_id"`
	PeerTTLSecs              uint64                 `json:"peer_ttl_secs"`
	JoinTimeoutSecs          uint64                 `json:"join_timeout_secs"`
	StrategySyncIntervalSecs uint64                 `json:"strategy_sync_interval_secs"`
	StaticPeers              []string               `json:"static_peers"`
	ControlPlaneEnabled      bool                   `json:"control_plane_enabled"`
	ControlPlane             ControlPlaneConfigView `json:"control_plane"`
}

// ControlPlaneSecurityView 是控制面安全装配的运行时视图（ADR-019）。
type ControlPlaneSecurityView struct {
	TLSEnabledConfig           bool   `json:"tls_enabled_config"`
	TLSWired                   bool   `json:"tls_wired"`
	SubmitAuthModeConfig       string `json:"submit_auth_mode_config"`
	SubmitAuthEnforced         bool   `json:"submit_auth_enforced"`
	AppLayerSubmitAuthEnforced bool   `json:"app_layer_submit_auth_enforced"`
	EffectiveArmed             bool   `json:"effective_armed"`
	AllowedSubmitSubjectsCount int    `json:"allowed_submit_subjects_count"`
	Note                       string `json:"note"`
}

// ControlPlaneStatus 是控制面运行时状态（只读）。
type ControlPlaneStatus struct {
	Configured            bool                     `json:"configured"`
	CoordinatorConfigured bool                     `json:"coordinator_configured"`
	Security              ControlPlaneSecurityView `json:"security"`
	// Stats 是控制面统计快照；未 arm 时为 null。
	Stats json.RawMessage `json:"stats"`
}

// ClusterStatus 是 GET /api/cluster/status 的响应。
type ClusterStatus struct {
	Enabled      bool               `json:"enabled"`
	ClusterName  string             `json:"cluster_name"`
	Config       ClusterConfigView  `json:"config"`
	ControlPlane ControlPlaneStatus `json:"control_plane"`
	PeerManager  PeerManagerView    `json:"peer_manager"`
	Consensus    ConsensusView      `json:"consensus"`
	Peers        []ClusterPeer      `json:"peers"`
}

// 租约查询的状态码（GET /api/cluster/control/leases/{key}）。
const (
	LeaseStatusOK          = "OK"
	LeaseStatusNotFound    = "NOT_FOUND"
	LeaseStatusDisabled    = "DISABLED"
	LeaseStatusUnavailable = "UNAVAILABLE"
)

// ControlLease 是 GET /api/cluster/control/leases/{key} 的响应。
//
// 该端点是只读的（ADR-020：REST 写入=否）：控制面未启用时返回
// Found=false + StatusCode=DISABLED，而不是错误。
type ControlLease struct {
	Found           bool   `json:"found"`
	Key             string `json:"key"`
	Owner           string `json:"owner"`
	Generation      uint64 `json:"generation"`
	ExpiresAtUnixMS uint64 `json:"expires_at_unix_ms"`
	LastRequestID   string `json:"last_request_id"`
	StatusCode      string `json:"status_code"`
	StatusMessage   string `json:"status_message"`
}

// APIError 表示服务端返回的 HTTP 错误，可能包含可操作 hint。
type APIError struct {
	Status  int
	Message string
	Hint    string
	Raw     string
}

func (e *APIError) Error() string {
	if e.Hint != "" {
		return fmt.Sprintf("API error %d: %s (hint: %s)", e.Status, e.Message, e.Hint)
	}
	return fmt.Sprintf("API error %d: %s", e.Status, e.Message)
}

// Health 检查 bridge 状态。
func (c *Client) Health() (*HealthResponse, error) {
	var h HealthResponse
	if err := c.get("/health", &h); err != nil {
		return nil, err
	}
	return &h, nil
}

// ListSessions 返回所有会话。
func (c *Client) ListSessions() ([]Session, error) {
	var sessions []Session
	if err := c.get("/api/sessions", &sessions); err != nil {
		return nil, err
	}
	return sessions, nil
}

// GetSession 返回单个会话详情。
func (c *Client) GetSession(sessionID string) (*SessionDetail, error) {
	var s SessionDetail
	if err := c.get("/api/sessions/"+url.PathEscape(sessionID), &s); err != nil {
		return nil, err
	}
	return &s, nil
}

// CreateSession 创建新会话。
func (c *Client) CreateSession(name string) (*SessionDetail, error) {
	var s SessionDetail
	if err := c.post("/api/sessions", map[string]string{"name": name}, &s); err != nil {
		return nil, err
	}
	return &s, nil
}

// AddMessage 向会话追加一条消息，返回更新后的会话详情。
func (c *Client) AddMessage(sessionID, role, content string) (*SessionDetail, error) {
	body := map[string]string{"role": role, "content": content}
	var s SessionDetail
	path := "/api/sessions/" + url.PathEscape(sessionID) + "/messages"
	if err := c.post(path, body, &s); err != nil {
		return nil, err
	}
	return &s, nil
}

// SearchMemory 执行向量搜索。
func (c *Client) SearchMemory(query string, topK int) ([]SearchResult, error) {
	var results []SearchResult
	body := map[string]interface{}{"query": query, "top_k": topK}
	if err := c.post("/api/vector/search", body, &results); err != nil {
		return nil, err
	}
	return results, nil
}

// ExecuteSOP 通过 SOP 执行认知目标（POST /api/execute/goal）。
//
// 注意：`POST /api/execute` 是**工具执行**端点（tool_id/args_json），
// 不是 SOP 执行；旧版本 SDK 把两者混为一谈，请求体会被服务端拒绝。
// projectID 传空串表示使用服务端默认项目（"default"）。
func (c *Client) ExecuteSOP(sopID, goal, projectID string) (*GoalExecuteResult, error) {
	body := map[string]interface{}{"goal": goal, "sop_id": sopID}
	if projectID != "" {
		body["project_id"] = projectID
	}
	var r GoalExecuteResult
	if err := c.post("/api/execute/goal", body, &r); err != nil {
		return nil, err
	}
	return &r, nil
}

// ExecuteTool 按工具 ID 执行工具（POST /api/execute）。
//
// argsJSON 是工具参数的 JSON 字符串（例如 `{"path":"/tmp/a.txt"}`）；
// fuelLimit 传 0 表示使用服务端默认值。
func (c *Client) ExecuteTool(toolID, argsJSON string, fuelLimit uint64) (*ToolExecuteResult, error) {
	body := map[string]interface{}{"tool_id": toolID, "args_json": argsJSON}
	if fuelLimit > 0 {
		body["fuel_limit"] = fuelLimit
	}
	var r ToolExecuteResult
	if err := c.post("/api/execute", body, &r); err != nil {
		return nil, err
	}
	return &r, nil
}

// ListSOPs 返回可用的 SOP ID 列表（服务端返回字符串数组）。
func (c *Client) ListSOPs() ([]string, error) {
	var sops []string
	if err := c.get("/api/sops", &sops); err != nil {
		return nil, err
	}
	return sops, nil
}

// GetSOP 返回单个 SOP 的配置（GET /api/sops/{id}）。
//
// 该端点按工作区隔离：服务端要求 X-Sisyphus-Workspace-Id 头，
// 因此需设置 Client.WorkspaceID，否则服务端返回 400。
func (c *Client) GetSOP(sopID string) (*SopDetail, error) {
	var s SopDetail
	if err := c.get("/api/sops/"+url.PathEscape(sopID), &s); err != nil {
		return nil, err
	}
	return &s, nil
}

// ListSkills 返回本机已加载的技能列表。
func (c *Client) ListSkills() ([]SkillEntry, error) {
	var skills []SkillEntry
	if err := c.get("/api/skills", &skills); err != nil {
		return nil, err
	}
	return skills, nil
}

// InstallSkill 按 ID 安装远端技能（POST /api/skills/install）。
//
// 成功时服务端返回 201 且无响应体。
func (c *Client) InstallSkill(skillID string) error {
	return c.post("/api/skills/install", map[string]string{"skill_id": skillID}, nil)
}

// ValidateAuth 检查 API 密钥是否有效。
func (c *Client) ValidateAuth(apiKey string) (bool, error) {
	var resp struct {
		Valid   bool   `json:"valid"`
		Message string `json:"message"`
	}
	if err := c.post("/api/auth/validate", map[string]string{"api_key": apiKey}, &resp); err != nil {
		return false, err
	}
	return resp.Valid, nil
}

// ── 集群 / 控制面只读观测 ────────────────

// GetClusterStatus 返回集群协调的只读快照（GET /api/cluster/status）。
func (c *Client) GetClusterStatus() (*ClusterStatus, error) {
	var s ClusterStatus
	if err := c.get("/api/cluster/status", &s); err != nil {
		return nil, err
	}
	return &s, nil
}

// GetControlLease 读取指定 key 的控制面租约（GET /api/cluster/control/leases/{key}）。
//
// 该端点只读。控制面未启用 / 租约不存在时返回 200 + StatusCode，
// 不返回错误 —— 请以 StatusCode 判定（OK / NOT_FOUND / DISABLED / UNAVAILABLE）。
func (c *Client) GetControlLease(key string) (*ControlLease, error) {
	var lease ControlLease
	if err := c.get("/api/cluster/control/leases/"+url.PathEscape(key), &lease); err != nil {
		return nil, err
	}
	return &lease, nil
}

// ── 认知循环可观测性 ────────────────────

// AnomalyReports 读取认知循环观测到的异常。
//
// sopID 为空时返回全部作用域；服务端对未知作用域返回空切片，不会报错。
func (c *Client) AnomalyReports(sopID string) ([]AnomalyReport, error) {
	path := "/api/anomaly"
	if sopID != "" {
		path += "?sop_id=" + url.QueryEscape(sopID)
	}
	var reports []AnomalyReport
	if err := c.get(path, &reports); err != nil {
		return nil, err
	}
	return reports, nil
}

// ── 经验共享 ────────────────────────────

// ListSharedExperiences 列出所有已跨项目共享的经验。
func (c *Client) ListSharedExperiences() ([]SharedExperience, error) {
	var resp sharedExperiencesResponse
	if err := c.get("/api/experience/shared", &resp); err != nil {
		return nil, err
	}
	return resp.Experiences, nil
}

// ShareExperience 把一条本地经验标记为跨项目共享。
func (c *Client) ShareExperience(expKey string) (*ExperienceShareResult, error) {
	var resp ExperienceShareResult
	if err := c.post("/api/experience/share", map[string]string{"exp_key": expKey}, &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

// UnshareExperience 取消某项目的经验共享。
//
// projectID 必填（服务端缺少时返回 400），用于限定取消共享的作用域。
func (c *Client) UnshareExperience(expKey, projectID string) (*ExperienceShareResult, error) {
	body := map[string]string{"exp_key": expKey, "project_id": projectID}
	var resp ExperienceShareResult
	if err := c.post("/api/experience/unshare", body, &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

// ── 内部辅助 ──────────────────────────

func (c *Client) get(path string, result interface{}) error {
	req, err := http.NewRequest("GET", c.BaseURL+path, nil)
	if err != nil {
		return fmt.Errorf("create request: %w", err)
	}
	return c.do(req, result)
}

func (c *Client) post(path string, body interface{}, result interface{}) error {
	var reader io.Reader
	if body != nil {
		data, err := json.Marshal(body)
		if err != nil {
			return fmt.Errorf("marshal body: %w", err)
		}
		reader = bytes.NewReader(data)
	}
	req, err := http.NewRequest("POST", c.BaseURL+path, reader)
	if err != nil {
		return fmt.Errorf("create request: %w", err)
	}
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	return c.do(req, result)
}

func (c *Client) do(req *http.Request, result interface{}) error {
	if c.AuthToken != "" {
		req.Header.Set("Authorization", "Bearer "+c.AuthToken)
	}
	if c.WorkspaceID != "" {
		req.Header.Set("X-Sisyphus-Workspace-Id", c.WorkspaceID)
	}

	resp, err := c.HTTP.Do(req)
	if err != nil {
		return fmt.Errorf("request failed: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode >= 400 {
		body, _ := io.ReadAll(resp.Body)
		raw := string(body)
		msg := raw
		hint := ""
		var parsed map[string]interface{}
		if err := json.Unmarshal(body, &parsed); err == nil {
			if v, ok := parsed["error"].(string); ok && v != "" {
				msg = v
			}
			if v, ok := parsed["hint"].(string); ok && v != "" {
				hint = v
			}
		}
		return &APIError{Status: resp.StatusCode, Message: msg, Hint: hint, Raw: raw}
	}

	if result != nil {
		if err := json.NewDecoder(resp.Body).Decode(result); err != nil {
			return fmt.Errorf("decode response: %w", err)
		}
	}
	return nil
}
