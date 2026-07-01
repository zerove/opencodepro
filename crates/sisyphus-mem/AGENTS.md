# Sisyphus Mem Agent Knowledge Base

## OVERVIEW
Tiered storage layer: L1 (DashMap in-memory cache) + L2 (SQLite persistent store) with snapshot/restore.

## WHERE TO LOOK
- Store implementation: `store.rs` (MfsStore, L1/L2 coordination)
- Snapshot: `snapshot.rs` (serialize/deserialize state)
- Schema: inline in store.rs (SQLite table definitions)

## CONVENTIONS
- Write-through: writes go to both L1 and L2 atomically
- Snapshot uses postcard serialization for compactness
- MfsStore behind `Arc<dyn Mfs>` for testability
- All operations async via tokio

## COMMANDS
```bash
cargo test -p sisyphus-mem
```
