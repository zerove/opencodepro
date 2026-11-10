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
	// createSession() 应该返回新创建的会话详情（POST /api/sessions → SessionDetail）
	expected := SessionDetail{ID: "s3", Name: "New Session", MessageCount: 0}
	srv := mockServer(t, 201, expected)
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

func TestGetSession(t *testing.T) {
	expected := SessionDetail{
		ID: "s1", Name: "Session 1", MessageCount: 1,
		Messages: []MessageDto{{Role: "user", Content: "hi"}},
	}
	srv := mockServer(t, 200, expected)
	defer srv.Close()

	c := NewClient(srv.URL, "")
	s, err := c.GetSession("s1")
	if err != nil {
		t.Fatalf("GetSession() 返回错误: %v", err)
	}
	if len(s.Messages) != 1 || s.Messages[0].Content != "hi" {
		t.Errorf("期望解析出 1 条消息，得到 %+v", s.Messages)
	}
}

func TestAddMessage(t *testing.T) {
	// addMessage() 必须 POST 到 /api/sessions/{id}/messages 并回传更新后的会话
	var gotPath string
	var gotBody map[string]string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.Path
		json.NewDecoder(r.Body).Decode(&gotBody)
		json.NewEncoder(w).Encode(SessionDetail{
			ID: "s1", Name: "Session 1", MessageCount: 2,
			Messages: []MessageDto{{Role: "user", Content: "hello"}},
		})
	}))
	defer srv.Close()

	c := NewClient(srv.URL, "")
	s, err := c.AddMessage("s1", "user", "hello")
	if err != nil {
		t.Fatalf("AddMessage() 返回错误: %v", err)
	}
	if gotPath != "/api/sessions/s1/messages" {
		t.Errorf("期望请求 /api/sessions/s1/messages，得到 %s", gotPath)
	}
	if gotBody["role"] != "user" || gotBody["content"] != "hello" {
		t.Errorf("请求体不正确: %v", gotBody)
	}
	if s.MessageCount != 2 {
		t.Errorf("期望 MessageCount=2，得到 %d", s.MessageCount)
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
	// executeSOP() 应该走 /api/execute/goal，并解出 output/report
	var gotPath string
	var gotBody map[string]interface{}
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.Path
		json.NewDecoder(r.Body).Decode(&gotBody)
		json.NewEncoder(w).Encode(map[string]interface{}{
			"output": map[string]interface{}{"answer": 42},
			"report": "ok",
		})
	}))
	defer srv.Close()

	c := NewClient(srv.URL, "")
	result, err := c.ExecuteSOP("sop1", "Analyze code", "proj1")
	if err != nil {
		t.Fatalf("ExecuteSOP() 返回错误: %v", err)
	}
	if gotPath != "/api/execute/goal" {
		t.Errorf("期望请求 /api/execute/goal，得到 %s", gotPath)
	}
	if gotBody["goal"] != "Analyze code" || gotBody["sop_id"] != "sop1" || gotBody["project_id"] != "proj1" {
		t.Errorf("请求体不符合 /api/execute/goal 契约: %v", gotBody)
	}
	if result.Report != "ok" {
		t.Errorf("期望 Report=ok，得到 %s", result.Report)
	}
	var out map[string]interface{}
	if err := json.Unmarshal(result.Output, &out); err != nil {
		t.Fatalf("output 不是合法 JSON: %v", err)
	}
	if out["answer"] != float64(42) {
		t.Errorf("期望 output.answer=42，得到 %v", out["answer"])
	}
}

func TestExecuteTool(t *testing.T) {
	// executeTool() 应该走 /api/execute（工具执行）并保持 args_json 为字符串
	var gotPath string
	var gotBody map[string]interface{}
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.Path
		json.NewDecoder(r.Body).Decode(&gotBody)
		json.NewEncoder(w).Encode(map[string]interface{}{
			"status": "SUCCESS", "result_json": "{\"ok\":true}",
		})
	}))
	defer srv.Close()

	c := NewClient(srv.URL, "")
	result, err := c.ExecuteTool("file_read", `{"path":"/tmp/a.txt"}`, 1000)
	if err != nil {
		t.Fatalf("ExecuteTool() 返回错误: %v", err)
	}
	if gotPath != "/api/execute" {
		t.Errorf("期望请求 /api/execute，得到 %s", gotPath)
	}
	if gotBody["tool_id"] != "file_read" {
		t.Errorf("期望 tool_id=file_read，得到 %v", gotBody["tool_id"])
	}
	if gotBody["args_json"] != `{"path":"/tmp/a.txt"}` {
		t.Errorf("args_json 必须是 JSON 字符串，得到 %v", gotBody["args_json"])
	}
	if result.Status != "SUCCESS" {
		t.Errorf("期望 Status=SUCCESS，得到 %s", result.Status)
	}
}

