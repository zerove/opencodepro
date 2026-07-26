# Security

## API Key Authentication

All API endpoints require authentication via:

1. **Header**: `X-API-Key: <your-key>`
2. **Bearer**: `Authorization: Bearer <your-key>`

The key is validated against the `api.key` field in configuration.

## External Directory Permissions

The `external_dir_permissions` config controls which directories the engine can access:

```toml
[tools]
external_dir_permissions = [
    { dir = "/project", write = true },
    { dir = "/tmp", write = true },
    { dir = "/home", write = false }
]
```

## Environment Variable Substitution

Config values support env var substitution:

- `{env:VAR_NAME}` - Use env var, error if missing
- `{env:VAR_NAME|default}` - Use env var with fallback
- `{env:VAR_NAME:}` - Use env var, empty string if missing

## RBAC (Role-Based Access Control)

Roles: `admin`, `user`, `viewer`, `guest`

| Role | Permissions |
|------|------------|
| admin | Full access |
| user | Execute, read, write |
| viewer | Read-only |
| guest | Health check only |

## Session Isolation

Sessions are isolated by session ID. Each session maintains:

- Separate message history
- Separate memory context
- Separate DAG execution state

## Sandbox Execution

The WASM sandbox provides process isolation for tool execution:

- Memory limits via wasmtime config
- Fuel-based instruction limits
- No filesystem access outside allowed directories
- No network access (unless explicitly granted)
