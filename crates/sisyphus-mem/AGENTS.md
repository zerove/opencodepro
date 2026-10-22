# Sisyphus Mem Agent Knowledge Base

## OVERVIEW
Tiered storage layer: L1 (LRU in-memory cache) + L2 (SQLite persistent store) with snapshot/restore.

## WHERE TO LOOK
- Store implementation: `store.rs` (MfsStore, L1/L2 coordination)
- Snapshot: `take_snapshot` / `restore_snapshot` in `store.rs` (types in `types.rs`)
- Schema: SQLite DDL is per-module (`store.rs`, `sop_store.rs`, `skill_store.rs`, `roadmap_store.rs`, `vector_memory.rs`)

## CONVENTIONS
- Writes: L1 is updated immediately; L2 writes are buffered (`WriteOp` channel) and flushed in batches by a worker
- State is persisted as JSON via serde_json (no binary codec)
- Test doubles via `Arc<dyn EmbeddingClient>` / `Arc<dyn ExperienceBank>`; `MfsStore` itself is a concrete struct
- All operations async via tokio

## COMMANDS
```bash
cargo test -p sisyphus-mem
```
