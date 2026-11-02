// Go SDK 单元测试
// 使用 httptest 模拟 HTTP 服务器，验证每个客户端方法

package sisyphus

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

// mockServer 创建一个测试用的 HTTP 服务器，返回固定响应
func mockServer(t *testing.T, status int, respBody interface{}) *httptest.Server {
	t.Helper()
	return httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(status)
		if respBody != nil {
			json.NewEncoder(w).Encode(respBody)
		}
	}))
}

// ── 构造器 ──────────────────────────────────────

func TestNewClient(t *testing.T) {
	// 验证 NewClient 返回非 nil 的客户端实例
	c := NewClient("http://localhost:8080", "")
	if c == nil {
		t.Fatal("NewClient() 返回了 nil")
	}
	if c.BaseURL != "http://localhost:8080" {
		t.Errorf("期望 BaseURL=http://localhost:8080，得到 %s", c.BaseURL)
	}
}

func TestNewClientWithAuth(t *testing.T) {
	// 验证 authToken 被正确设置
	c := NewClient("http://localhost:8080", "my-token")
	if c.AuthToken != "my-token" {
		t.Errorf("期望 AuthToken=my-token，得到 %s", c.AuthToken)
	}
}

// ── Health ─────────────────────────────────────

func TestHealth(t *testing.T) {
	// health() 应该返回正确的健康状态
	srv := mockServer(t, 200, HealthResponse{
		Status:     "ok",
		Version:    "1.0.0",
		UptimeSecs: 42,
	})
	defer srv.Close()

	c := NewClient(srv.URL, "")
	h, err := c.Health()
	if err != nil {
		t.Fatalf("Health() 返回错误: %v", err)
	}
	if h.Status != "ok" {
		t.Errorf("期望 Status=ok，得到 %s", h.Status)
	}
	if h.Version != "1.0.0" {
		t.Errorf("期望 Version=1.0.0，得到 %s", h.Version)
	}
	if h.UptimeSecs != 42 {
		t.Errorf("期望 UptimeSecs=42，得到 %d", h.UptimeSecs)
	}
}

// ── Sessions ───────────────────────────────────

func TestListSessions(t *testing.T) {
	// listSessions() 应该返回会话列表
	srv := mockServer(t, 200, []Session{
		{ID: "s1", Name: "Session 1", MessageCount: 5},
		{ID: "s2", Name: "Session 2", MessageCount: 3},
	})
	defer srv.Close()

	c := NewClient(srv.URL, "")
	sessions, err := c.ListSessions()
	if err != nil {
		t.Fatalf("ListSessions() 返回错误: %v", err)
	}
	if len(sessions) != 2 {
		t.Fatalf("期望 2 个 session，得到 %d", len(sessions))
	}
	if sessions[0].ID != "s1" {
		t.Errorf("期望第一个 session ID=s1，得到 %s", sessions[0].ID)
	}
}

func TestCreateSession(t *testing.T) {
	// createSession() 应该返回新创建的会话
	expected := Session{ID: "s3", Name: "New Session", MessageCount: 0}
	srv := mockServer(t, 200, expected)
	defer srv.Close()

	c := NewClient(srv.URL, "")
	s, err := c.CreateSession("New Session")
	if err != nil {
		t.Fatalf("CreateSession() 返回错误: %v", err)
	}
	if s.ID != "s3" {
		t.Errorf("期望 ID=s3，得到 %s", s.ID)
	}
	if s.Name != "New Session" {
		t.Errorf("期望 Name='New Session'，得到 %s", s.Name)
	}
}

// ── 向量搜索 ─────────────────────────────────

func TestSearchMemory(t *testing.T) {
	// searchMemory() 应该返回搜索结果
	srv := mockServer(t, 200, []SearchResult{
		{ID: "r1", Score: 0.95, Content: "result 1"},
	})
	defer srv.Close()

	c := NewClient(srv.URL, "")
	results, err := c.SearchMemory("query", 3)
	if err != nil {
		t.Fatalf("SearchMemory() 返回错误: %v", err)
	}
	if len(results) != 1 {
		t.Fatalf("期望 1 个结果，得到 %d", len(results))
	}
	if results[0].Score != 0.95 {
		t.Errorf("期望 Score=0.95，得到 %f", results[0].Score)
	}
}

// ── 执行 ─────────────────────────────────────