// ── SOPs ──────────────────────────────────────

func TestListSOPs(t *testing.T) {
	// listSOPs() 返回服务端实际契约：SOP ID 字符串数组
	srv := mockServer(t, 200, []string{"sop1", "sop2"})
	defer srv.Close()

	c := NewClient(srv.URL, "")
	sops, err := c.ListSOPs()
	if err != nil {
		t.Fatalf("ListSOPs() 返回错误: %v", err)
	}
	if len(sops) != 2 {
		t.Fatalf("期望 2 个 SOP，得到 %d", len(sops))
	}
	if sops[0] != "sop1" {
		t.Errorf("期望 ID=sop1，得到 %s", sops[0])
	}
}

func TestGetSOP(t *testing.T) {
	srv := mockServer(t, 200, map[string]interface{}{
		"id": "sop1", "name": "SOP 1", "version": "1.0.0",
		"config": map[string]interface{}{"nodes": []interface{}{}},
	})
	defer srv.Close()

	c := NewClient(srv.URL, "")
	sop, err := c.GetSOP("sop1")
	if err != nil {
		t.Fatalf("GetSOP() 返回错误: %v", err)
	}
	if sop.ID != "sop1" {
		t.Errorf("期望 ID=sop1，得到 %s", sop.ID)
	}
}

// ── Skills ───────────────────────────────────

func TestListSkills(t *testing.T) {
	// listSkills() 必须按服务端字段（name/description/version/author/tags/source）解码
	srv := mockServer(t, 200, []SkillEntry{
		{
			Name: "Skill 1", Description: "Test", Author: "author",
			Version: "1.0", Tags: []string{"demo"}, Source: "builtin",
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
	if skills[0].Source != "builtin" {
		t.Errorf("期望 Source=builtin，得到 %s", skills[0].Source)
	}
}

func TestInstallSkill(t *testing.T) {
	// installSkill() 成功时服务端返回 201 且无响应体
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusCreated)
	}))
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

// ── 集群 / 控制面只读观测 ─────────────────

func TestGetClusterStatus(t *testing.T) {
	// 解析 GET /api/cluster/status 的只读快照（含控制面安全装配视图）
	srv := mockServer(t, 200, map[string]interface{}{
		"enabled":      true,
		"cluster_name": "demo",
		"config": map[string]interface{}{
			"node_id": "node-1", "peer_ttl_secs": 30, "join_timeout_secs": 5,
			"strategy_sync_interval_secs": 10, "static_peers": []string{"node-2:50051"},
			"control_plane_enabled": true,
			"control_plane": map[string]interface{}{
				"enabled": true, "propose_timeout_ms": 2000,
				"connect_timeout_ms": 1000, "lease_ttl_min_secs": 5, "lease_ttl_max_secs": 300,
			},
		},
		"control_plane": map[string]interface{}{
			"configured": true, "coordinator_configured": true,
			"security": map[string]interface{}{
				"tls_enabled_config": true, "tls_wired": true,
				"submit_auth_mode_config": "mtls_subject_allowlist",
				"submit_auth_enforced":    true, "app_layer_submit_auth_enforced": true,
				"effective_armed": true, "allowed_submit_subjects_count": 3,
				"note": "ADR-019",
			},
			"stats": map[string]interface{}{"accepts": 2},
		},
		"peer_manager": map[string]interface{}{
			"self_node_id": "node-1", "leader_id": "node-1", "is_leader": true,
			"cluster_size": 3, "state_proposal_broadcasts": 7,
			"experience_sync": map[string]interface{}{
				"broadcasts": 1, "inbound_delivered": 2, "inbound_applied": 2,
				"inbound_rejected": 0, "inbound_rejected_deserialize": 0,
				"inbound_rejected_quality": 0, "inbound_rejected_apply": 0,
			},
		},
		"consensus": map[string]interface{}{"leader_id": "node-1", "is_leader": true},
		"peers": []map[string]interface{}{
			{"node_id": "node-1", "grpc_addr": "http://127.0.0.1:50051", "joined_at": 0, "last_seen_secs_ago": 0},
			{"node_id": "node-2", "grpc_addr": "http://127.0.0.1:50052", "joined_at": 0, "last_seen_secs_ago": nil},
		},
	})
	defer srv.Close()

	c := NewClient(srv.URL, "")
	status, err := c.GetClusterStatus()
	if err != nil {
		t.Fatalf("GetClusterStatus() 返回错误: %v", err)
	}
	if !status.Enabled || status.ClusterName != "demo" {
		t.Errorf("期望 enabled=true cluster_name=demo，得到 %+v", status)
	}
	if !status.ControlPlane.Security.TLSWired {
		t.Error("期望 control_plane.security.tls_wired=true")
	}
	if status.ControlPlane.Security.AllowedSubmitSubjectsCount != 3 {
		t.Errorf("期望 allowlist=3，得到 %d", status.ControlPlane.Security.AllowedSubmitSubjectsCount)
	}
	if status.PeerManager.ClusterSize != 3 || !status.PeerManager.IsLeader {
		t.Errorf("期望 cluster_size=3/is_leader=true，得到 %+v", status.PeerManager)
	}
	if len(status.Peers) != 2 {
		t.Fatalf("期望 2 个 peer，得到 %d", len(status.Peers))
	}
	if status.Peers[1].LastSeenSecsAgo != nil {
		t.Error("期望未知 last_seen_secs_ago 解码为 nil")
	}
	if len(status.ControlPlane.Stats) == 0 || string(status.ControlPlane.Stats) == "null" {
		t.Errorf("期望 stats 非 null，得到 %s", status.ControlPlane.Stats)
	}
}

