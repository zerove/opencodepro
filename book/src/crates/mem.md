# sisyphus-mem — Memory & Caching

Two-level key-value store for SOP execution history, intermediate results, and configuration state.

## Architecture

```
┌──────────┐   write-through    ┌──────────┐
│   L1     │ ◄──────────────── │   L2     │
│ DashMap  │    (L2 → L1)      │  SQLite  │
│ (memory) │ ────────────────► │  (disk)  │
└──────────┘    read-through    └──────────┘
                 (L1 → L2)
```

- **L1**: In-memory `DashMap<String, serde_json::Value>` — sub-millisecond reads
- **L2**: SQLite via `sqlx` — persistent storage, WAL mode for concurrency

## Key Type: `MfsStore`

```rust
pub struct MfsStore {
    l1: Arc<DashMap<String, serde_json::Value>>,
    l2: SqlitePool,
}
```

### Construction

```rust
// In-memory (for testing)
let store = MfsStore::new("sqlite::memory:").await?;

// Persistent
let store = MfsStore::new("sqlite:data/state.db").await?;
```

WAL mode and `synchronous=NORMAL` are enabled automatically for performance.

### Read/Write Strategy

| Operation | Path | Atomicity |
|-----------|------|-----------|
| `set(key, value)` | Write to L2 SQLite → update L1 DashMap | L2 write is a single SQL `INSERT OR REPLACE` |
| `get(key)` | Check L1 → miss? → query L2 → populate L1 | L1 get is lock-free DashMap read |
| `snapshot()` | Dump all entries into `Snapshot` struct | Captures L2 snapshot via `SELECT *` |
| `restore(snapshot)` | Clear and reload from snapshot | Truncates table, bulk insert |

### Example

```rust
use sisyphus_mem::MfsStore;

let store = MfsStore::new("sqlite::memory:").await?;
store.set_state_json("my_key", serde_json::json!({"hello": "world"})).await?;
let val = store.get_state_json("my_key").await?;
assert_eq!(val, Some(serde_json::json!({"hello": "world"})));
```
