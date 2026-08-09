# ADR-003: Monolithic-to-Modular Restructure

- **Status**: Accepted
- **Date**: 2026-07-01
- **Driver**: 消除巨型文件，降低认知负载，提升 AI agent 可导航性

## Context

Several crates had grown monolithic `lib.rs` or `main.rs` files exceeding 1000+ lines:

| File | Before | After | Split Into |
|------|--------|-------|------------|
| `sisyphus-cli/src/main.rs` | 2540 | 1478 | commands.rs (1070) |
| `sisyphus-core/src/lib.rs` | ~3000+ | ~200 | cognitive_loop, executor, budget, intent_gate, dag/ |
| `sisyphus-intel/src/lib.rs` | ~2000+ | ~50 | synthesis, fuel, reflect, simulation, error |
| `sisyphus-mem/src/lib.rs` | 538 | 9 | types, store, tests |
| `sisyphus-tools/src/delegate.rs` | monolithic | modular | delegate/types.rs, etc. |

These large files make code hard to navigate (for humans and AI), slow to compile incrementally, and create merge conflicts.

## Decision

### 1. Sub-module by Concern, Not by Layer

Each extracted module owns one cohesive concern:

- **types.rs**: Data structures only (structs, enums, type aliases)
- **{module}.rs**: Logic + trait impls
- **tests.rs**: `#[cfg(test)]` module (moved out of lib)

Exception: Small modules (<100 lines) stay inline (e.g., `intent_gate.rs` at 42 lines).

### 2. lib.rs as Pure Re-export Hub

After extraction, `lib.rs` contains only:
```rust
pub mod store;
pub mod types;
pub mod user_preferences;

pub use store::MfsStore;
pub use types::{Snapshot, WriteOp};
```

No logic, no tests — pure module declaration.

### 3. Private Fields Use `pub(crate)`

Sub-modules need access to struct internals for trait impls. Rather than making fields fully public, use `pub(crate)` where needed:

```rust
pub struct MfsStore {
    pub(crate) l1: Arc<Mutex<LruCache<...>>>,
    pub(crate) l2: SqlitePool,
    pub(crate) write_tx: mpsc::UnboundedSender<WriteOp>,
}
```

### 4. No "Extract for the Sake of Extraction"

Files under 250 lines of cohesive code are left intact:

- `sisyphus-sandbox/src/plugin_manager.rs` (211 lines) — kept monolithic
- `sisyphus-core/src/budget.rs` (117 lines) — kept monolithic
- `sisyphus-core/src/intent_gate.rs` (42 lines) — kept monolithic

### 5. Test Placement

Unit tests move to the module they test:

```
src/store.rs          #[cfg(test)] → src/tests.rs
src/dag/mod.rs        #[cfg(test)]  stays in dag/tests.rs
```

Integration tests remain in `tests/` at crate root.

## Consequences

**Positive:**
- Each file has a single, searchable responsibility
- `cargo check` invalidates fewer files on change
- AI agents can target specific modules without loading entire crate
- Merge conflicts reduce (no more 10-person edits to one giant lib.rs)

**Negative:**
- Slightly more files to open when understanding full flow
- `pub(crate)` fields blur encapsulation (acceptable trade-off)

**Migration:**
Existing imports keep working — all public types are re-exported from the crate root. No downstream crate changes needed.

## Related

- [002-phase-3b-agent-team-architecture](002-phase-3b-agent-team-architecture.md): Phase 3b Multi-Agent Team Architecture
- PR #1: CLI main.rs split
- PR #2: Core/Intel module extraction
- PR #3: Tools types.rs extraction
- PR #4: Mem store/types extraction
