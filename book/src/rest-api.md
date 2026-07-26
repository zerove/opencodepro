# REST API Reference

All endpoints are served by the webui or bridge service.

## Authentication

Include `X-API-Key` header or `Authorization: Bearer <token>`.

## Health

### `GET /health`

```json
{ "status": "ok", "version": "0.1.0", "uptime_secs": 3600 }
```

## Sessions

### `GET /api/v1/sessions`

List all sessions.

### `POST /api/v1/sessions`

Create a new session.

```json
{ "name": "my-session" }
```

### `GET /api/v1/sessions/{id}`

Get session detail with messages.

### `POST /api/v1/sessions/{id}/messages`

Add a message to a session.

```json
{ "role": "user", "content": "Hello" }
```

## Search

### `POST /api/v1/vector/search`

```json
{ "query": "search text", "top_k": 10 }
```

Response:
```json
[{ "id": "doc-1", "score": 0.95, "content": "..." }]
```

## Execution

### `POST /api/v1/execute`

```json
{ "sop_id": "sop-id", "context": {} }
```

Response:
```json
{ "sop_id": "sop-id", "status": "success", "iterations": 3, "deviation": 0.02 }
```

## SOPs

### `GET /api/v1/sops`

List available SOPs.

### `GET /api/v1/sops/{id}/dag`

Get DAG graph for a SOP.

```json
{
  "nodes": [{ "id": "n1", "label": "Start", "status": "completed", "x": 0, "y": 0 }],
  "edges": [{ "from": "n1", "to": "n2" }],
  "width": 400,
  "height": 300
}
```

## Skills

### `GET /api/v1/skills`

List available skills.

### `POST /api/v1/skills/install`

Install a skill.

```json
{ "skill_id": "skill-name" }
```

## Auth

### `POST /api/auth/validate`

Validate an API key.

```json
{ "api_key": "key-to-validate" }
```

Response:
```json
{ "valid": true, "message": "authorized" }
```

## Real-time

### `GET /events` (SSE)

Server-Sent Events stream.

### `GET /ws` (WebSocket)

Bidirectional WebSocket connection.

## Metrics

### `GET /metrics`

Prometheus-compatible metrics endpoint.

```
# HELP sisyphus_requests_total Total HTTP requests
sisyphus_requests_total 1234
# HELP sisyphus_sessions_created Total sessions created
sisyphus_sessions_created 56
# HELP sisyphus_uptime_seconds Server uptime
sisyphus_uptime_seconds 3600
```
