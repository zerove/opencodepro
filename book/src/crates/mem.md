# sisyphus-mem — Memory & Caching

Three-tier storage for SOP execution history, intermediate results, configuration state, and vector embeddings.

## Architecture

```
┌──────────┐   write-back     ┌──────────┐   optional   ┌────────────┐
│   L1     │ ◄─────────────── │   L2     │ ◄─────────── │  pgvector  │
│  LRU     │    (buffered)    │  SQLite  │   (ANN index) │ (Postgres) │
│ (memory) │ ───────────────► │  (disk)  │ ────────────► │  (exter.)  │
└──────────┘   read-through   └──────────┘   vector        └────────────┘
                (L1 → L2)                   similarity
```

- **L1**: In-memory `LruCache<String, Value>` with configurable capacity (default 10,000)
- **L2**: SQLite via `sqlx` with WAL mode and async write-back buffer
- **Vector**: Optional PostgreSQL / pgvector for ANN similarity search

## Key Type: `SqlStore`

```rust
pub struct SqlStore {
    l1: Arc<Mutex<LruCache<String, CacheEntry>>>,
    pool: AnyPool,  // SQLite or PostgreSQL
    write_tx: mpsc::UnboundedSender<WriteOp>,
    config: SqlStoreConfig,
}
```

### Construction

```rust
// SQLite in-memory (for testing)
let store = SqlStore::new("sqlite::memory:").await?;

// SQLite persistent
let store = SqlStore::new("sqlite:data/sisyphus.db").await?;

// PostgreSQL with pgvector
let config = SqlStoreConfig {
    database_url: "postgres://user:pass@localhost/sisyphus".into(),
    enable_vectors: true,
    vector_dimension: 1536,
    ..Default::default()
};
let store = SqlStore::with_config(config).await?;
```

### Read/Write Strategy

| Operation | Path | Atomicity |
|-----------|------|-----------|
| `set(key, value)` | L1 cache → batch write buffer → L2 | Buffered, flushed every 100ms or 100 ops |
| `get(key)` | Check L1 → miss? query L2 → populate L1 | Lock-free LRU read |
| `flush()` | Force all buffered writes to L2 | Barrier with oneshot |
| `snapshot()` / `restore()` | L1 + L2 round-trip | Full table dump / clear + reload |

### Vector Search

```rust
// SQLite backend: full table scan + in-memory cosine similarity
// PostgreSQL backend: pgvector <-> (cosine distance) operator with ANN index
let results = store.search_similar(
    Some("my-project"),
    "experiences",
    &[0.1, 0.2, ..., 0.9],
    5,  // top-k
).await?;
```

## Additional Stores

| Type | Description |
|------|-------------|
| `SopSqlStore` | SOP definition CRUD (DAG nodes + edges) |
| `RoadmapSqlStore` | Project milestones, phases, and goals |
| `SkillMarket` | Remote skill marketplace (publish / install / uninstall) |
| `WorkspaceManager` | Workspace lifecycle (create / switch / transfer / export) |
| `VectorMemoryStore` | High-level vector memory with user-scoped collections |
| `UserMemoryStore` | Long-term user preference memory |
| `MfsStore` | Legacy flat KV store (migration to SqlStore recommended) |
