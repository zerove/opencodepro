# ADR-005: Sandboxd Daemon Architecture

**Status:** Accepted
**Date:** 2026-07-30
**Author:** Sisyphus Team

## Context

The WASM sandbox (wasmtime) pulls in heavy LLVM/cranelift dependencies. When embedded in the bridge process, memory peaks above 7GB during `cargo check`, causing OOM on CI runners. We need an isolation strategy.

## Decision

Introduce `sisyphus-sandboxd` — a standalone daemon process that:

1. Runs `WasmToolExecutor` in an isolated process
2. Communicates with the bridge via a stdio JSON-line protocol (v1) or gRPC (v2)
3. Is started and stopped by the bridge on demand

### Communication Protocol (v1 — stdio)

- Each line is a complete JSON message terminated by `\n`
- Requests have an `id` field; responses echo the `id`
- Notifications (no `id`): `{"method":"shutdown"}`
- Supported methods: `execute`, `ping`, `shutdown`

### gRPC Mode (v2 — planned)

- The daemon exposes a tonic gRPC service on `127.0.0.1:0`
- Bridge uses `SandboxdClient` to communicate
- Enables proper health checking, streaming, and concurrent execution

## Consequences

**Positive:**
- Bridge no longer links wasmtime → CI OOM resolved at architecture level
- Independent daemon can be restarted without bridge downtime
- Process-level memory isolation

**Negative:**
- Added deployment complexity (two binaries instead of one)
- IPC overhead for WASM calls
- Need to manage daemon lifecycle (spawn, health-check, reap)

## Configuration

```toml
[sandbox]
use_sandboxd = false  # set true to enable daemon mode
sandboxd_path = "sisyphus-sandboxd"  # search PATH by default
```
