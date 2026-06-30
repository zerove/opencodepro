//! REST API for Sisyphus Bridge (axum HTTP layer alongside gRPC).
use crate::MySisyphusBridge;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

// ── Response / Request Types ───────────────────────────────────────────────

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

#[derive(Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub name: String,
    pub message_count: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Deserialize)]
pub struct SessionDetail {
    pub id: String,
    pub name: String,
    pub message_count: u64,
    pub created_at: String,
    pub updated_at: String,
    pub messages: Vec<MessageDto>,
}

#[derive(Serialize, Deserialize)]
pub struct MessageDto {
    pub role: String,
    pub content: String,
    pub created_at: String,
    pub estimated_tokens: u64,
}

#[derive(Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub name: String,
}

#[derive(Serialize, Deserialize)]
pub struct AddMessageRequest {
    pub role: String,
    pub content: String,
}

#[derive(Serialize, Deserialize)]
pub struct ExecuteRequest {
    pub tool_id: String,
    #[serde(default)]
    pub args_json: String,
    #[serde(default)]
    pub fuel_limit: u64,
}

#[derive(Serialize)]
pub struct ExecuteResponse {
    pub status: String,
    pub result_json: String,
}

#[derive(Serialize)]
pub struct ToolEntry {
    pub tool_id: String,
    pub name: String,
    pub description: String,
    pub source: String,
}

#[derive(Serialize)]
pub struct AgentEntry {
    pub agent_id: String,
    pub display_name: String,
    pub description: String,
    pub tool_prefixes: Vec<String>,
    pub max_concurrency: u32,
}

#[derive(Serialize)]
pub struct SkillEntry {
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: String,
    pub tags: Vec<String>,
    pub source: String,
}

// ── Shared State ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub(crate) struct RestApiState {
    pub bridge: Arc<MySisyphusBridge>,
    pub session_db: PathBuf,
}

// ── Handlers ──────────────────────────────────────────────────────────────

