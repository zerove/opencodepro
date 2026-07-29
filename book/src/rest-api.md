# REST API Reference

All endpoints are served by the Sisyphus Bridge HTTP server (listening on `[::1]:8080` by default).

The OpenAPI specification is available at [`/openapi.json`](/openapi.json) or via the Swagger UI at [`/swagger-ui`](/swagger-ui).

## Authentication

Include `Authorization: Bearer <token>` header. The token is configured in `config.auth_token`.

## Health

### `GET /health`

```json
{ "status": "SERVING", "version": "0.1.0" }
```

## Tool Execution

### `POST /api/execute`

Execute a tool by tool ID.

```json
{ "tool_id": "file_read", "args_json": "{\"path\": \"/tmp/test.txt\"}", "fuel_limit": 1000000 }
```

## Goal Execution

### `POST /api/execute/goal`

Execute a cognitive goal via an SOP.

```json
{ "goal": "Analyze code", "sop_id": "code-review", "project_id": "my-project" }
```

### `GET /api/execute/goal/{goal_id}/status`

Get the current execution status.

### `GET /api/execute/goal/{goal_id}/trace`

Get the full execution trace with per-node events.

### `GET /api/execute/goal/{goal_id}/stream`

Server-Sent Events stream of execution progress.

### `POST /api/execute/composite`

Execute multiple goals as a composite DAG.

```json
{ "goals": ["analyze", "test", "deploy"], "project_id": "my-project" }
```

## Sessions

### `GET /api/sessions`

List all sessions.

### `POST /api/sessions`

Create a new session.

```json
{ "name": "my-session" }
```

### `GET /api/sessions/{id}`

Get session details with messages.

### `POST /api/sessions/{id}/messages`

Add a message to a session.

```json
{ "role": "user", "content": "Hello" }
```

## Tools & Agents

### `GET /api/tools`

List registered tools.

### `GET /api/agents`

List registered agents.

## Skills

### `GET /api/skills`

List installed skill entries.

### `GET /api/skills/available`

List available remote skills.

### `POST /api/skills/install`

Install a remote skill.

```json
{ "skill_id": "code-reviewer" }
```

### `GET /api/skills/installed`

List installed remote skills.

### `DELETE /api/skills/remote/{id}`

Uninstall a remote skill.

## Workspaces

### `GET /api/workspaces`

List all workspaces.

### `POST /api/workspaces`

Create a workspace.

```json
{ "name": "dev", "project_id": "my-project", "root_dir": "/home/user/project" }
```

### `POST /api/workspaces/switch`

Switch the active workspace.

```json
{ "id": "ws-1" }
```

### `GET /api/workspaces/current`

Get the currently active workspace.

## SOP Management

### `GET /api/sops`

List SOPs for the current workspace. Pass `X-Sisyphus-Workspace-Id` header.

### `POST /api/sops`

Save or create an SOP.

```json
{ "id": "review-sop", "name": "Code Review", "config": { "nodes": [...], "edges": [...] } }
```

### `GET /api/sops/{id}`

Get SOP details with DAG config.

### `DELETE /api/sops/{id}`

Delete an SOP.

## Cognitive Map

### `GET /api/cognitive/map/{sop_id}`

Get the cognitive map for a running SOP.

## Experience Engine

### `POST /api/experience/similar`

Query similar experiences by semantic similarity.

```json
{ "query_text": "error during build", "action_id": "compile", "limit": 5 }
```

## Roadmap

### `GET /api/roadmap/project/{id}`

Get project roadmap details.

### `POST /api/roadmap/project/{id}`

Create a new project.

```json
{ "id": "my-project", "name": "My Project", "description": "..." }
```

### `POST /api/roadmap/milestone`

Add a milestone to a project.

```json
{ "project_id": "my-project", "milestone": { "id": "m1", "title": "Alpha", "status": "pending" } }
```

### `POST /api/roadmap/phase`

Add a phase to a milestone.

```json
{ "project_id": "my-project", "milestone_id": "m1", "phase": { "id": "p1", "title": "Design" } }
```

### `PATCH /api/roadmap/goal/{goal_id}`

Update goal status.

### `GET /api/roadmap/health/{project_id}`

Evaluate project health score.
