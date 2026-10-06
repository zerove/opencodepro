//! OpenAPI specification and Swagger UI for the REST API.

use axum::response::IntoResponse;
use axum::Json;

pub(crate) async fn openapi_json() -> impl IntoResponse {
    Json(serde_json::json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Sisyphus Bridge REST API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "REST API for the Sisyphus Cognitive Engine"
        },
        "servers": [{"url": "http://localhost:8080", "description": "Local dev"}],
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

            // ── Goal Execution ──────────────────────────────────────────
            "/api/execute/goal": {
                "post": {
                    "summary": "Execute a goal via SOP",
                    "operationId": "executeGoal",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/GoalExecuteRequest"}}}},
                    "responses": {
                        "200": {"description": "Goal execution result", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/GoalExecuteResponse"}}}}
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
                        "200": {"description": "Composite execution results", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CompositeExecuteResponse"}}}}
                    }
                }
            },

            // ── SSE ──────────────────────────────────────────────────────
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

            // ── Roadmap ──────────────────────────────────────────────────
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

            // ── Observability ───────────────────────────────────────────
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
            }
        },

        "components": {
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
                    "description": "One cognitive execution event (serde externally tagged). Variants: NodeStarted | NodeCompleted | NodeFailed | ExecutionComplete | CognitivePlan | CognitiveStep | MicroReflection | CognitiveComplete.",
                    "properties": {
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
                        "error": {"type": "string"}
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
        let mut routes: Vec<(String, String)> = Vec::new();
        for line in mod_src.lines() {
            let line = line.trim();
            let Some(rest) = line.strip_prefix(".route(") else {
                continue;
            };
            let Some((path, method)) = rest.split_once(',') else {
                continue;
            };
            let path = path.trim().trim_matches('"').to_string();
            // nest_service / ServeDir 不是 API 端点
            if path.starts_with("/dashboard") || path.is_empty() {
                continue;
            }
            let method =
                method.trim().split(['(', ',']).next().unwrap_or_default().trim().to_uppercase();
            routes.push((method, path));
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
}