async fn health_handler(State(state): State<RestApiState>) -> impl IntoResponse {
    let _ = state.bridge.kernel();
    Json(HealthResponse {
        status: "SERVING".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

async fn execute_handler(
    State(state): State<RestApiState>,
    Json(req): Json<ExecuteRequest>,
) -> impl IntoResponse {
    let fuel = if req.fuel_limit > 0 {
        req.fuel_limit
    } else {
        state.bridge.default_fuel()
    };
    let args_bytes = req.args_json.into_bytes();
    match state
        .bridge
        .kernel()
        .tool_executor
        .execute(&req.tool_id, args_bytes, fuel)
        .await
    {
        Ok(bytes) => {
            let result_str = String::from_utf8_lossy(&bytes).to_string();
            (
                StatusCode::OK,
                Json(ExecuteResponse {
                    status: "SUCCESS".into(),
                    result_json: result_str,
                }),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ExecuteResponse {
                status: format!("ERROR: {e}"),
                result_json: String::new(),
            }),
        ),
    }
}

async fn list_sessions_handler(State(state): State<RestApiState>) -> impl IntoResponse {
    match sisyphus_tools::session_list(&state.session_db) {
        Ok(sessions) => {
            let summary: Vec<SessionSummary> = sessions
                .into_iter()
                .map(|s| SessionSummary {
                    id: s.id,
                    name: s.name,
                    message_count: s.message_count,
                    created_at: s.created_at,
                    updated_at: s.updated_at,
                })
                .collect();
            (StatusCode::OK, Json(summary))
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Vec::<SessionSummary>::new()),
        ),
    }
}

async fn create_session_handler(
    State(state): State<RestApiState>,
    Json(req): Json<CreateSessionRequest>,
) -> impl IntoResponse {
    match sisyphus_tools::session_create(&state.session_db, &req.name) {
        Ok(session) => {
            let detail = SessionDetail {
                id: session.id,
                name: session.name,
                message_count: session.message_count,
                created_at: session.created_at,
                updated_at: session.updated_at,
                messages: session
                    .messages
                    .into_iter()
                    .map(|m| MessageDto {
                        role: m.role,
                        content: m.content,
                        created_at: m.created_at,
                        estimated_tokens: m.estimated_tokens,
                    })
                    .collect(),
            };
            (StatusCode::CREATED, Json(detail))
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(SessionDetail {
                id: String::new(),
                name: String::new(),
                message_count: 0,
                created_at: String::new(),
                updated_at: String::new(),
                messages: vec![],
            }),
        ),
    }
}

async fn get_session_handler(
    State(state): State<RestApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match sisyphus_tools::session_get(&state.session_db, &id) {
        Ok(session) => {
            let detail = SessionDetail {
                id: session.id,
                name: session.name,
                message_count: session.message_count,
                created_at: session.created_at,
                updated_at: session.updated_at,
                messages: session
                    .messages
                    .into_iter()
                    .map(|m| MessageDto {
                        role: m.role,
                        content: m.content,
                        created_at: m.created_at,
                        estimated_tokens: m.estimated_tokens,
                    })
                    .collect(),
            };
            (StatusCode::OK, Json(detail))
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(SessionDetail {
                id: String::new(),
                name: String::new(),
                message_count: 0,
                created_at: String::new(),
                updated_at: String::new(),
                messages: vec![],
            }),
        ),
    }
}

async fn add_message_handler(
    State(state): State<RestApiState>,
    Path(id): Path<String>,
    Json(req): Json<AddMessageRequest>,
) -> impl IntoResponse {
    match sisyphus_tools::session_add_message(&state.session_db, &id, &req.role, &req.content) {
        Ok(session) => {
            let detail = SessionDetail {
                id: session.id,
                name: session.name,
                message_count: session.message_count,
                created_at: session.created_at,
                updated_at: session.updated_at,
                messages: session
                    .messages
                    .into_iter()
                    .map(|m| MessageDto {
                        role: m.role,
                        content: m.content,
                        created_at: m.created_at,
                        estimated_tokens: m.estimated_tokens,
                    })
                    .collect(),
            };
            (StatusCode::OK, Json(detail))
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(SessionDetail {
                id: String::new(),
                name: String::new(),
                message_count: 0,
                created_at: String::new(),
                updated_at: String::new(),
                messages: vec![],
            }),
        ),
    }
}

async fn list_tools_handler(State(state): State<RestApiState>) -> impl IntoResponse {
    let tools = state.bridge.tool_registry().list();
    let entries: Vec<ToolEntry> = tools
        .into_iter()
        .map(|(tool_id, info)| {
            let source = match &info.source {
                sisyphus_tools::ToolSource::Builtin => "builtin",
                sisyphus_tools::ToolSource::Mcp { .. } => "mcp",
                sisyphus_tools::ToolSource::Strategic => "strategic",
                sisyphus_tools::ToolSource::Wasm => "wasm",
            };
            ToolEntry {
                tool_id,
                name: info.name,
                description: info.description,
                source: source.into(),
            }
        })
        .collect();
    (StatusCode::OK, Json(entries))
}

async fn list_agents_handler(State(state): State<RestApiState>) -> impl IntoResponse {
    let agents = state.bridge.agent_registry().list();
    let entries: Vec<AgentEntry> = agents
        .into_iter()
        .map(|(agent_id, info)| {
            let description = if !info.tool_prefixes.is_empty() {
                let tools = state
                    .bridge
                    .tool_registry()
                    .list_for_prefixes(&info.tool_prefixes);
                let tool_names: Vec<&str> =
                    tools.iter().map(|(_, t)| t.name.as_str()).collect();
                if tool_names.is_empty() {
                    info.description
                } else {
                    format!("{} | Tools: {}", info.description, tool_names.join(", "))
                }
            } else {
                info.description
            };
            AgentEntry {
                agent_id,
                display_name: info.display_name,
                description,
                tool_prefixes: info.tool_prefixes,
                max_concurrency: info.max_concurrency as u32,
            }
        })
        .collect();
    (StatusCode::OK, Json(entries))
}

async fn list_skills_handler(State(state): State<RestApiState>) -> impl IntoResponse {
    let skills = state.bridge.skill_loader().load_all();
    let entries: Vec<SkillEntry> = skills
        .into_iter()
        .map(|s| SkillEntry {
            name: s.name,
            description: s.description,
            version: s.version,
            author: s.author.unwrap_or_default(),
            tags: s.tags,
            source: match s.source {
                sisyphus_tools::skill::SkillSource::BuiltIn => "builtin",
                sisyphus_tools::skill::SkillSource::User => "user",
                sisyphus_tools::skill::SkillSource::Project => "project",
            }
            .into(),
        })
        .collect();
    (StatusCode::OK, Json(entries))
}

// ── Router Builder ────────────────────────────────────────────────────────

impl RestApiState {
    pub fn new(bridge: Arc<MySisyphusBridge>, data_dir: PathBuf) -> Self {
        let session_db = data_dir.join("sessions.json");
        Self { bridge, session_db }
    }
}

/// Build the axum Router with all REST API routes.
pub(crate) fn build_router(state: RestApiState) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/api/execute", post(execute_handler))
        .route(
            "/api/sessions",
            get(list_sessions_handler).post(create_session_handler),
        )
        .route("/api/sessions/{id}", get(get_session_handler))
        .route("/api/sessions/{id}/messages", post(add_message_handler))
        .route("/api/tools", get(list_tools_handler))
        .route("/api/agents", get(list_agents_handler))
        .route("/api/skills", get(list_skills_handler))
        .route("/openapi.json", get(openapi_json))
        .route("/swagger-ui/*rest", get(swagger_ui_handler))
        .with_state(state)
}

// ── OpenAPI helpers ───────────────────────────────────────────────────────

async fn openapi_json() -> impl IntoResponse {
    Json(serde_json::json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Sisyphus Bridge REST API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "REST API for the Sisyphus Cognitive Engine"
        },
        "paths": {
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
            "/api/execute": {
                "post": {
                    "summary": "Execute a tool",
                    "operationId": "executeTool",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ExecuteRequest"}}}},
                    "responses": {
                        "200": {"description": "Execution result", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ExecuteResponse"}}}}
                    }
                }
            },
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
                    "summary": "Add message",
                    "operationId": "addMessage",
                    "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/AddMessageRequest"}}}},
                    "responses": {
                        "200": {"description": "Message added", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SessionDetail"}}}}
                    }
                }
            },
            "/api/tools": {
                "get": {
                    "summary": "List tools",
                    "operationId": "listTools",
                    "responses": {
                        "200": {"description": "Tool list", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/ToolEntry"}}}}}
                    }
                }
            },
            "/api/agents": {
                "get": {
                    "summary": "List agents",
                    "operationId": "listAgents",
                    "responses": {
                        "200": {"description": "Agent list", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/AgentEntry"}}}}}
                    }
                }
            },
            "/api/skills": {
                "get": {
                    "summary": "List skills",
                    "operationId": "listSkills",
                    "responses": {
                        "200": {"description": "Skill list", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/SkillEntry"}}}}}
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
                }
            }
        }
    }))
}

async fn swagger_ui_handler() -> impl IntoResponse {
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
