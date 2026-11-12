//! OpenAPI specification and Swagger UI for the REST API.

use axum::response::IntoResponse;
use axum::Json;

pub(crate) async fn openapi_json() -> impl IntoResponse {
    match build_document() {
        Ok(doc) => Json(doc).into_response(),
        // 文档构造失败是服务端缺陷，必须显式暴露成 500：
        // 早期版本在这里用 `unreachable!`，一旦某块 json! 少了包装键，
        // 请求会 panic 而不是返回可诊断的错误（且会被 no-panic 检查判红）。
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("OpenAPI document construction failed: {e}"),
        )
            .into_response(),
    }
}

/// 构造完整的 OpenAPI 文档。
///
/// 每个分块都必须带自己的顶层包装键（`paths` / `schemas`），
/// 缺失说明分块写错了 —— 返回 `Err` 而不是静默丢弃，
/// 否则 Swagger UI 会莫名少掉一整批端点。
fn build_document() -> Result<serde_json::Value, String> {
    // 这份文档不能用单个 `json!` 字面量表达：serde_json 的 muncher 对每个 key
    // 都吃掉一层宏展开深度，而 lib.rs 固定了 `#![recursion_limit = "256"]`。
    // 一份完整的 OpenAPI 文档（全部路径 + 全部 schema）远超这个上限，
    // 直接展开会报 "recursion limit reached while expanding $crate::json_internal"。
    // 所以这里按章节切成多个较小的 `json!`，再用 `extend` 合并。
    // 拆分的唯一约束是：每个 `json!` 字面量自身都要在 256 层以内。
    let mut path_chunks: Vec<serde_json::Value> = Vec::new();

    path_chunks.push(serde_json::json!({
        "paths": {
            // ── Health ───────────────────────────────────────────────────
            "/health": {
                "get": {
                    "summary": "Health check",
                    "operationId": "health",
                    "responses": {
                        "200": {
                            "description": "Service is healthy",
                            "content": {"application/json": {"schema": {"$ref": "#/components/schemas/HealthResponse"}}}
                        }
                    }
                }
            },
            "/health/deep": {
                "get": {
                    "summary": "Deep health check (per-component status)",
                    "description": "Probe every startup dependency (session DB, shared HTTP client, kernel) and report per-component status. Returns 200 when all OK; 207 Multi-Status on partial degradation. Use for K8s readiness probes or the TUI `:health` panel.",
                    "operationId": "healthDeep",
                    "responses": {
                        "200": {
                            "description": "All components healthy",
                            "content": {"application/json": {"schema": {"$ref": "#/components/schemas/HealthDeepResponse"}}}
                        },
                        "207": {
                            "description": "Partial degradation; see components for detail",
                            "content": {"application/json": {"schema": {"$ref": "#/components/schemas/HealthDeepResponse"}}}
                        }
                    }
                }
            },
            "/health/live": {
                "get": {
                    "summary": "K8s liveness probe (always 200 if process responds)",
                    "description": "Returns 200 unconditionally as long as the HTTP server is responsive. Does NOT check any dependencies (avoids pod restarts from transient downstream issues). Suitable for K8s livenessProbe.",
                    "operationId": "healthLive",
                    "responses": {
                        "200": {
                            "description": "Process is alive",
                            "content": {"application/json": {"schema": {"$ref": "#/components/schemas/HealthResponse"}}}
                        }
                    }
                }
            },
            "/health/ready": {
                "get": {
                    "summary": "K8s readiness probe (200 when all dependencies OK, 503 otherwise)",
                    "description": "Returns 200 when all critical dependencies (session DB, shared HTTP, kernel) are OK; 503 Service Unavailable on any degradation. Suitable for K8s readinessProbe — traffic only routed to ready pods.",
                    "operationId": "healthReady",
                    "responses": {
                        "200": {
                            "description": "All dependencies healthy",
                            "content": {"application/json": {"schema": {"$ref": "#/components/schemas/HealthDeepResponse"}}}
                        },
                        "503": {
                            "description": "Service unavailable; not ready to receive traffic",
                            "content": {"application/json": {"schema": {"$ref": "#/components/schemas/HealthDeepResponse"}}}
                        }
                    }
                }
            },

            // ── Tool Execution ───────────────────────────────────────────
            "/api/execute": {
                "post": {
                    "summary": "Execute a tool by id",
                    "operationId": "executeTool",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ExecuteRequest"}}}},
                    "responses": {
                        "200": {"description": "Execution result", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ExecuteResponse"}}}}
                    }
                }
            },

            "/api/tools/execute": {
                "post": {
                    "summary": "Execute a registered tool by id",
                    "description": "Routes args_json to the tool router by tool_id and returns the raw tool output as a string. fuel_limit falls back to the bridge default when omitted. Unlike /api/execute, tool failures are returned as HTTP 500 with status=\"ERROR: <message>\" rather than an error envelope.",
                    "operationId": "executeToolById",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ExecuteToolRequest"}}}},
                    "responses": {
                        "200": {"description": "Execution result", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ExecuteToolResponse"}}}},
                        "500": {"description": "Tool execution failed; status carries the error message", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ExecuteToolResponse"}}}}
                    }
                }
            },

            // ── Goal Execution ──────────────────────────────────────────
            "/api/execute/goal": {
                "post": {
                    "summary": "Execute a goal via SOP",
                    "operationId": "executeGoal",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/GoalExecuteRequest"}}}},
                    "responses": {
                        "200": {"description": "Goal execution result", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/GoalExecuteResponse"}}}},
                        "500": {"description": "Execution failed (may include actionable hint)", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ErrorResponse"}}}}
                    }
                }
            },
            "/api/execute/goal/{goal_id}/status": {
                "get": {
                    "summary": "Get goal execution status",
                    "operationId": "getGoalStatus",
                    "parameters": [{"name": "goal_id", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "responses": {
                        "200": {"description": "Goal status", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/GoalStatusResponse"}}}}
                    }
                }
            },
            "/api/execute/goal/{goal_id}/trace": {
                "get": {
                    "summary": "Get goal execution trace",
                    "operationId": "getGoalTrace",
                    "parameters": [{"name": "goal_id", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "responses": {
                        "200": {"description": "Execution trace", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/GoalTraceResponse"}}}}
                    }
                }
            },
            "/api/execute/goal/{goal_id}/stream": {
                "get": {
                    "summary": "Stream goal execution events (SSE)",
                    "operationId": "streamGoal",
                    "parameters": [{"name": "goal_id", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "responses": {
                        "200": {"description": "Server-sent event stream"}
                    }
                }
            },
            "/api/execute/composite": {
                "post": {
                    "summary": "Execute multiple goals as a composite DAG",
                    "operationId": "executeComposite",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CompositeExecuteRequest"}}}},
                    "responses": {
                        "200": {"description": "Composite execution results", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CompositeExecuteResponse"}}}},
                        "500": {"description": "Composite execution failed. sse_sop_id is still returned: node events for this run have already been broadcast by the time the failure is reported, and this id is the only way for a client to subscribe to them.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CompositeExecuteErrorResponse"}}}}
                    }
                }
            },

            // ── SSE ──────────────────────────────────────────────────────
        }
    }));

    path_chunks.push(serde_json::json!({
        "paths": {
            "/api/events": {
                "get": {
                    "summary": "Subscribe to the server-sent event stream",
                    "operationId": "streamEvents",
                    "description": "Long-lived SSE stream. All provided filters are ANDed; blank values are treated as absent. An event is delivered only when it carries every requested dimension (a missing dimension never matches). NOTE: reconnecting does not replay past events — event ids are random per push and last_event_id is parsed but unused.",
                    "parameters": [
                        {"name": "events", "in": "query", "required": false, "schema": {"type": "string"}, "description": "Comma-separated event types to include"},
                        {"name": "sop_id", "in": "query", "required": false, "schema": {"type": "string"}, "description": "Only events carrying this SOP id"},
                        {"name": "session_id", "in": "query", "required": false, "schema": {"type": "string"}, "description": "Only events carrying this session id"},
                        {"name": "project_id", "in": "query", "required": false, "schema": {"type": "string"}, "description": "Only events carrying this project id"},
                        {"name": "heartbeat", "in": "query", "required": false, "schema": {"type": "integer", "default": 30}, "description": "Heartbeat interval in seconds; 0 disables"},
                        {"name": "last_event_id", "in": "query", "required": false, "schema": {"type": "string"}, "description": "Parsed but currently unused; resumption is not implemented"}
                    ],
                    "responses": {
                        "200": {"description": "Server-sent event stream", "content": {"text/event-stream": {"schema": {"type": "string"}}}},
                        "429": {"description": "Too many concurrent SSE connections", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ErrorResponse"}}}}
                    }
                }
            },
            "/api/events/broadcast": {
                "post": {
                    "summary": "Broadcast a custom event to SSE subscribers",
                    "operationId": "broadcastEvent",
                    "description": "Top-level sop_id / session_id / project_id declare the event's ownership and take precedence over same-named keys inside payload. Blank values count as absent, leaving any payload-provided dimension intact.",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/BroadcastEventRequest"}}}},
                    "responses": {
                        "200": {"description": "Event broadcast", "content": {"application/json": {"schema": {"type": "object", "properties": {"status": {"type": "string"}}}}}},
                        "500": {"description": "No active subscribers", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ErrorResponse"}}}}
                    }
                }
            },
            "/api/events/health": {
                "get": {
                    "summary": "SSE subsystem health",
                    "operationId": "sseHealth",
                    "responses": {
                        "200": {"description": "SSE status", "content": {"application/json": {"schema": {"type": "object", "properties": {"status": {"type": "string"}, "service": {"type": "string"}}}}}}
                    }
                }
            },

            // ── Session Management ───────────────────────────────────────
            "/api/sessions": {
                "get": {
                    "summary": "List sessions",
                    "operationId": "listSessions",
                    "responses": {
                        "200": {"description": "Session list", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/SessionSummary"}}}}}
                    }
                },
                "post": {
                    "summary": "Create session",
                    "operationId": "createSession",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CreateSessionRequest"}}}},
                    "responses": {
                        "201": {"description": "Session created", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SessionDetail"}}}}
                    }
                }
            },
            "/api/sessions/{id}": {
                "get": {
                    "summary": "Get session details",
                    "operationId": "getSession",
                    "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "responses": {
                        "200": {"description": "Session details", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SessionDetail"}}}}
                    }
                }
            },
            "/api/sessions/{id}/messages": {
                "post": {
                    "summary": "Add message to session",
                    "operationId": "addMessage",
                    "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/AddMessageRequest"}}}},
                    "responses": {
                        "200": {"description": "Message added", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SessionDetail"}}}}
                    }
                }
            },

            // ── Tools & Agents ───────────────────────────────────────────
            "/api/tools": {
                "get": {
                    "summary": "List registered tools",
                    "operationId": "listTools",
                    "responses": {
                        "200": {"description": "Tool list", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/ToolEntry"}}}}}
                    }
                }
            },
            "/api/mcp/resources": {
                "get": {
                    "summary": "List MCP resources discovered from connected servers (startup snapshot)",
                    "operationId": "listMcpResources",
                    "responses": {
                        "200": {"description": "MCP resources grouped by server", "content": {"application/json": {"schema": {"type": "array", "items": {"type": "object"}}}}}
                    }
                }
            },
            "/api/mcp/prompts": {
                "get": {
                    "summary": "List MCP prompts discovered from connected servers (startup snapshot)",
                    "operationId": "listMcpPrompts",
                    "responses": {
                        "200": {"description": "MCP prompts grouped by server", "content": {"application/json": {"schema": {"type": "array", "items": {"type": "object"}}}}}
                    }
                }
            },
            "/api/mcp/resources/read": {
                "get": {
                    "summary": "Read an MCP resource on demand via a connected server",
                    "operationId": "mcpReadResource",
                    "parameters": [
                        {"name": "server", "in": "query", "required": true, "schema": {"type": "string"}},
                        {"name": "uri", "in": "query", "required": true, "schema": {"type": "string"}}
                    ],
                    "responses": {
                        "200": {"description": "Resource contents", "content": {"application/json": {"schema": {"type": "object"}}}},
                        "404": {"description": "MCP server not found"},
                        "502": {"description": "MCP server/protocol error"}
                    }
                }
            },
            "/api/mcp/prompts/get": {
                "post": {
                    "summary": "Get an MCP prompt on demand via a connected server",
                    "operationId": "mcpGetPrompt",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"type": "object", "required": ["server", "name"], "properties": {"server": {"type": "string"}, "name": {"type": "string"}, "arguments": {"type": "object"}}}}}},
                    "responses": {
                        "200": {"description": "Prompt result", "content": {"application/json": {"schema": {"type": "object"}}}},
                        "404": {"description": "MCP server not found"},
                        "502": {"description": "MCP server/protocol error"}
                    }
                }
            },
            "/api/agents": {
                "get": {
                    "summary": "List registered agents",
                    "operationId": "listAgents",
                    "responses": {
                        "200": {"description": "Agent list", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/AgentEntry"}}}}}
                    }
                }
            },

            // ── Skills ───────────────────────────────────────────────────
            "/api/skills": {
                "get": {
                    "summary": "List local skill entries",
                    "operationId": "listSkills",
                    "responses": {
                        "200": {"description": "Skill list", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/SkillEntry"}}}}}
                    }
                }
            },
            "/api/skills/{id}": {
                "get": {
                    "summary": "Get a single skill's metadata",
                    "operationId": "showSkill",
                    "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "responses": {
                        "200": {"description": "Skill found", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SkillDetailResponse"}}}},
                        "404": {"description": "Skill not found; body still carries found=false", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SkillDetailResponse"}}}},
                        "500": {"description": "Registry lookup failed", "content": {"application/json": {"schema": {"type": "string"}}}}
                    }
                }
            },

            "/api/skills/available": {
                "get": {
                    "summary": "List available remote skills",
                    "operationId": "listAvailableSkills",
                    "responses": {
                        "200": {"description": "Remote skill list", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/RemoteSkillEntry"}}}}}
                    }
                }
            },
            "/api/skills/install": {
                "post": {
                    "summary": "Install a remote skill",
                    "operationId": "installSkill",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/InstallSkillRequest"}}}},
                    "responses": {
                        "201": {"description": "Skill installed"},
                        "500": {"description": "Installation failed"}
                    }
                }
            },
            "/api/skills/installed": {
                "get": {
                    "summary": "List installed skills",
                    "operationId": "listInstalledSkills",
                    "responses": {
                        "200": {"description": "Installed skill list", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/SkillEntry"}}}}}
                    }
                }
            },
            "/api/skills/remote/{id}": {
                "delete": {
                    "summary": "Uninstall a remote skill",
                    "operationId": "uninstallSkill",
                    "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "responses": {
                        "204": {"description": "Skill uninstalled"},
                        "500": {"description": "Uninstall failed"}
                    }
                }
            },
            "/api/skills/search": {
                "get": {
                    "summary": "Search skill market across configured registries",
                    "operationId": "searchSkills",
                    "parameters": [{"name": "q", "in": "query", "required": true, "schema": {"type": "string"}}],
                    "responses": {
                        "200": {"description": "Search results", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SkillSearchResponse"}}}}
                    }
                }
            },
            "/api/skills/publish": {
                "post": {
                    "summary": "Publish a locally installed skill to a remote registry",
                    "operationId": "publishSkill",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/PublishSkillRequest"}}}},
                    "responses": {
                        "200": {"description": "Skill published", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/PublishSkillResponse"}}}},
                        "404": {"description": "Skill not installed locally"},
                        "500": {"description": "Publish failed"}
                    }
                }
            },
            "/peer/v1/skills": {
                "get": {
                    "summary": "Peer protocol: list skills this instance can serve",
                    "operationId": "peerListSkills",
                    "responses": {
                        "200": {"description": "Peer skill list", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/PeerSkillListResponse"}}}}
                    }
                },
                "post": {
                    "summary": "Peer protocol: receive a published skill bundle",
                    "operationId": "peerPublishSkill",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SkillBundle"}}}},
                    "responses": {
                        "201": {"description": "Skill installed"},
                        "500": {"description": "Install failed"}
                    }
                }
            },
            "/peer/v1/skills/{id}": {
                "get": {
                    "summary": "Peer protocol: fetch a skill bundle",
                    "operationId": "peerGetSkill",
                    "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "responses": {
                        "200": {"description": "Skill bundle", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SkillBundle"}}}},
                        "404": {"description": "Skill not found"}
                    }
                }
            },

            // ── Cognitive Map ────────────────────────────────────────────
            "/api/cognitive/map/{sop_id}": {
                "get": {
                    "summary": "Get cognitive map for an SOP",
                    "operationId": "getCognitiveMap",
                    "parameters": [{"name": "sop_id", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "responses": {
                        "200": {"description": "Cognitive map", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CogMapResponse"}}}}
                    }
                }
            },

            "/api/cognitive/map/{sop_id}/stream": {
                "get": {
                    "summary": "Stream cognitive map updates for an SOP (SSE)",
                    "description": "Complements /api/cognitive/map/{sop_id}: use the plain GET for a one-shot snapshot and this stream for live observation. On subscribe the server emits a `snapshot` event with the current CogMap, or `absent` when no map exists for this sop_id. Afterwards it polls the in-memory cognitive map every 500ms and emits an `update` event only when iteration or active_node changed. A `heartbeat` event is emitted every 15s to keep the connection alive. The stream ends when the client disconnects; axum also sends its own keep-alive comments.",
                    "operationId": "streamCognitiveMap",
                    "parameters": [{"name": "sop_id", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "responses": {
                        "200": {"description": "Server-sent event stream (event names: snapshot | absent | update | heartbeat)", "content": {"text/event-stream": {"schema": {"type": "string"}}}},
                        "429": {"description": "Too many concurrent SSE connections", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ErrorResponse"}}}}
                    }
                }
            },
            "/api/cognitive/map/{sop_id}/intervene": {
                "post": {
                    "summary": "Intervene on a running SOP's cognitive map",
                    "description": "Applies a live intervention to a running SOP. Supported actions: `abort` (drop the active execution entry so the run loop exits), `pause` / `resume` (flip execution status), `redirect` (set current_node to target_node). Every request also broadcasts an `intervention` SSE event to subscribers of this sop_id. Returns 200 when the action was applied to an active execution, 202 when only the broadcast happened (the SOP may have already finished); an unknown action is treated the same as the 202 case.",
                    "operationId": "interveneCognitiveMap",
                    "parameters": [{"name": "sop_id", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/InterventionRequest"}}}},
                    "responses": {
                        "200": {"description": "Intervention applied to the active execution", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/InterventionResponse"}}}},
                        "202": {"description": "Intervention broadcast but no active execution matched this sop_id", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/InterventionResponse"}}}}
                    }
                }
            },

            // ── Cognitive Trace (Explainability) ──────────────────────────
            "/api/cognitive/trace": {
                "get": {
                    "summary": "Get recent cognitive decisions (explainability)",
                    "description": "Returns the most recent CognitiveStep / MicroReflection decisions from the cognitive loop, newest first.",
                    "operationId": "getCognitiveTrace",
                    "parameters": [{"name": "limit", "in": "query", "required": false, "schema": {"type": "integer", "minimum": 1, "maximum": 200, "default": 100}, "description": "Max entries to return (default 100, cap 200)"}],
                    "responses": {
                        "200": {"description": "Recent cognitive decision chain", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CognitiveTraceResponse"}}}}
                    }
                }
            },
            "/api/cognitive/trace/history": {
                "get": {
                    "summary": "Get persisted cognitive decisions across sessions",
                    "description": "Returns cognitive decisions persisted to SQL, newest first. Survives bridge restarts; use it to inspect historical decision chains.",
                    "operationId": "getCognitiveTraceHistory",
                    "parameters": [
                        {"name": "sop_id", "in": "query", "required": false, "schema": {"type": "string"}, "description": "Filter by SOP id; omit for all SOPs"},
                        {"name": "limit", "in": "query", "required": false, "schema": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 100}, "description": "Max entries to return (default 100, cap 1000)"}
                    ],
                    "responses": {
                        "200": {"description": "Persisted cognitive decision chain", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CognitiveTraceResponse"}}}}
                    }
                }
            },

            "/api/cognitive/summary": {
                "get": {
                    "summary": "Get cognitive decision health summary",
                    "description": "Aggregates persisted cognitive decisions across sessions into a health summary: decision counts, mean metrics (deviation/confidence/stability/info-gain), cumulative token cost, and time range. Use it to assess cognitive loop stability and convergence quality.",
                    "operationId": "getCognitiveTraceSummary",
                    "parameters": [
                        {"name": "sop_id", "in": "query", "required": false, "schema": {"type": "string"}, "description": "Filter by SOP id; omit to aggregate all SOPs"},
                        {"name": "limit", "in": "query", "required": false, "schema": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 1000}, "description": "Max records to aggregate (default 1000, cap 1000)"}
                    ],
                    "responses": {
                        "200": {"description": "Cognitive decision health summary", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CognitiveTraceSummary"}}}}
                    }
                }
            },

            // ── Workspaces ───────────────────────────────────────────────
        }
    }));

    path_chunks.push(serde_json::json!({
        "paths": {
            "/api/workspaces": {
                "get": {
                    "summary": "List workspaces",
                    "operationId": "listWorkspaces",
                    "responses": {
                        "200": {"description": "Workspace list", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/WorkspaceEntry"}}}}}
                    }
                },
                "post": {
                    "summary": "Create workspace",
                    "operationId": "createWorkspace",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CreateWorkspaceRequest"}}}},
                    "responses": {
                        "201": {"description": "Workspace created", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/WorkspaceEntry"}}}}
                    }
                }
            },
            "/api/workspaces/switch": {
                "post": {
                    "summary": "Switch active workspace",
                    "operationId": "switchWorkspace",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SwitchWorkspaceRequest"}}}},
                    "responses": {
                        "200": {"description": "Workspace switched"}
                    }
                }
            },
            "/api/workspaces/current": {
                "get": {
                    "summary": "Get current workspace",
                    "operationId": "getCurrentWorkspace",
                    "responses": {
                        "200": {"description": "Current workspace or null", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/WorkspaceEntry"}}}}
                    }
                }
            },

            "/api/workspaces/{id}": {
                "delete": {
                    "summary": "Delete a workspace",
                    "description": "Removes the workspace registration. Sessions stored under this workspace id are not cascaded by this endpoint.",
                    "operationId": "deleteWorkspace",
                    "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "responses": {
                        "200": {"description": "Workspace deleted", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/DeleteWorkspaceResponse"}}}},
                        "500": {"description": "Deletion failed", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/DeleteWorkspaceResponse"}}}}
                    }
                }
            },

            // ── SOP CRUD ─────────────────────────────────────────────────
            "/api/sops": {
                "get": {
                    "summary": "List SOPs",
                    "operationId": "listSops",
                    "parameters": [{
                        "name": "X-Sisyphus-Workspace-Id",
                        "in": "header",
                        "required": false,
                        "schema": {"type": "string"}
                    }],
                    "responses": {
                        "200": {"description": "SOP list", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/SopDetailResponse"}}}}}
                    }
                },
                "post": {
                    "summary": "Save an SOP",
                    "operationId": "saveSop",
                    "parameters": [{
                        "name": "X-Sisyphus-Workspace-Id",
                        "in": "header",
                        "required": false,
                        "schema": {"type": "string"}
                    }],
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SopSaveRequest"}}}},
                    "responses": {
                        "201": {"description": "SOP saved"}
                    }
                }
            },
            "/api/sops/{id}": {
                "get": {
                    "summary": "Get SOP details",
                    "operationId": "getSop",
                    "parameters": [
                        {"name": "id", "in": "path", "required": true, "schema": {"type": "string"}},
                        {"name": "X-Sisyphus-Workspace-Id", "in": "header", "required": false, "schema": {"type": "string"}}
                    ],
                    "responses": {
                        "200": {"description": "SOP details", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SopDetailResponse"}}}}
                    }
                },
                "delete": {
                    "summary": "Delete an SOP",
                    "operationId": "deleteSop",
                    "parameters": [
                        {"name": "id", "in": "path", "required": true, "schema": {"type": "string"}},
                        {"name": "X-Sisyphus-Workspace-Id", "in": "header", "required": false, "schema": {"type": "string"}}
                    ],
                    "responses": {
                        "204": {"description": "SOP deleted"}
                    }
                }
            },

            // ── Experience Engine ────────────────────────────────────────
            "/api/roadmap/delay-alerts": {
                "post": {
                    "summary": "Check milestones for schedule-slippage risk",
                    "operationId": "checkDelayAlerts",
                    "responses": {"200": {"description": "Delay alerts"}}
                }
            },
            "/api/roadmap/parallel-execution": {
                "post": {
                    "summary": "Suggest goals that could run concurrently",
                    "operationId": "suggestParallelExecution",
                    "responses": {"200": {"description": "Suggestions"}}
                }
            },
            "/api/roadmap/goal-resources": {
                "post": {
                    "summary": "Estimate resource needs for a goal",
                    "operationId": "estimateGoalResources",
                    "responses": {"200": {"description": "Estimate"}}
                }
            },
            "/api/roadmap/proposals/{project_id}": {
                "get": {
                    "summary": "List roadmap change proposals",
                    "operationId": "listProposals",
                    "parameters": [{"name": "project_id", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "responses": {"200": {"description": "Proposals"}}
                }
            },
            "/api/roadmap/proposals/{project_id}/{proposal_id}": {
                "get": {
                    "summary": "Fetch one roadmap change proposal",
                    "operationId": "getProposal",
                    "parameters": [{"name": "project_id", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "responses": {"200": {"description": "Proposal"}}
                }
            },
            "/api/roadmap/proposals/approve": {
                "post": {
                    "summary": "Approve a roadmap change proposal",
                    "operationId": "approveProposal",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"type": "object"}}}},
                    "responses": {"200": {"description": "Approved"}}
                }
            },
            "/api/roadmap/proposals/reject": {
                "post": {
                    "summary": "Reject a roadmap change proposal",
                    "operationId": "rejectProposal",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"type": "object"}}}},
                    "responses": {"200": {"description": "Rejected"}}
                }
            },
            "/api/cognitive/delegate": {
                "post": {
                    "summary": "Delegate a cognitive task to a specialised agent",
                    "operationId": "delegateCognitiveTask",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"type": "object"}}}},
                    "responses": {"200": {"description": "Delegated"}}
                }
            },
            "/api/state": {
                "get": {
                    "summary": "List all state entries for a project",
                    "operationId": "listState",
                    "parameters": [{"name": "project_id", "in": "query", "required": false, "schema": {"type": "string", "default": "default"}}],
                    "responses": {
                        "200": {"description": "State entries", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/StateEntry"}}}}}
                    }
                }
            },
            "/api/state/{key}": {
                "get": {
                    "summary": "Read a single state entry",
                    "operationId": "getState",
                    "parameters": [{"name": "key", "in": "path", "required": true, "schema": {"type": "string"}}, {"name": "project_id", "in": "query", "required": false, "schema": {"type": "string", "default": "default"}}],
                    "responses": {
                        "200": {"description": "State entry", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/StateEntry"}}}},
                        "404": {"description": "No such key"}
                    }
                },
                "put": {
                    "summary": "Create or replace a state entry",
                    "operationId": "setState",
                    "parameters": [{"name": "key", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"type": "object", "required": ["value"], "properties": {"project_id": {"type": "string"}, "value": {}}}}}},
                    "responses": {"204": {"description": "Stored"}}
                }
            },
            "/api/vector/search": {
                "post": {
                    "summary": "Semantic search over stored vectors",
                    "operationId": "vectorSearch",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"type": "object", "required": ["query"], "properties": {"query": {"type": "string"}, "top_k": {"type": "integer", "default": 5}}}}}},
                    "responses": {
                        "200": {"description": "Scored matches", "content": {"application/json": {"schema": {"type": "array", "items": {"type": "object", "properties": {"key": {"type": "string"}, "score": {"type": "number", "format": "float"}, "metadata": {"type": "object"}}}}}}},
                        "400": {"description": "Empty query"},
                        "503": {"description": "Embedding provider unavailable"}
                    }
                }
            },
            "/api/auth/validate": {
                "post": {
                    "summary": "Validate API credentials before issuing requests",
                    "operationId": "validateAuth",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"type": "object", "required": ["api_key"], "properties": {"api_key": {"type": "string"}}}}}},
                    "responses": {
                        "200": {"description": "Credentials accepted", "content": {"application/json": {"schema": {"type": "object", "properties": {"valid": {"type": "boolean"}, "message": {"type": "string"}}}}}},
                        "400": {"description": "Missing api_key"},
                        "401": {"description": "Invalid credentials"}
                    }
                }
            },
            "/api/experience/similar": {
                "post": {
                    "summary": "Query similar experiences",
                    "operationId": "querySimilarExperiences",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ExperienceQueryRequest"}}}},
                    "responses": {
                        "200": {"description": "Similar experiences", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ExperienceSimilarResponse"}}}}
                    }
                }
            },
            "/api/experience/share": {
                "post": {
                    "summary": "Share an experience across projects (multi-workspace knowledge transfer)",
                    "operationId": "shareExperience",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ShareExperienceRequest"}}}},
                    "responses": {
                        "200": {"description": "Experience shared", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ShareExperienceResponse"}}}}
                    }
                }
            },
            "/api/experience/unshare": {
                "post": {
                    "summary": "Restore project ownership of a shared experience",
                    "operationId": "unshareExperience",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ShareExperienceRequest"}}}},
                    "responses": {
                        "200": {"description": "Experience ownership restored", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ShareExperienceResponse"}}}},
                        "400": {"description": "Missing project_id"}
                    }
                }
            },
            "/api/experience/shared": {
                "get": {
                    "summary": "List all globally shared experiences",
                    "operationId": "listSharedExperiences",
                    "responses": {
                        "200": {"description": "Shared experiences", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SharedExperiencesResponse"}}}}
                    }
                }
            },

            "/api/experience/capture": {
                "post": {
                    "summary": "Capture a successful plan as a reusable experience",
                    "description": "Parses plan_json into a CognitivePlan and stores it under plan_id in the experience bank so later runs can retrieve it via /api/experience/similar. project_id resolves from the x-project-id header first, then the body field, then \"default\".",
                    "operationId": "captureExperience",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CaptureExperienceRequest"}}}},
                    "responses": {
                        "200": {"description": "Experience captured", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CaptureExperienceResponse"}}}},
                        "400": {"description": "plan_json is not a valid CognitivePlan", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CaptureExperienceResponse"}}}},
                        "500": {"description": "Capture failed", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CaptureExperienceResponse"}}}}
                    }
                }
            },
            "/api/feedback": {
                "post": {
                    "summary": "Submit execution feedback and trigger evolution",
                    "description": "Parses feedback_json into an ExecutionFeedback and hands it to the cognitive evolution loop, returning the resulting evolution record serialized as evolution_json. project_id resolves from the x-project-id header first, then the body field, then \"default\".",
                    "operationId": "submitFeedback",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SubmitFeedbackRequest"}}}},
                    "responses": {
                        "200": {"description": "Feedback accepted and evolution produced", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SubmitFeedbackResponse"}}}},
                        "400": {"description": "feedback_json is not a valid ExecutionFeedback", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SubmitFeedbackResponse"}}}},
                        "500": {"description": "Evolution failed", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SubmitFeedbackResponse"}}}}
                    }
                }
            },

            // ── 联邦策略权重（v0.4.0 Phase B，ADR-017）────────────────────
            "/api/strategy/weights": {
                "get": {
                    "summary": "Get the federated strategy weights view",
                    "description": "Returns the cluster-wide strategy weight snapshot: per-node strategy statistics plus the aggregated view. Each node reports strategy statistics through the StateProposal cluster event and the bridge merges them by generation. An empty view means no node has reported yet.",
                    "operationId": "getStrategyWeights",
                    "responses": {
                        "200": {"description": "Federated strategy weights", "content": {"application/json": {"schema": {"type": "object", "description": "node_count (integer), nodes (array), aggregate (object of strategy stats), best (strategy pick or null)"}}}}
                    }
                }
            },
            "/api/cluster/status": {
                "get": {
                    "summary": "Get cluster coordination status",
                    "description": "Read-only snapshot of cluster coordination state: config (enabled/name/peer TTL/strategy sync interval, control plane config), peer manager view (self/leader/size, state_proposal_broadcasts, experience_sync counters incl. reject breakdown), control_plane runtime stats, consensus trait view (leader/is_leader), and peer list with last_seen age. Does not change propagation, merge, or decision paths.",
                    "operationId": "getClusterStatus",
                    "responses": {
                        "200": {"description": "Cluster coordination status", "content": {"application/json": {"schema": {"type": "object", "description": "enabled, cluster_name, config, control_plane, peer_manager (incl. experience_sync), consensus, peers"}}}},
                        "401": {"description": "Unauthorized (only when REST auth is enabled: bearer token or JWT)"}
                    },
                    "security": [{"bearerAuth": []}]
                }
            },
            "/api/cluster/control/leases/{key}": {
                "get": {
                    "summary": "Get control-plane lease (ADR-019, read-only)",
                    "description": "Read-only lease query aligned with gRPC GetControlLease. Does not change state. Returns status_code OK/NOT_FOUND/DISABLED/UNAVAILABLE; DISABLED when cluster.control_plane.enabled=false. Used for uncertain submission verification (retry with same request_id, then query).",
                    "operationId": "getControlLease",
                    "parameters": [{"name": "key", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "responses": {
                        "200": {"description": "Lease query result", "content": {"application/json": {"schema": {"type": "object", "description": "found, key, owner, generation, expires_at_unix_ms, last_request_id, status_code, status_message"}}}},
                        "401": {"description": "Unauthorized (only when REST auth is enabled: bearer token or JWT)"}
                    },
                    "security": [{"bearerAuth": []}]
                }
            },

            // ── Roadmap ──────────────────────────────────────────────────
        }
    }));

    path_chunks.push(serde_json::json!({
        "paths": {
            "/api/roadmap/project/{id}": {
                "get": {
                    "summary": "Get project details",
                    "operationId": "getProject",
                    "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "responses": {
                        "200": {"description": "Project details"},
                        "404": {"description": "Project not found"}
                    }
                },
                "post": {
                    "summary": "Create a new project",
                    "operationId": "createProject",
                    "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CreateProjectRequest"}}}},
                    "responses": {
                        "201": {"description": "Project created"}
                    }
                }
            },
            "/api/roadmap/milestone": {
                "post": {
                    "summary": "Add milestone to project",
                    "operationId": "addMilestone",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/AddMilestoneRequest"}}}},
                    "responses": {
                        "201": {"description": "Milestone added"}
                    }
                }
            },
            "/api/roadmap/phase": {
                "post": {
                    "summary": "Add phase to milestone",
                    "operationId": "addPhase",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/AddPhaseRequest"}}}},
                    "responses": {
                        "201": {"description": "Phase added"}
                    }
                }
            },
            "/api/roadmap/goal/{goal_id}": {
                "patch": {
                    "summary": "Update goal status",
                    "operationId": "updateGoalStatus",
                    "parameters": [{"name": "goal_id", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/UpdateGoalStatusRequest"}}}},
                    "responses": {
                        "200": {"description": "Goal status updated"}
                    }
                }
            },
            "/api/roadmap/health/{project_id}": {
                "get": {
                    "summary": "Evaluate project health",
                    "operationId": "evaluateHealth",
                    "parameters": [{"name": "project_id", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "responses": {
                        "200": {"description": "Health evaluation"}
                    }
                }
            },
            "/api/roadmap/dashboard/{project_id}": {
                "get": {
                    "summary": "Aggregate project roadmap dashboard",
                    "description": "Single-call dashboard payload: project metadata, milestone/phase/goal structure, computed progress, and health snapshot.",
                    "operationId": "getRoadmapDashboard",
                    "parameters": [{"name": "project_id", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "responses": {
                        "200": {"description": "Dashboard aggregate"},
                        "404": {"description": "Project not found"}
                    }
                }
            },

            "/api/roadmap/predict": {
                "post": {
                    "summary": "Predict project completion time with confidence intervals",
                    "description": "Derives per-milestone velocity (goals/sec) from stored execution outcomes, then projects remaining time with a 95% confidence interval at both milestone and project level. has_sufficient_data reports whether the history was rich enough to make the projection meaningful.",
                    "operationId": "predictCompletion",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/PredictCompletionRequest"}}}},
                    "responses": {
                        "200": {"description": "Completion time prediction", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CompletionTimePrediction"}}}},
                        "500": {"description": "Prediction failed; body is the error message as a JSON string", "content": {"application/json": {"schema": {"type": "string"}}}}
                    }
                }
            },
            "/api/roadmap/project-costs": {
                "post": {
                    "summary": "Project remaining cost and duration",
                    "description": "Extrapolates remaining cost and wall-clock duration from the project's recorded execution outcomes. total_goals / completed_goals come from the caller because progress bookkeeping lives on the roadmap side.",
                    "operationId": "projectRoadmapCosts",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ProjectCostsRequest"}}}},
                    "responses": {
                        "200": {"description": "Cost projection", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CostProjection"}}}},
                        "500": {"description": "Projection failed; body is the error message as a JSON string", "content": {"application/json": {"schema": {"type": "string"}}}}
                    }
                }
            },
            "/api/roadmap/bottlenecks": {
                "post": {
                    "summary": "Find execution pipeline bottlenecks",
                    "description": "Aggregates stored execution outcomes into per-goal bottleneck records ranked by severity (low / medium / high), failure count, average duration and downstream blocking count.",
                    "operationId": "findRoadmapBottlenecks",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/FindBottlenecksRequest"}}}},
                    "responses": {
                        "200": {"description": "Detected bottlenecks", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/FindBottlenecksResponse"}}}},
                        "500": {"description": "Analysis failed; body is the error message as a JSON string", "content": {"application/json": {"schema": {"type": "string"}}}}
                    }
                }
            },

            // ── Observability ───────────────────────────────────────────
        }
    }));

    path_chunks.push(serde_json::json!({
        "paths": {
            "/api/metrics": {
                "get": {
                    "summary": "Prometheus text-format metrics",
                    "description": "Exposes Sisyphus bridge metrics in Prometheus 0.0.4 exposition format. Counters: sisyphus_http_requests_total / sisyphus_http_errors_total / sisyphus_http_shutdowns_total / sisyphus_chaos_faults_total. Gauges: sisyphus_http_in_flight / sisyphus_sse_connections_active / sisyphus_uptime_seconds. Scrape this endpoint with Prometheus or any OTLP-compatible backend.",
                    "operationId": "prometheusMetrics",
                    "responses": {
                        "200": {
                            "description": "Prometheus exposition format (text/plain; version=0.0.4)",
                            "content": {"text/plain": {"schema": {"type": "string", "description": "Prometheus 0.0.4 text exposition"}}}
                        }
                    }
                }
            },

            // ── Observability (Benchmark / Breaker / Insight) ────────────
            "/api/benchmarks": {
                "get": {
                    "summary": "List recorded benchmark runs",
                    "description": "Returns the last 100 runs for every registered benchmark name, flattened into a single list. The name_filter query parameter is accepted for API compatibility but currently not applied by the handler.",
                    "operationId": "listBenchmarks",
                    "parameters": [{"name": "name_filter", "in": "query", "required": false, "schema": {"type": "string"}, "description": "Filter by benchmark name; accepted but not currently applied"}],
                    "responses": {
                        "200": {"description": "Benchmark runs", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ListBenchmarksResponse"}}}}
                    }
                }
            },
            "/api/breakers": {
                "get": {
                    "summary": "Circuit breaker metrics per model",
                    "description": "Circuit breaker state is tracked inside the LLM client trait object, which the bridge cannot downcast, so this endpoint currently returns an empty breaker list. The model_name filter is accepted but unused.",
                    "operationId": "getBreakerMetrics",
                    "parameters": [{"name": "model_name", "in": "query", "required": false, "schema": {"type": "string"}, "description": "Filter by model; accepted but not currently applied"}],
                    "responses": {
                        "200": {"description": "Breaker metrics (currently always an empty list)", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/BreakerMetricsResponse"}}}}
                    }
                }
            },
            "/api/insights": {
                "get": {
                    "summary": "Meta-learning insights per strategy",
                    "description": "Aggregates the meta-learner's running statistics into per-strategy success rates and durations plus an overall success rate. strategy_filter narrows the result to a single strategy when supplied.",
                    "operationId": "getLearningInsights",
                    "parameters": [{"name": "strategy_filter", "in": "query", "required": false, "schema": {"type": "string"}, "description": "Return only this strategy; omit for all"}],
                    "responses": {
                        "200": {"description": "Learning insights", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/LearningInsightsResponse"}}}}
                    }
                }
            },
            "/api/anomaly": {
                "get": {
                    "summary": "Anomaly reports for an SOP",
                    "description": "Returns the anomalies detected by the cognitive loop's AnomalyDetector (fuel, duration and error-rate deviations), newest last. The detector keeps a bounded buffer of the most recent detections; results are scoped to the supplied sop_id. An empty array means the loop observed no anomalies for that run.",
                    "operationId": "getAnomalyReports",
                    "parameters": [{"name": "sop_id", "in": "query", "required": true, "schema": {"type": "string"}, "description": "Cognitive-loop run to report on (the sop_id passed to run_cognitive_loop)"}],
                    "responses": {
                        "200": {"description": "Anomaly reports for the run (possibly empty)", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/AnomalyReport"}}}}}
                    }
                }
            },

            // ── Plugins ──────────────────────────────────────────────────
            "/api/plugins": {
                "get": {
                    "summary": "List registered WASM plugins",
                    "operationId": "listPlugins",
                    "responses": {
                        "200": {"description": "Plugin list", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/PluginDescriptor"}}}}}
                    }
                }
            },
            "/api/plugins/load": {
                "post": {
                    "summary": "Load (register) a WASM plugin",
                    "description": "Registers a plugin descriptor with the plugin manager, which compiles and instantiates the WASM module at wasm_path and exposes it as tool_id. fuel_limit defaults to 0 when omitted, and the checksum is always left unset by this endpoint.",
                    "operationId": "loadPlugin",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/LoadPluginRequest"}}}},
                    "responses": {
                        "200": {"description": "Plugin loaded", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/LoadPluginResponse"}}}},
                        "500": {"description": "Plugin registration failed", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/LoadPluginResponse"}}}}
                    }
                }
            },
            "/api/plugins/unload": {
                "post": {
                    "summary": "Unload a WASM plugin by name",
                    "operationId": "unloadPlugin",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/UnloadPluginRequest"}}}},
                    "responses": {
                        "200": {"description": "Plugin unloaded", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/UnloadPluginResponse"}}}},
                        "500": {"description": "Unload failed (e.g. no such plugin)", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/UnloadPluginResponse"}}}}
                    }
                }
            },

            // ── JCode Delegation ─────────────────────────────────────────
            "/api/jcode/call": {
                "post": {
                    "summary": "Delegate a task to JCode over the sandbox bridge",
                    "description": "Forwards {task_id, goal, context} to the jcode/call tool on the sandbox RPC client and returns the raw tool output as a string in `result`.",
                    "operationId": "callJcode",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CallJcodeRequest"}}}},
                    "responses": {
                        "200": {"description": "JCode call completed", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CallJcodeResponse"}}}},
                        "400": {"description": "Context could not be encoded", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CallJcodeResponse"}}}},
                        "500": {"description": "JCode call failed", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CallJcodeResponse"}}}}
                    }
                }
            },

            // ── Chaos Engineering ────────────────────────────────────────
            "/api/chaos/start": {
                "post": {
                    "summary": "Start a chaos experiment",
                    "description": "Injects faults into matching operations for the configured duration. fault_types accepts LlmTimeout | LlmGarbage | ToolCrash | ToolHang | StorageWriteFailure | StorageReadFailure | NetworkPartition | RandomDelay(<ms>); unrecognised entries are silently dropped. An empty target_operations list means all operations. The returned experiment_id is generated by the handler and is not tracked by the chaos runner itself, so /api/chaos/status ignores it.",
                    "operationId": "chaosStart",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ChaosStartRequest"}}}},
                    "responses": {
                        "200": {"description": "Experiment started", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ChaosStartResponse"}}}}
                    }
                }
            },
            "/api/chaos/stop": {
                "post": {
                    "summary": "Stop the running chaos experiment",
                    "description": "Stops whichever experiment is currently running and returns its final counters. experiment_id is accepted in the body but unused — the runner tracks a single active experiment.",
                    "operationId": "chaosStop",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ChaosStopRequest"}}}},
                    "responses": {
                        "200": {"description": "Experiment stopped with final results", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ChaosStopResponse"}}}},
                        "404": {"description": "No experiment was running", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ChaosStopResponse"}}}}
                    }
                }
            },
            "/api/chaos/status": {
                "get": {
                    "summary": "Get chaos experiment status",
                    "description": "Reports whether an experiment is currently running. experiment_id is accepted as a query parameter but unused — the runner tracks a single active experiment. Counter fields in `result` are placeholders while an experiment is active.",
                    "operationId": "chaosStatus",
                    "parameters": [{"name": "experiment_id", "in": "query", "required": true, "schema": {"type": "string"}, "description": "Accepted but unused; the runner tracks a single active experiment"}],
                    "responses": {
                        "200": {"description": "Chaos status", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ChaosStatusResponse"}}}}
                    }
                }
            }
        }
    }));

    // 每块都带一层 `{"paths": { ... }}` 包装，直接 `extend` 会互相覆盖同名 key。
    // 先把包装层拆掉再合并，才能得到扁平的路径表。
    fn flatten(
        chunk: serde_json::Value,
        key: &str,
        into: &mut serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), String> {
        match chunk.get(key).and_then(serde_json::Value::as_object) {
            Some(block) => {
                into.extend(block.iter().map(|(k, v)| (k.clone(), v.clone())));
                Ok(())
            },
            None => Err(format!("OpenAPI 分块缺少顶层包装键 `{key}`")),
        }
    }
    let mut all_paths = serde_json::Map::new();
    for chunk in path_chunks {
        flatten(chunk, "paths", &mut all_paths)?;
    }

    let mut schema_chunks: Vec<serde_json::Value> = Vec::new();

    schema_chunks.push(serde_json::json!({
        "schemas": {
            "HealthResponse": {
                    "type": "object",
                    "properties": {
                        "status": {"type": "string"},
                        "version": {"type": "string"}
                    }
                },
                "HealthDeepResponse": {
                    "type": "object",
                    "properties": {
                        "status": {"type": "string", "description": "ok | degraded"},
                        "version": {"type": "string"},
                        "components": {
                            "type": "object",
                            "description": "Per-component status map; keys: session_db, shared_http, kernel",
                            "additionalProperties": {
                                "type": "object",
                                "properties": {
                                    "status": {"type": "string", "description": "ok | degraded | absent"},
                                    "path": {"type": "string", "description": "Path or other diagnostic info (optional)"}
                                }
                            }
                        }
                    }
                },

                "ExecuteRequest": {
                    "type": "object",
                    "required": ["tool_id"],
                    "properties": {
                        "tool_id": {"type": "string"},
                        "args_json": {"type": "string"},
                        "fuel_limit": {"type": "integer"}
                    }
                },
                "ExecuteResponse": {
                    "type": "object",
                    "properties": {
                        "status": {"type": "string"},
                        "result_json": {"type": "string"}
                    }
                },

                "GoalExecuteRequest": {
                    "type": "object",
                    "required": ["goal", "sop_id"],
                    "properties": {
                        "goal": {"type": "string"},
                        "sop_id": {"type": "string"},
                        "project_id": {"type": "string"},
                        "event_tx": {"type": "boolean"}
                    }
                },
                "GoalExecuteResponse": {
                    "type": "object",
                    "properties": {
                        "output": {"$ref": "#/components/schemas/NodeOutput"},
                        "report": {"type": "string"}
                    }
                },
                "NodeOutput": {
                    "type": "object",
                    "description": "Generic node execution output",
                    "properties": {
                        "status": {"type": "string"},
                        "result": {"description": "Arbitrary JSON result"}
                    }
                },
                "GoalStatusResponse": {
                    "type": "object",
                    "properties": {
                        "status": {"type": "string"},
                        "current_node": {"type": "string"},
                        "progress": {"type": "number"},
                        "thought_logs": {"type": "array", "items": {"type": "string"}},
                        "last_updated": {"type": "integer"}
                    }
                },
                "TraceEventDetail": {
                    "type": "object",
                    "properties": {
                        "node_id": {"type": "string"},
                        "status": {"type": "string"},
                        "output": {"type": "string"},
                        "fuel_consumed": {"type": "integer"},
                        "duration_ms": {"type": "integer"}
                    }
                },
                "GoalTraceResponse": {
                    "type": "object",
                    "properties": {
                        "sop_id": {"type": "string"},
                        "events": {"type": "array", "items": {"$ref": "#/components/schemas/TraceEventDetail"}}
                    }
                },

                "CognitiveTraceEntry": {
                    "type": "object",
                    "description": "A single recorded cognitive decision (explainability primitive)",
                    "properties": {
                        "timestamp_ms": {"type": "integer", "description": "Epoch milliseconds when the decision was recorded"},
                        "sop_id": {"type": "string", "description": "SOP the decision belongs to"},
                        "event": {"$ref": "#/components/schemas/ExecutionEvent"}
                    }
                },
                "ExecutionEvent": {
                    "type": "object",
                    "description": "One cognitive execution event (serde externally tagged). Variants: NodeStarted | NodeCompleted | NodeFailed | ToolCall | TokenUsage | ExecutionComplete | CognitivePlan | CognitiveStep | MicroReflection | CognitiveComplete.",
                    "properties": {
                        "ToolCall": {
                            "type": "object",
                            "description": "Tool invocation progress: tool_id, phase (started|completed|failed), duration_ms (optional), success (optional), error (optional)"
                        },
                        "TokenUsage": {
                            "type": "object",
                            "description": "LLM token usage: model (optional), prompt_tokens, completion_tokens, total_tokens"
                        },
                        "CognitiveStep": {
                            "type": "object",
                            "description": "Loop phase update with metrics: step, message, metrics (iteration, deviation, confidence, progress, token_cost, stability, info_gain)"
                        },
                        "MicroReflection": {
                            "type": "object",
                            "description": "Prediction vs actual per node: node_name, prediction, actual, decision, updates, reason"
                        }
                    }
                },
                "CognitiveTraceResponse": {
                    "type": "object",
                    "properties": {
                        "entries": {
                            "type": "array",
                            "items": {"$ref": "#/components/schemas/CognitiveTraceEntry"},
                            "description": "Recent cognitive decisions, newest first"
                        }
                    }
                },
                "CognitiveTraceSummary": {
                    "type": "object",
                    "description": "Cross-session health summary aggregated from persisted cognitive decisions",
                    "properties": {
                        "total_decisions": {"type": "integer", "description": "Records aggregated"},
                        "step_count": {"type": "integer", "description": "CognitiveStep count"},
                        "reflection_count": {"type": "integer", "description": "MicroReflection count"},
                        "avg_deviation": {"type": "number", "format": "double", "description": "Mean deviation across metric-bearing steps"},
                        "avg_confidence": {"type": "number", "format": "double", "description": "Mean confidence"},
                        "avg_stability": {"type": "number", "format": "double", "description": "Mean stability"},
                        "avg_info_gain": {"type": "number", "format": "double", "description": "Mean information gain"},
                        "total_token_cost": {"type": "integer", "description": "Cumulative token cost"},
                        "decision_counts": {"type": "object", "additionalProperties": {"type": "integer"}, "description": "Micro-reflection decision distribution (CONTINUE/ADJUST/PIVOT -> count)"},
                        "first_timestamp_ms": {"type": "integer", "description": "Earliest record in window (epoch ms)"},
                        "last_timestamp_ms": {"type": "integer", "description": "Newest record in window (epoch ms)"}
                    }
                },

                "CompositeExecuteRequest": {
                    "type": "object",
                    "required": ["goals"],
                    "properties": {
                        "goals": {"type": "array", "items": {"type": "string"}},
                        "project_id": {"type": "string"},
                        "event_tx": {"type": "boolean"}
                    }
                },
                "ErrorResponse": {
                    "type": "object",
                    "description": "Uniform error envelope returned by REST endpoints",
                    "properties": {
                        "error": {"type": "string"},
                        "hint": {"type": "string", "description": "Optional actionable hint for operators/clients"}
                    }
                },

                "BroadcastEventRequest": {
                    "type": "object",
                    "description": "Broadcast a custom event. Top-level sop_id / session_id / project_id declare the event's ownership and are written into the event payload, taking precedence over same-named keys inside payload. Blank values count as absent.",
                    "required": ["event_type", "payload"],
                    "properties": {
                        "event_type": {"type": "string", "description": "Event type delivered as the SSE event name"},
                        "payload": {"description": "Arbitrary JSON payload; non-object payloads are broadcast unchanged"},
                        "sop_id": {"type": "string", "description": "SOP this event belongs to"},
                        "session_id": {"type": "string", "description": "Session this event belongs to"},
                        "project_id": {"type": "string", "description": "Project this event belongs to"}
                    }
                },

                "CompositeExecuteResponse": {
                    "type": "object",
                    "properties": {
                        "results": {
                            "type": "object",
                            "additionalProperties": {"$ref": "#/components/schemas/NodeOutput"},
                            "description": "Map from goal to its output"
                        },
                        "execution_order": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Order goals were executed in"
                        },
                        "sse_sop_id": {
                            "type": "string",
                            "description": "SSE subscription id for this composite run (composite-<uuid>). Individual graph sop_ids are generated inside the kernel and not visible here, so clients must pass this value as ?sop_id= to /api/events to receive node events."
                        }
                    }
                },

                "CompositeExecuteErrorResponse": {
                    "type": "object",
                    "description": "Failure envelope for composite execution. Carries sse_sop_id in addition to the usual error message because the run's node events were already broadcast before the failure was reported; without this id a client that saw those events could not subscribe to them (composite's sop_id is server-generated and not derivable by the client, unlike execute_goal whose sop_id the client supplies).",
                    "required": ["error"],
                    "properties": {
                        "error": {"type": "string"},
                        "hint": {"type": "string", "description": "Optional actionable hint for operators/clients"},
                        "sse_sop_id": {"type": "string", "description": "Same composite-<uuid> value the 200 response would carry; use it as ?sop_id= on /api/events"}
                    }
                },

                "SessionSummary": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "name": {"type": "string"},
                        "message_count": {"type": "integer"},
                        "created_at": {"type": "string"},
                        "updated_at": {"type": "string"}
                    }
                },
                "SessionDetail": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "name": {"type": "string"},
                        "message_count": {"type": "integer"},
                        "created_at": {"type": "string"},
                        "updated_at": {"type": "string"},
                        "messages": {
                            "type": "array",
                            "items": {"$ref": "#/components/schemas/MessageDto"}
                        }
                    }
                },
                "MessageDto": {
                    "type": "object",
                    "properties": {
                        "role": {"type": "string"},
                        "content": {"type": "string"},
                        "created_at": {"type": "string"},
                        "estimated_tokens": {"type": "integer"}
                    }
                },
                "CreateSessionRequest": {
                    "type": "object",
                    "required": ["name"],
                    "properties": {"name": {"type": "string"}}
                },
                "AddMessageRequest": {
                    "type": "object",
                    "required": ["role", "content"],
                    "properties": {
                        "role": {"type": "string"},
                        "content": {"type": "string"}
                    }
                },

                "ToolEntry": {
                    "type": "object",
                    "properties": {
                        "tool_id": {"type": "string"},
                        "name": {"type": "string"},
                        "description": {"type": "string"},
                        "source": {"type": "string"}
                    }
                },
                "AgentEntry": {
                    "type": "object",
                    "properties": {
                        "agent_id": {"type": "string"},
                        "display_name": {"type": "string"},
                        "description": {"type": "string"},
                        "tool_prefixes": {"type": "array", "items": {"type": "string"}},
                        "max_concurrency": {"type": "integer"}
                    }
                },

                "SkillEntry": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "description": {"type": "string"},
                        "version": {"type": "string"},
                        "author": {"type": "string"},
                        "tags": {"type": "array", "items": {"type": "string"}},
                        "source": {"type": "string"}
                    }
                },
                "RemoteSkillEntry": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "metadata": {"$ref": "#/components/schemas/SkillMetadata"}
                    }
                },
                "SkillMetadata": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "description": {"type": "string"},
                        "version": {"type": "string"},
                        "author": {"type": "string"},
                        "tags": {"type": "array", "items": {"type": "string"}}
                    }
                },
                // ── Skills ───────────────────────────────────────────────────
        }
    }));

    schema_chunks.push(serde_json::json!({
        "schemas": {
            "InstallSkillRequest": {
                    "type": "object",
                    "required": ["skill_id"],
                    "properties": {"skill_id": {"type": "string"}}
                },

                "WorkspaceEntry": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "name": {"type": "string"},
                        "project_id": {"type": "string"},
                        "root_dir": {"type": "string"}
                    }
                },
                // ── Workspaces ───────────────────────────────────────────────
        }
    }));

    schema_chunks.push(serde_json::json!({
        "schemas": {
            "CreateWorkspaceRequest": {
                    "type": "object",
                    "required": ["name", "project_id", "root_dir"],
                    "properties": {
                        "name": {"type": "string"},
                        "project_id": {"type": "string"},
                        "root_dir": {"type": "string"}
                    }
                },
                "SwitchWorkspaceRequest": {
                    "type": "object",
                    "required": ["id"],
                    "properties": {"id": {"type": "string"}}
                },

                "SopSaveRequest": {
                    "type": "object",
                    "required": ["id", "name", "config"],
                    "properties": {
                        "id": {"type": "string"},
                        "name": {"type": "string"},
                        "version": {"type": "string"},
                        "config": {"$ref": "#/components/schemas/SOPGraphConfig"}
                    }
                },
                "SOPGraphConfig": {
                    "type": "object",
                    "description": "DAG configuration for an SOP",
                    "properties": {
                        "nodes": {"type": "array", "items": {"type": "object"}},
                        "edges": {"type": "array", "items": {"type": "object"}},
                        "metadata": {"type": "object"}
                    }
                },
                "SopDetailResponse": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "name": {"type": "string"},
                        "version": {"type": "string"},
                        "config": {"$ref": "#/components/schemas/SOPGraphConfig"}
                    }
                },

                "CogMapResponse": {
                    "type": "object",
                    "properties": {
                        "map": {"description": "Cognitive map object or null"},
                        "status": {"type": "string"}
                    }
                },

                "StateEntry": {"type": "object", "required": ["key", "value"], "properties": {"key": {"type": "string"}, "value": {}}},
            "ExperienceQueryRequest": {
                    "type": "object",
                    "required": ["query_text"],
                    "properties": {
                        "query_text": {"type": "string"},
                        "action_id": {"type": "string"},
                        "limit": {"type": "integer"}
                    }
                },
                "ExperienceSimilarResponse": {
                    "type": "object",
                    "properties": {
                        "experiences": {
                            "type": "array",
                            "items": {"type": "object"}
                        }
                    }
                },
                "ShareExperienceRequest": {
                    "type": "object",
                    "required": ["exp_key"],
                    "properties": {
                        "exp_key": {"type": "string"},
                        "project_id": {"type": "string"}
                    }
                },
                "ShareExperienceResponse": {
                    "type": "object",
                    "properties": {
                        "success": {"type": "boolean"},
                        "error": {"type": "string"}
                    }
                },
                "SharedExperiencesResponse": {
                    "type": "object",
                    "properties": {
                        "experiences": {
                            "type": "array",
                            "items": {"type": "object"}
                        }
                    }
                },
                "SkillSearchResponse": {
                    "type": "object",
                    "properties": {
                        "results": {
                            "type": "array",
                            "items": {"type": "object"}
                        }
                    }
                },
                "PublishSkillRequest": {
                    "type": "object",
                    "required": ["registry_url", "skill_id"],
                    "properties": {
                        "registry_url": {"type": "string"},
                        "skill_id": {"type": "string"}
                    }
                },
                "PublishSkillResponse": {
                    "type": "object",
                    "properties": {
                        "success": {"type": "boolean"},
                        "entry": {"type": "object"},
                        "error": {"type": "string"}
                    }
                },
                "PeerSkillListResponse": {
                    "type": "object",
                    "properties": {
                        "skills": {
                            "type": "array",
                            "items": {"type": "object"}
                        }
                    }
                },
                "SkillBundle": {
                    "type": "object",
                    "properties": {
                        "metadata": {"type": "object"},
                        "required_tools": {
                            "type": "array",
                            "items": {"type": "string"}
                        }
                    }
                },

                // ── Roadmap ──────────────────────────────────────────────────
        }
    }));

    schema_chunks.push(serde_json::json!({
        "schemas": {
            "CreateProjectRequest": {
                    "type": "object",
                    "required": ["id", "name", "description"],
                    "properties": {
                        "id": {"type": "string"},
                        "name": {"type": "string"},
                        "description": {"type": "string"}
                    }
                },
                "AddMilestoneRequest": {
                    "type": "object",
                    "required": ["project_id", "milestone"],
                    "properties": {
                        "project_id": {"type": "string"},
                        "milestone": {"$ref": "#/components/schemas/Milestone"}
                    }
                },
                "Milestone": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "title": {"type": "string"},
                        "description": {"type": "string"},
                        "status": {"type": "string"},
                        "phases": {"type": "array", "items": {"$ref": "#/components/schemas/Phase"}}
                    }
                },
                "AddPhaseRequest": {
                    "type": "object",
                    "required": ["project_id", "milestone_id", "phase"],
                    "properties": {
                        "project_id": {"type": "string"},
                        "milestone_id": {"type": "string"},
                        "phase": {"$ref": "#/components/schemas/Phase"}
                    }
                },
                "Phase": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "title": {"type": "string"},
                        "description": {"type": "string"},
                        "goals": {"type": "array", "items": {"$ref": "#/components/schemas/GoalRef"}}
                    }
                },
                "UpdateGoalStatusRequest": {
                    "type": "object",
                    "required": ["project_id", "milestone_id", "phase_id", "status"],
                    "properties": {
                        "project_id": {"type": "string"},
                        "milestone_id": {"type": "string"},
                        "phase_id": {"type": "string"},
                        "status": {"type": "string"}
                    }
                },
                "GoalRef": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "description": {"type": "string"},
                        "status": {"type": "string"}
                    }
                },

                // ── Cognitive Map Live Intervention ──────────────────────────

                "InterventionRequest": {
                    "type": "object",
                    "required": ["action"],
                    "properties": {
                        "action": {"type": "string", "description": "abort | pause | resume | redirect; matched case-insensitively. Unrecognised values are broadcast but never applied."},
                        "target_node": {"type": "string", "description": "Node to jump to; required for the redirect action, ignored otherwise"},
                        "reason": {"type": "string", "description": "Free-form reason recorded in the broadcast intervention event"}
                    }
                },
                "InterventionResponse": {
                    "type": "object",
                    "description": "Outcome of a live intervention request. applied=false means the SOP had no entry in active_executions (it may have already finished) but the intervention event was still broadcast.",
                    "properties": {
                        "sop_id": {"type": "string"},
                        "action": {"type": "string", "description": "Action as lower-cased by the handler"},
                        "applied": {"type": "boolean", "description": "Whether the action mutated the active execution"},
                        "message": {"type": "string"}
                    }
                },

                // ── Tool Execution (by tool id) ─────────────────────────────

                "ExecuteToolRequest": {
                    "type": "object",
                    "required": ["tool_id", "args_json"],
                    "properties": {
                        "tool_id": {"type": "string"},
                        "args_json": {"type": "string", "description": "Raw JSON string passed to the tool as its argument bytes"},
                        "fuel_limit": {"type": "integer", "format": "int64", "description": "Fuel ceiling for the tool call; falls back to the bridge default when omitted"}
                    }
                },
                "ExecuteToolResponse": {
                    "type": "object",
                    "properties": {
                        "status": {"type": "string", "description": "SUCCESS on 200; on failure the status field carries \"ERROR: <message>\""},
                        "result_json": {"type": "string", "description": "Raw tool output as a UTF-8 string (lossy); empty on failure"}
                    }
                },

                // ── Skill Detail ───────────────────────────────────────────

                "SkillDetailResponse": {
                    "type": "object",
                    "properties": {
                        "found": {"type": "boolean"},
                        "skill": {"$ref": "#/components/schemas/SkillMetadata"}
                    }
                },

                // ── Workspace Deletion ─────────────────────────────────────

                "DeleteWorkspaceResponse": {
                    "type": "object",
                    "properties": {
                        "success": {"type": "boolean"},
                        "error": {"type": "string"}
                    }
                },

                // ── Feedback & Experience Capture ───────────────────────────

                "SubmitFeedbackRequest": {
                    "type": "object",
                    "required": ["feedback_json"],
                    "properties": {
                        "feedback_json": {"type": "string", "description": "JSON-encoded ExecutionFeedback"},
                        "project_id": {"type": "string", "description": "Target project; the x-project-id header takes precedence, \"default\" is used when neither is set"}
                    }
                },
                "SubmitFeedbackResponse": {
                    "type": "object",
                    "properties": {
                        "success": {"type": "boolean"},
                        "error": {"type": "string"},
                        "evolution_json": {"type": "string", "description": "JSON-encoded evolution record produced from the feedback; empty on failure"}
                    }
                },
                "CaptureExperienceRequest": {
                    "type": "object",
                    "required": ["plan_id", "plan_json"],
                    "properties": {
                        "plan_id": {"type": "string", "description": "Storage key the plan is captured under"},
                        "plan_json": {"type": "string", "description": "JSON-encoded CognitivePlan"},
                        "project_id": {"type": "string", "description": "Target project; the x-project-id header takes precedence, \"default\" is used when neither is set"}
                    }
                },
                "CaptureExperienceResponse": {
                    "type": "object",
                    "properties": {
                        "success": {"type": "boolean"},
                        "error": {"type": "string"}
                    }
                },

                // ── Roadmap Prediction & Resource Optimization ──────────────

                }
    }));

    schema_chunks.push(serde_json::json!({
        "schemas": {
            "PredictCompletionRequest": {
                    "type": "object",
                    "required": ["project_id"],
                    "properties": {"project_id": {"type": "string"}}
                },
                "MilestoneTimePrediction": {
                    "type": "object",
                    "description": "Per-milestone completion forecast with a 95% confidence interval",
                    "properties": {
                        "milestone_id": {"type": "string"},
                        "name": {"type": "string"},
                        "estimated_remaining_secs": {"type": "number", "format": "double"},
                        "ci_lower_secs": {"type": "number", "format": "double", "description": "95% confidence interval lower bound (secs)"},
                        "ci_upper_secs": {"type": "number", "format": "double", "description": "95% confidence interval upper bound (secs)"},
                        "progress_pct": {"type": "number", "format": "double"},
                        "remaining_goals": {"type": "integer"},
                        "risk_level": {"type": "string", "description": "low | medium | high | done"},
                        "velocity_goals_per_sec": {"type": "number", "format": "double", "description": "Historical throughput in goals/sec"}
                    }
                },
                "CompletionTimePrediction": {
                    "type": "object",
                    "description": "Project-level completion forecast derived from historical execution velocity",
                    "properties": {
                        "project_id": {"type": "string"},
                        "project_name": {"type": "string"},
                        "overall_progress_pct": {"type": "number", "format": "double"},
                        "remaining_goals": {"type": "integer"},
                        "estimated_total_remaining_secs": {"type": "number", "format": "double"},
                        "ci_lower_total_secs": {"type": "number", "format": "double", "description": "95% confidence interval lower bound (secs)"},
                        "ci_upper_total_secs": {"type": "number", "format": "double", "description": "95% confidence interval upper bound (secs)"},
                        "milestone_predictions": {"type": "array", "items": {"$ref": "#/components/schemas/MilestoneTimePrediction"}},
                        "estimated_completion_order": {"type": "array", "items": {"type": "string"}, "description": "Milestone ids in predicted completion order"},
                        "risk_level": {"type": "string", "description": "low | medium | high | done"},
                        "overall_velocity_goals_per_sec": {"type": "number", "format": "double"},
                        "has_sufficient_data": {"type": "boolean", "description": "False when history was too sparse for the projection to be meaningful"}
                    }
                },
                "ProjectCostsRequest": {
                    "type": "object",
                    "required": ["project_id", "total_goals", "completed_goals"],
                    "properties": {
                        "project_id": {"type": "string"},
                        "total_goals": {"type": "integer", "description": "Total goals planned for the project"},
                        "completed_goals": {"type": "integer", "description": "Goals already completed"}
                    }
                },
                "CostProjection": {
                    "type": "object",
                    "description": "Extrapolated remaining cost and duration for a project",
                    "properties": {
                        "total_goals": {"type": "integer"},
                        "completed_goals": {"type": "integer"},
                        "remaining_goals": {"type": "integer"},
                        "avg_cost_per_goal": {"type": "number", "format": "double"},
                        "avg_duration_per_goal_secs": {"type": "number", "format": "double"},
                        "projected_remaining_cost": {"type": "number", "format": "double"},
                        "projected_remaining_duration_secs": {"type": "number", "format": "double"},
                        "confidence": {"type": "number", "format": "double", "description": "Confidence in the projection, 0.0-1.0"}
                    }
                },
                "FindBottlenecksRequest": {
                    "type": "object",
                    "required": ["project_id"],
                    "properties": {"project_id": {"type": "string"}}
                },
                "Bottleneck": {
                    "type": "object",
                    "description": "A goal identified as a bottleneck in the execution pipeline",
                    "properties": {
                        "goal_id": {"type": "string"},
                        "description": {"type": "string"},
                        "failure_count": {"type": "integer"},
                        "avg_duration_secs": {"type": "number", "format": "double"},
                        "blocking_count": {"type": "integer", "description": "How many other goals this one blocked"},
                        "severity": {"type": "string", "description": "low | medium | high"}
                    }
                },
                "FindBottlenecksResponse": {
                    "type": "object",
                    "properties": {
                        "bottlenecks": {"type": "array", "items": {"$ref": "#/components/schemas/Bottleneck"}}
                    }
                },

                // ── Benchmarks / Breakers / Learning Insights / Anomaly ────

                "BenchmarkEntry": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "duration_secs": {"type": "number", "format": "double"},
                        "timestamp": {"type": "integer", "format": "int64", "description": "Epoch seconds"},
                        "success": {"type": "boolean"},
                        "metrics_json": {"type": "string", "description": "JSON-encoded metric map"}
                    }
                },
                "ListBenchmarksResponse": {
                    "type": "object",
                    "properties": {
                        "benchmarks": {"type": "array", "items": {"$ref": "#/components/schemas/BenchmarkEntry"}, "description": "Recent runs across all benchmark names, flattened"}
                    }
                },
                "BreakerStatus": {
                    "type": "object",
                    "properties": {
                        "model_name": {"type": "string"},
                        "state": {"type": "string"},
                        "failure_count": {"type": "integer", "format": "int32"},
                        "success_count": {"type": "integer", "format": "int32"}
                    }
                },
                "BreakerMetricsResponse": {
                    "type": "object",
                    "properties": {
                        "breakers": {"type": "array", "items": {"$ref": "#/components/schemas/BreakerStatus"}, "description": "Currently always empty; breaker state lives behind the LlmClient trait object"}
                    }
                },
                "StrategyInsight": {
                    "type": "object",
                    "properties": {
                        "strategy": {"type": "string"},
                        "total_uses": {"type": "integer", "format": "int64"},
                        "successes": {"type": "integer", "format": "int64"},
                        "failures": {"type": "integer", "format": "int64"},
                        "avg_duration_secs": {"type": "number", "format": "double"},
                        "success_rate": {"type": "number", "format": "double"}
                    }
                },
                "LearningInsightsResponse": {
                    "type": "object",
                    "properties": {
                        "total_samples": {"type": "integer", "format": "int64"},
                        "overall_success_rate": {"type": "number", "format": "double"},
                        "strategies": {"type": "array", "items": {"$ref": "#/components/schemas/StrategyInsight"}}
                    }
                },
                "AnomalyReport": {
                    "type": "object",
                    "properties": {
                        "node_name": {"type": "string"},
                        "severity": {"type": "string"},
                        "metric": {"type": "string"},
                        "observed": {"type": "number", "format": "double"},
                        "expected": {"type": "number", "format": "double"},
                        "description": {"type": "string"},
                        "timestamp": {"type": "integer", "format": "int64", "description": "Epoch seconds"}
                    }
                },

                // ── Plugins ────────────────────────────────────────────────

                "PluginDescriptor": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "version": {"type": "string"},
                        "wasm_path": {"type": "string"},
                        "tool_id": {"type": "string"},
                        "fuel_limit": {"type": "integer", "format": "int64"},
                        "checksum": {"type": "string", "description": "Empty when the plugin was loaded without a checksum"}
                    }
                },
                "LoadPluginRequest": {
                    "type": "object",
                    "required": ["name", "version", "wasm_path", "tool_id"],
                    "properties": {
                        "name": {"type": "string"},
                        "version": {"type": "string"},
                        "wasm_path": {"type": "string", "description": "Filesystem path to the plugin's .wasm module"},
                        "tool_id": {"type": "string", "description": "Tool id the plugin is exposed under"},
                        "fuel_limit": {"type": "integer", "format": "int64", "description": "Fuel ceiling for plugin execution; defaults to 0 when omitted"}
                    }
                },
                "LoadPluginResponse": {
                    "type": "object",
                    "properties": {
                        "success": {"type": "boolean"},
                        "error": {"type": "string"}
                    }
                },
                "UnloadPluginRequest": {
                    "type": "object",
                    "required": ["name"],
                    "properties": {"name": {"type": "string"}}
                },
                "UnloadPluginResponse": {
                    "type": "object",
                    "properties": {
                        "success": {"type": "boolean"},
                        "error": {"type": "string"}
                    }
                },

                // ── JCode Delegation ───────────────────────────────────────

                "CallJcodeRequest": {
                    "type": "object",
                    "required": ["task_id", "goal", "context_json"],
                    "properties": {
                        "task_id": {"type": "string"},
                        "goal": {"type": "string"},
                        "context_json": {"type": "string", "description": "Context forwarded verbatim as the `context` field of the jcode/call payload"}
                    }
                },
                "CallJcodeResponse": {
                    "type": "object",
                    "properties": {
                        "success": {"type": "boolean"},
                        "result": {"type": "string", "description": "Raw JCode tool output as a UTF-8 string; empty on failure"},
                        "error": {"type": "string"}
                    }
                },

                // ── Chaos Engineering ──────────────────────────────────────

                "ChaosStartRequest": {
                    "type": "object",
                    "required": ["name", "fault_types", "fault_probability", "duration_seconds", "target_operations"],
                    "properties": {
                        "name": {"type": "string"},
                        "fault_types": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "LlmTimeout | LlmGarbage | ToolCrash | ToolHang | StorageWriteFailure | StorageReadFailure | NetworkPartition | RandomDelay(<ms>); unrecognised entries are dropped"
                        },
                        "fault_probability": {"type": "number", "format": "double", "description": "Per-operation probability of injecting a fault, 0.0-1.0"},
                        "duration_seconds": {"type": "integer", "format": "int64"},
                        "target_operations": {"type": "array", "items": {"type": "string"}, "description": "Operations to fault; an empty list targets all operations"}
                    }
                },
                "ChaosStartResponse": {
                    "type": "object",
                    "properties": {
                        "success": {"type": "boolean"},
                        "experiment_id": {"type": "string", "description": "Handler-generated UUID; not tracked by the chaos runner, so /api/chaos/status ignores it"},
                        "error": {"type": "string"}
                    }
                },
                "ChaosStopRequest": {
                    "type": "object",
                    "required": ["experiment_id"],
                    "properties": {
                        "experiment_id": {"type": "string", "description": "Accepted but unused; the runner stops whichever experiment is currently active"}
                    }
                },
                "ChaosExperimentResult": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "operations_attempted": {"type": "integer", "format": "int64"},
                        "operations_succeeded": {"type": "integer", "format": "int64"},
                        "operations_failed": {"type": "integer", "format": "int64"},
                        "faults_injected": {"type": "integer", "format": "int64"},
                        "success_rate": {"type": "number", "format": "double"}
                    }
                },
                "ChaosStopResponse": {
                    "type": "object",
                    "properties": {
                        "success": {"type": "boolean"},
                        "result": {"$ref": "#/components/schemas/ChaosExperimentResult"},
                        "error": {"type": "string"}
                    }
                },
                "ChaosStatusResponse": {
                    "type": "object",
                    "properties": {
                        "running": {"type": "boolean"},
                        "result": {"$ref": "#/components/schemas/ChaosExperimentResult", "description": "Placeholder counters while an experiment is active"}
                    }
                }
        }
    }));

    let mut all_schemas = serde_json::Map::new();
    for chunk in schema_chunks {
        flatten(chunk, "schemas", &mut all_schemas)?;
    }

    Ok(serde_json::json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Sisyphus Bridge REST API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "REST API for the Sisyphus Cognitive Engine. Every response carries an `x-trace-id` header: the value is echoed from the request when present and valid, otherwise generated. Use it to correlate the request with its tool/LLM log lines and SSE events (event payload key `trace_id`)."
        },
        "servers": [{"url": "http://localhost:8080", "description": "Local dev"}],
        "paths": serde_json::Value::Object(all_paths),
        "components": {
            "schemas": serde_json::Value::Object(all_schemas),
            // 与 `http/mod.rs` 的 bearer/JWT 中间件对应；`security` 引用必须可解析。
            "securitySchemes": {
                "bearerAuth": {
                    "type": "http",
                    "scheme": "bearer",
                    "bearerFormat": "JWT or static token"
                }
            }
        }
    }))
}

pub(crate) async fn swagger_ui_handler() -> impl IntoResponse {
    let html = r#"<!DOCTYPE html>
<html>
<head>
  <title>Sisyphus Bridge API</title>
  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui.css" />
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
  <script>
    SwaggerUIBundle({
      url: '/openapi.json',
      dom_id: '#swagger-ui',
    });
  </script>
</body>
</html>"#;
    (
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// 收集文档里出现的所有 `#/components/schemas/X` 引用名
    fn collect_refs(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(m) => {
                for (k, val) in m {
                    if k == "$ref" {
                        if let Some(name) = val.as_str().and_then(|s| s.rsplit('/').next()) {
                            out.push(name.to_string());
                        }
                    } else {
                        collect_refs(val, out);
                    }
                }
            },
            Value::Array(a) => {
                for x in a {
                    collect_refs(x, out);
                }
            },
            _ => {},
        }
    }

    /// 走真实 handler 取出文档 —— 与客户端拿到的完全一致。
    /// 直接读源码里的 json! 字面量会漏掉 handler 层可能做的加工。
    async fn doc() -> Value {
        let resp = openapi_json().await.into_response();
        assert_eq!(resp.status(), 200);
        let body = axum::body::to_bytes(resp.into_body(), 1 << 20).await.expect("应能读取响应体");
        serde_json::from_slice(&body).expect("OpenAPI 文档必须是合法 JSON")
    }

    /// 每个 `$ref` 都必须指向真实存在的 schema。
    ///
    /// 悬空引用不会让 `/openapi.json` 返回错误 —— 它照常返回 200，
    /// Swagger UI 却在渲染时才报错，开发者看到的是空白页。
    /// 没有这个测试，悬空引用会一直潜伏。
    #[tokio::test]
    async fn every_schema_ref_resolves() {
        let doc = doc().await;
        let schemas = doc
            .pointer("/components/schemas")
            .and_then(Value::as_object)
            .expect("必须存在 components.schemas");

        let mut refs = Vec::new();
        collect_refs(&doc, &mut refs);
        assert!(!refs.is_empty(), "应至少存在一些 $ref 引用");

        let dangling: Vec<_> =
            refs.iter().filter(|name| !schemas.contains_key(name.as_str())).collect();
        assert!(
            dangling.is_empty(),
            "以下 $ref 指向不存在的 schema（Swagger UI 会渲染失败）: {dangling:?}"
        );
    }

    /// 每个 `security` 引用都必须指向已定义的 security scheme。
    ///
    /// 未定义的 scheme 不会让 `/openapi.json` 报错，但 Swagger UI 无法正确渲染
    /// 鉴权入口，且文档与 `http/mod.rs` 的鉴权中间件容易脱节。
    #[tokio::test]
    async fn security_requirements_reference_defined_schemes() {
        let doc = doc().await;
        let schemes = doc
            .pointer("/components/securitySchemes")
            .and_then(Value::as_object)
            .expect("必须存在 components.securitySchemes");
        let paths = doc.pointer("/paths").and_then(Value::as_object).expect("paths");

        let mut missing = Vec::new();
        for item in paths.values() {
            let Some(ops) = item.as_object() else {
                continue;
            };
            for op in ops.values() {
                let Some(reqs) = op.get("security").and_then(Value::as_array) else {
                    continue;
                };
                for req in reqs {
                    let Some(req) = req.as_object() else { continue };
                    for name in req.keys() {
                        if !schemes.contains_key(name) {
                            missing.push(name.clone());
                        }
                    }
                }
            }
        }
        assert!(
            missing.is_empty(),
            "以下 security scheme 未在 components.securitySchemes 中定义: {missing:?}"
        );
    }

    /// 每个 path 必须有至少一个操作，且 operationId 全局唯一。
    #[tokio::test]
    async fn operation_ids_are_unique() {
        let doc = doc().await;
        let paths = doc.pointer("/paths").and_then(Value::as_object).expect("paths");

        let mut seen = std::collections::HashSet::new();
        for (path, item) in paths {
            for (method, op) in item.as_object().expect("path 项应为对象") {
                if !matches!(method.as_str(), "get" | "post" | "put" | "delete" | "patch") {
                    continue;
                }
                let id = op
                    .get("operationId")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| panic!("{method} {path} 缺少 operationId"));
                assert!(
                    seen.insert(id.to_string()),
                    "operationId 重复: {id}（OpenAPI 要求全局唯一，客户端代码生成会冲突）"
                );
            }
        }
    }

    /// 把 axum 的路径参数语法转为 OpenAPI 的
    ///
    /// `/api/sessions/:id` → `/api/sessions/{id}`
    fn normalize_axum_path(path: &str) -> String {
        let mut out = String::with_capacity(path.len() + 4);
        let mut chars = path.chars().peekable();
        while let Some(c) = chars.next() {
            if c != ':' {
                out.push(c);
                continue;
            }
            let mut name = String::new();
            while let Some(&n) = chars.peek() {
                if n == '/' || n == '}' {
                    break;
                }
                name.push(n);
                chars.next();
            }
            out.push('{');
            out.push_str(&name);
            out.push('}');
        }
        out
    }

    /// OpenAPI 必须覆盖所有对外暴露的路由。
    ///
    /// 未收录的端点在 Swagger UI 里完全不可见 —— 开发者看到的是
    /// 「接口不存在」，而不是「文档漏了」。这不是文档洁癖问题：
    /// 上一轮就发现 `/api/events` 整条 SSE 链路从未进过文档。
    #[tokio::test]
    async fn every_route_is_documented() {
        use axum::routing::MethodRouter;
        let _ = std::any::type_name::<MethodRouter>();

        let doc = doc().await;
        let paths = doc.pointer("/paths").and_then(Value::as_object).expect("paths");

        // 从路由表读取真实注册的路径
        let mod_src = include_str!("http/mod.rs");

        // `.route(...)` 的实参经常跨行书写。只看单行会漏掉它们，于是这些
        // 路由既没进文档、门禁也看不见 —— 门禁静默失去覆盖。先把整段
        // `.route(` … `)` 收成一条逻辑记录再解析。
        fn methods_in(body: &str) -> Vec<String> {
            const METHODS: [&str; 6] = ["get", "post", "put", "delete", "patch", "head"];
            let mut out = Vec::new();
            // The method is the identifier immediately preceding '(' — e.g.
            // `get(list_sops_handler).post(save_sop_handler)`. Scanning inside
            // the parentheses instead would read handler names, not methods.
            for ident in body.split('(').map(|chunk| {
                chunk
                    .rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .next()
                    .unwrap_or_default()
            }) {
                if METHODS.contains(&ident.to_ascii_lowercase().as_str()) {
                    out.push(ident.to_uppercase());
                }
            }
            out
        }

        let mut routes: Vec<(String, String)> = Vec::new();
        let mut pending: Option<String> = None;
        for raw in mod_src.lines() {
            let line = raw.trim();
            let chunk = match pending.take() {
                Some(mut buf) => {
                    buf.push(' ');
                    buf.push_str(line);
                    Some(buf)
                },
                None => line.strip_prefix(".route(").map(|rest| rest.to_string()),
            };
            let Some(buf) = chunk else { continue };
            if !buf.ends_with(')') {
                pending = Some(buf);
                continue;
            }
            let Some((path, body)) = buf.split_once(',') else {
                continue;
            };
            let path = path.trim().trim_matches('"').to_string();
            if path.starts_with("/dashboard") || path.is_empty() {
                continue;
            }
            for m in methods_in(body) {
                routes.push((m, path.clone()));
            }
        }

        assert!(
            routes.len() > 30,
            "路由解析明显失败，只读到 {} 条",
            routes.len()
        );

        // OpenAPI 不描述文档自身
        const DOC_ROUTES: [&str; 2] = ["/openapi.json", "/swagger-ui/*rest"];

        let undocumented: Vec<String> = routes
            .iter()
            .filter(|(_, p)| !DOC_ROUTES.contains(&p.as_str()))
            .filter(|(method, path)| {
                // axum 路径参数 `:id` → OpenAPI 路径参数 `{id}`。
                // 必须成对替换：只把 `:` 换成 `{` 会得到 `{id` 这种残缺形式，
                // 与文档永远匹配不上，测试会误报一堆假阳性。
                let oa_path = normalize_axum_path(path);
                let m_lower = method.to_lowercase();
                !paths.contains_key(&oa_path)
                    || !paths[&oa_path]
                        .as_object()
                        .map(|op| op.contains_key(&m_lower))
                        .unwrap_or(false)
            })
            .map(|(m, p)| format!("{m} {p}"))
            .collect();

        assert!(
            undocumented.is_empty(),
            "以下路由未收录进 OpenAPI（Swagger UI 中不可见）:\n  {}",
            undocumented.join("\n  ")
        );
    }

    /// composite 的失败响应必须声明，且必须带 `sse_sop_id`。
    ///
    /// 该端点的失败路径有个特殊之处：执行期间的事件**已经广播**到 SSE，
    /// 而 `sse_sop_id` 由服务端生成、客户端无从推算（对比 `/api/execute`
    /// 的 sop_id 由客户端自己提供）。所以失败时若不在文档里声明这个字段，
    /// 客户端按 schema 生成的类型就看不到它 —— 于是「收到了事件，却不知道
    /// 该订阅哪个 sop_id」。
    ///
    /// 这里不能只靠实现侧的测试：handler 返回了字段，不代表文档承诺了它。
    /// 契约是给消费方看的，必须由文档本身来断言。
    #[tokio::test]
    async fn composite_failure_contract_documents_sse_sop_id() {
        let doc = doc().await;

        let responses = doc
            .pointer("/paths/~1api~1execute~1composite/post/responses")
            .and_then(Value::as_object)
            .expect("composite 端点应声明 responses");

        assert!(
            responses.contains_key("500"),
            "composite 端点必须声明 500 响应，实际只有 {:?}",
            responses.keys().collect::<Vec<_>>()
        );

        // 500 指向的 schema 必须含 sse_sop_id
        let schema_name = responses
            .get("500")
            .and_then(|r| r.pointer("/content/application~1json/schema/$ref"))
            .and_then(Value::as_str)
            .and_then(|r| r.rsplit('/').next())
            .expect("500 响应应通过 $ref 指向一个具名 schema");
        assert_eq!(schema_name, "CompositeExecuteErrorResponse");

        let props = doc
            .pointer("/components/schemas/CompositeExecuteErrorResponse/properties")
            .and_then(Value::as_object)
            .expect("CompositeExecuteErrorResponse 应有 properties");
        assert!(
            props.contains_key("sse_sop_id"),
            "失败响应 schema 必须声明 sse_sop_id，否则客户端看不到该字段"
        );
        assert!(props.contains_key("error"), "失败响应 schema 应声明 error");

        // 成功响应也要带同一个字段，两个响应的契约才对得上
        let ok_props = doc
            .pointer("/components/schemas/CompositeExecuteResponse/properties")
            .and_then(Value::as_object)
            .expect("CompositeExecuteResponse 应有 properties");
        assert!(
            ok_props.contains_key("sse_sop_id"),
            "成功响应 schema 应声明 sse_sop_id，与失败响应保持一致"
        );
    }
}