func TestGetControlLeaseDisabled(t *testing.T) {
	// 控制面关闭时端点仍返回 200 + DISABLED（不是错误）
	srv := mockServer(t, 200, ControlLease{
		Found:         false,
		Key:           "lease:demo",
		StatusCode:    LeaseStatusDisabled,
		StatusMessage: "cluster.control_plane.enabled is false (ADR-019 gate)",
	})
	defer srv.Close()

	c := NewClient(srv.URL, "")
	lease, err := c.GetControlLease("lease:demo")
	if err != nil {
		t.Fatalf("GetControlLease() 返回错误: %v", err)
	}
	if lease.Found {
		t.Error("期望 found=false")
	}
	if lease.StatusCode != LeaseStatusDisabled {
		t.Errorf("期望 status_code=DISABLED，得到 %s", lease.StatusCode)
	}
}

func TestGetControlLeaseOK(t *testing.T) {
	var gotPath string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.Path
		json.NewEncoder(w).Encode(ControlLease{
			Found: true, Key: "lease/demo", Owner: "node-1", Generation: 4,
			ExpiresAtUnixMS: 1_700_000_000_000, LastRequestID: "req-1",
			StatusCode: LeaseStatusOK,
		})
	}))
	defer srv.Close()

	c := NewClient(srv.URL, "")
	lease, err := c.GetControlLease("lease/demo")
	if err != nil {
		t.Fatalf("GetControlLease() 返回错误: %v", err)
	}
	if gotPath != "/api/cluster/control/leases/lease%2Fdemo" && gotPath != "/api/cluster/control/leases/lease/demo" {
		t.Errorf("路径不正确: %s", gotPath)
	}
	if lease.Generation != 4 || lease.Owner != "node-1" {
		t.Errorf("期望 generation=4 owner=node-1，得到 %+v", lease)
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

func TestWorkspaceHeader(t *testing.T) {
	// GET /api/sops/{id} 要求 X-Sisyphus-Workspace-Id；验证 Client.WorkspaceID 生效
	var receivedWorkspace string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		receivedWorkspace = r.Header.Get("X-Sisyphus-Workspace-Id")
		json.NewEncoder(w).Encode(SopDetail{ID: "sop1", Name: "SOP 1"})
	}))
	defer srv.Close()

	c := NewClient(srv.URL, "")
	c.WorkspaceID = "proj1"
	if _, err := c.GetSOP("sop1"); err != nil {
		t.Fatalf("GetSOP() 返回错误: %v", err)
	}
	if receivedWorkspace != "proj1" {
		t.Errorf("期望 X-Sisyphus-Workspace-Id=proj1，得到 %q", receivedWorkspace)
	}
}