func TestExecuteSOP(t *testing.T) {
	// executeSOP() 应该返回执行结果
	srv := mockServer(t, 200, ExecuteResult{
		SopID:      "sop1",
		Status:     "completed",
		Iterations: 3,
		Deviation:  0.1,
	})
	defer srv.Close()

	c := NewClient(srv.URL, "")
	result, err := c.ExecuteSOP("sop1", map[string]interface{}{"input": "hello"})
	if err != nil {
		t.Fatalf("ExecuteSOP() 返回错误: %v", err)
	}
	if result.SopID != "sop1" {
		t.Errorf("期望 SopID=sop1，得到 %s", result.SopID)
	}
	if result.Status != "completed" {
		t.Errorf("期望 Status=completed，得到 %s", result.Status)
	}
	if result.Iterations != 3 {
		t.Errorf("期望 Iterations=3，得到 %d", result.Iterations)
	}
}

// ── SOPs ──────────────────────────────────────

func TestListSOPs(t *testing.T) {
	// listSOPs() 应该返回 SOP 列表
	srv := mockServer(t, 200, []SopInfo{
		{ID: "sop1", Name: "SOP 1", NodeCount: 5, Description: "Test SOP"},
	})
	defer srv.Close()

	c := NewClient(srv.URL, "")
	sops, err := c.ListSOPs()
	if err != nil {
		t.Fatalf("ListSOPs() 返回错误: %v", err)
	}
	if len(sops) != 1 {
		t.Fatalf("期望 1 个 SOP，得到 %d", len(sops))
	}
	if sops[0].ID != "sop1" {
		t.Errorf("期望 ID=sop1，得到 %s", sops[0].ID)
	}
}

// ── Skills ───────────────────────────────────

func TestListSkills(t *testing.T) {
	// listSkills() 应该返回技能列表
	srv := mockServer(t, 200, []SkillEntry{
		{
			ID: "sk1", Name: "Skill 1", Description: "Test",
			Author: "author", Version: "1.0",
			Downloads: 100, Rating: 4.5,
		},
	})
	defer srv.Close()

	c := NewClient(srv.URL, "")
	skills, err := c.ListSkills()
	if err != nil {
		t.Fatalf("ListSkills() 返回错误: %v", err)
	}
	if len(skills) != 1 {
		t.Fatalf("期望 1 个技能，得到 %d", len(skills))
	}
	if skills[0].Name != "Skill 1" {
		t.Errorf("期望 Name='Skill 1'，得到 %s", skills[0].Name)
	}
}

func TestInstallSkill(t *testing.T) {
	// installSkill() 成功时不应返回错误
	srv := mockServer(t, 200, map[string]string{"success": "true"})
	defer srv.Close()

	c := NewClient(srv.URL, "")
	if err := c.InstallSkill("sk1"); err != nil {
		t.Fatalf("InstallSkill() 返回错误: %v", err)
	}
}

// ── Auth ─────────────────────────────────────

func TestValidateAuth(t *testing.T) {
	// validateAuth() 对有效 key 应该返回 true
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body map[string]string
		json.NewDecoder(r.Body).Decode(&body)
		if body["api_key"] == "valid-key" {
			json.NewEncoder(w).Encode(map[string]interface{}{
				"valid": true, "message": "ok",
			})
		} else {
			json.NewEncoder(w).Encode(map[string]interface{}{
				"valid": false, "message": "invalid",
			})
		}
	}))
	defer srv.Close()

	c := NewClient(srv.URL, "")
	valid, err := c.ValidateAuth("valid-key")
	if err != nil {
		t.Fatalf("ValidateAuth() 返回错误: %v", err)
	}
	if !valid {
		t.Error("期望 valid=true，得到 false")
	}

	invalid, err := c.ValidateAuth("bad-key")
	if err != nil {
		t.Fatalf("ValidateAuth() 返回错误: %v", err)
	}
	if invalid {
		t.Error("期望 valid=false，得到 true")
	}
}

// ── 认知循环可观测性 ────────────────────────

func TestAnomalyReports(t *testing.T) {
	// 带 sop_id 时应拼查询参数，并解析异常列表
	var gotQuery string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotQuery = r.URL.Query().Get("sop_id")
		json.NewEncoder(w).Encode([]AnomalyReport{{
			NodeName:    "node_a",
			Severity:    "high",
			Metric:      "duration",
			Observed:    42,
			Expected:    10,
			Description: "duration deviation",
			Timestamp:   1_700_000_000,
		}})
	}))
	defer srv.Close()

	c := NewClient(srv.URL, "")
	reports, err := c.AnomalyReports("sop live/1")
	if err != nil {
		t.Fatalf("AnomalyReports() 返回错误: %v", err)
	}
	if gotQuery != "sop live/1" {
		t.Errorf("期望 sop_id='sop live/1'，得到 %q", gotQuery)
	}
	if len(reports) != 1 {
		t.Fatalf("期望 1 条异常，得到 %d", len(reports))
	}
	if reports[0].NodeName != "node_a" {
		t.Errorf("期望 NodeName=node_a，得到 %s", reports[0].NodeName)
	}
}

