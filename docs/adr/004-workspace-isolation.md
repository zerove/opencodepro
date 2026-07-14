# ADR 004: Multi-tenant Workspace Isolation Architecture

## Status
Accepted

## Context
The original architecture of Sisyphus assumed a global "current workspace" context. In a multi-tenant or multi-project environment, this model is vulnerable to state collisions and data leaks between different projects running concurrently.

Specifically:
- `MfsStore` used global keys, allowing Project B to overwrite Project A's state if they used the same key.
- `RoadmapSqlStore` lacked strict `project_id` filtering on child entities (phases, goals), allowing potential unauthorized access or modification if IDs were guessed.

## Decision
We migrate from a global context model to an explicit `project_id` propagation model across the entire stack:

### 1. API Surface
- All gRPC and REST endpoints must now include a `project_id` (extracted from `X-Sisyphus-Workspace-Id` metadata in gRPC).
- The `SisyphusKernel` core methods (`execute_goal`, etc.) now explicitly accept `project_id`.

### 2. Storage Isolation
- **MfsStore (State Store)**: Implement key-prefixing. Every key is transformed to `project_id:original_key`.
- **RoadmapSqlStore**: 
    - Add `project_id` to `milestones`, `phases`, and `goals` tables.
    - Change Primary Keys to composite keys `(id, project_id)`.
    - Enforce `project_id` filters on all `SELECT`, `UPDATE`, and `DELETE` queries.

### 3. Propagation
- The `project_id` is passed as a reference/string through all manager and executor layers, ensuring no reliance on implicit global state.

## Consequences
### Positive
- **Absolute Isolation**: Data leaks between projects are mathematically eliminated at the storage layer.
- **Concurrency**: Multiple projects can now be executed in parallel within the same engine instance without interference.
- **Auditability**: Every state change is now explicitly linked to a specific project.

### Negative
- **Breaking Change**: All existing storage clients and tests had to be updated to provide `project_id`.
- **Slight Overhead**: Minor performance cost due to key prefixing and composite index lookups.

## Validation
- A reproduction test `test_cross_project_isolation_vulnerability` was created in `sisyphus-mem` to prove that Project B can no longer overwrite Project A's state.
- Full integration test suite for `sisyphus-roadmap` and `sisyphus-mem` passed.
