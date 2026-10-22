# Sisyphus Bridge Agent Knowledge Base

## OVERVIEW
gRPC bridge service exposing the Sisyphus cognitive engine to external clients (tonic + prost).

## STRUCTURE
- `src/main.rs`: tonic server, IPv6 `[::1]:50051` bind with IPv4 fallback, health check
- `src/bridge_service.rs` + `src/grpc/`: bridge service (`MySisyphusBridge`), LLM cognition RPCs (add_message, health check)
- gRPC types/proto: defined in sibling crate `../sisyphus-bridge-api` (own `build.rs` + `proto/`); this crate has no local proto/build.rs

## WHERE TO LOOK
- Server startup: `main.rs` (socket bind with IPv6→IPv4 auto-fallback)
- LLM cognition RPC: `grpc/execution.rs` (add_message handler); service impl in `grpc_handlers.rs`
- Proto definitions: `../sisyphus-bridge-api/proto/bridge.proto` (this crate has no own proto/build.rs)
- Client side: `../sisyphus-cli/src/bridge.rs` (BridgeClient)

## CONVENTIONS
- Default address `[::1]:50051`; `Config.bridge_address` overrides
- Production entry point (CLI connects via bridge, never directly to LLM)
- Bridge tests excluded from CI workspace (OOM); add `#[ignore = "requires dedicated runner"]`

## COMMANDS
```bash
cargo check -p sisyphus-bridge      # Check (heavy crate; not in default check-all)
cargo build -p sisyphus-bridge      # Build
cargo test -p sisyphus-bridge       # Test (requires dedicated runner)
make check-bridge                   # Make target
```
