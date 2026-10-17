// Package sisyphus 提供 Sisyphus 认知引擎 REST API 的 Go 客户端。
package sisyphus

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"time"
)

// Client 是 Sisyphus REST API 客户端。
type Client struct {
	BaseURL   string
	AuthToken string
	HTTP      *http.Client
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
	Status    string `json:"status"`
	Version   string `json:"version"`
	UptimeSecs uint64 `json:"uptime_secs"`
}

// Session 表示一个对话会话。
type Session struct {
	ID               string    `json:"id"`
	Name             string    `json:"name"`
	MessageCount     int       `json:"message_count"`
	CreatedAt        *string   `json:"created_at,omitempty"`
	ParentSessionID  *string   `json:"parent_session_id,omitempty"`
}

// Message 是会话中的一条消息。
type Message struct {
	Role            string `json:"role"`
	Content         string `json:"content"`
	CreatedAt       string `json:"created_at"`
	EstimatedTokens uint64 `json:"estimated_tokens"`
}

// SearchResult 来自向量搜索。
type SearchResult struct {
	ID      string  `json:"id"`
	Score   float64 `json:"score"`
	Content string  `json:"content"`
}

// ExecuteResult 来自 SOP 执行。
type ExecuteResult struct {
	SopID      string  `json:"sop_id"`
	Status     string  `json:"status"`
	Iterations uint32  `json:"iterations"`
	Deviation  float64 `json:"deviation"`
}

// SopInfo 描述一个可用的 SOP。
type SopInfo struct {
	ID          string `json:"id"`
	Name        string `json:"name"`
	NodeCount   int    `json:"node_count"`
	Description string `json:"description"`
}

// SkillEntry 是技能市场中的技能条目。
type SkillEntry struct {
	ID          string  `json:"id"`
	Name        string  `json:"name"`
	Description string  `json:"description"`
	Author      string  `json:"author"`
	Version     string  `json:"version"`
	Downloads   uint64  `json:"downloads"`
	Rating      float64 `json:"rating"`
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

// CreateSession 创建新会话。
func (c *Client) CreateSession(name string) (*Session, error) {
	var s Session
	if err := c.post("/api/sessions", map[string]string{"name": name}, &s); err != nil {
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

// ExecuteSOP 运行一个 SOP。
func (c *Client) ExecuteSOP(sopID string, context map[string]interface{}) (*ExecuteResult, error) {
	var r ExecuteResult
	body := map[string]interface{}{"sop_id": sopID, "context": context}
	if err := c.post("/api/execute", body, &r); err != nil {
		return nil, err
	}
	return &r, nil
}

// ListSOPs 返回可用的 SOP 列表。
func (c *Client) ListSOPs() ([]SopInfo, error) {
	var sops []SopInfo
	if err := c.get("/api/sops", &sops); err != nil {
		return nil, err
	}
	return sops, nil
}

// ListSkills 返回可用的技能列表。
func (c *Client) ListSkills() ([]SkillEntry, error) {
	var skills []SkillEntry
	if err := c.get("/api/skills", &skills); err != nil {
		return nil, err
	}
	return skills, nil
}

// InstallSkill 按 ID 安装技能。
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

// ── 内部辅助 ──────────────────────────

func (c *Client) get(path string, result interface{}) error {
	req, err := http.NewRequest("GET", c.BaseURL+path, nil)
	if err != nil {
		return fmt.Errorf("create request: %w", err)
	}
	return c.do(req, result)
}

func (c *Client) post(path string, body interface{}, result interface{}) error {
	data, err := json.Marshal(body)
	if err != nil {
		return fmt.Errorf("marshal body: %w", err)
	}
	req, err := http.NewRequest("POST", c.BaseURL+path, bytes.NewReader(data))
	if err != nil {
		return fmt.Errorf("create request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	return c.do(req, result)
}

func (c *Client) do(req *http.Request, result interface{}) error {
	if c.AuthToken != "" {
		req.Header.Set("Authorization", "Bearer "+c.AuthToken)
	}

	resp, err := c.HTTP.Do(req)
	if err != nil {
		return fmt.Errorf("request failed: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode >= 400 {
		body, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("API error %d: %s", resp.StatusCode, string(body))
	}

	if result != nil {
		if err := json.NewDecoder(resp.Body).Decode(result); err != nil {
			return fmt.Errorf("decode response: %w", err)
		}
	}
	return nil
}
