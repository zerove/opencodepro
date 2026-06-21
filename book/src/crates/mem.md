# sisyphus-mem — Memory & Caching

Two-level cache store for SOP execution history, intermediate results, and configuration.

## Architecture

```
┌──────────┐     ┌──────────┐
│   L1     │     │   L2     │
│ DashMap  │────▶│  SQLite  │
│ (memory) │     │  (disk)  │
└──────────┘     └──────────┘
```

- **L1**: In-memory `DashMap<String, serde_json::Value>` for sub-millisecond reads. No persistence.
- **L2**: SQLite via `sqlx` for persistent storage. Fallback when L1 misses.

### Write Strategy

Writes go to L2 (SQLite) first, then update L1. This ensures durability even if the process crashes before L1 sync.

### Read Strategy

Reads check L1 first. On miss, query L2, and if found, populate L1 before returning.

## Key Operations

| Operation | Description |
|-----------|-------------|
| `set(key, value)` | Write to L2 → update L1 |
| `get(key)` | Check L1 → check L2 → return |
| `snapshot()` | Dump all entries into a `Snapshot` struct |
| `restore(snapshot)` | Restore from a previously taken snapshot |