func TestAnomalyReportsWithoutScope(t *testing.T) {
	// 不传 sopID 时不应拼查询参数
	var gotRawQuery string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotRawQuery = r.URL.RawQuery
		json.NewEncoder(w).Encode([]AnomalyReport{})
	}))
	defer srv.Close()

	c := NewClient(srv.URL, "")
	reports, err := c.AnomalyReports("")
	if err != nil {
		t.Fatalf("AnomalyReports() 返回错误: %v", err)
	}
	if len(reports) != 0 {
		t.Errorf("期望空列表，得到 %d 条", len(reports))
	}
	if gotRawQuery != "" {
		t.Errorf("期望无查询参数，得到 %q", gotRawQuery)
	}
}

// ── 经验共享 ────────────────────────────

func TestListSharedExperiences(t *testing.T) {
	// 响应是 {experiences: [...]} 包装，方法应该解包
	srv := mockServer(t, 200, sharedExperiencesResponse{
		Experiences: []SharedExperience{{
			ContextText:    "ctx",
			ContextHash:    "hash",
			ActionID:       "act",
			OutcomeSuccess: true,
			DeviationScore: 0,
			Cost:           12,
			Timestamp:      1_700_000_000,
			Version:        1,
			VectorClock:    map[string]uint64{"node_a": 1},
		}},
	})
	defer srv.Close()

	c := NewClient(srv.URL, "")
	experiences, err := c.ListSharedExperiences()
	if err != nil {
		t.Fatalf("ListSharedExperiences() 返回错误: %v", err)
	}
	if len(experiences) != 1 {
		t.Fatalf("期望 1 条经验，得到 %d", len(experiences))
	}
	if experiences[0].ActionID != "act" {
		t.Errorf("期望 ActionID=act，得到 %s", experiences[0].ActionID)
	}
	if experiences[0].VectorClock["node_a"] != 1 {
		t.Errorf("期望 vector_clock[node_a]=1，得到 %d", experiences[0].VectorClock["node_a"])
	}
}

func TestShareExperience(t *testing.T) {
	// 应该 POST {"exp_key": ...} 并返回成功标志
	var gotBody map[string]string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		json.NewDecoder(r.Body).Decode(&gotBody)
		json.NewEncoder(w).Encode(ExperienceShareResult{Success: true})
	}))
	defer srv.Close()

	c := NewClient(srv.URL, "")
	result, err := c.ShareExperience("exp:1")
	if err != nil {
		t.Fatalf("ShareExperience() 返回错误: %v", err)
	}
	if !result.Success {
		t.Error("期望 Success=true，得到 false")
	}
	if gotBody["exp_key"] != "exp:1" {
		t.Errorf("期望 exp_key=exp:1，得到 %q", gotBody["exp_key"])
	}
}

func TestUnshareExperience(t *testing.T) {
	// 应该同时发送 exp_key 与 project_id，并透传失败信息
	var gotBody map[string]string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		json.NewDecoder(r.Body).Decode(&gotBody)
		json.NewEncoder(w).Encode(ExperienceShareResult{
			Success: false,
			Error:   "experience not found",
		})
	}))
	defer srv.Close()

	c := NewClient(srv.URL, "")
	result, err := c.UnshareExperience("exp:1", "proj1")
	if err != nil {
		t.Fatalf("UnshareExperience() 返回错误: %v", err)
	}
	if gotBody["exp_key"] != "exp:1" || gotBody["project_id"] != "proj1" {
		t.Errorf("期望 exp_key/project_id 都被发送，得到 %v", gotBody)
	}
	if result.Success {
		t.Error("期望 Success=false，得到 true")
	}
	if result.Error != "experience not found" {
		t.Errorf("期望 Error='experience not found'，得到 %q", result.Error)
	}
}

// ── 错误处理 ─────────────────────────────────

func TestHttpError(t *testing.T) {
	// HTTP 500 错误应该返回 error
	srv := mockServer(t, 500, map[string]string{"error": "internal"})
	defer srv.Close()

	c := NewClient(srv.URL, "")
	_, err := c.Health()
	if err == nil {
		t.Fatal("期望 Health() 返回错误，但没有")
	}
}

func TestAuthHeader(t *testing.T) {
	// 验证 authToken 被正确设置在请求头中
	var receivedToken string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		receivedToken = r.Header.Get("Authorization")
		w.WriteHeader(200)
		json.NewEncoder(w).Encode(HealthResponse{
			Status: "ok", Version: "1.0", UptimeSecs: 100,
		})
	}))
	defer srv.Close()

	c := NewClient(srv.URL, "my-token")
	c.Health()
	expected := "Bearer my-token"
	if receivedToken != expected {
		t.Errorf("期望 Authorization=%s，得到 %s", expected, receivedToken)
	}
}
