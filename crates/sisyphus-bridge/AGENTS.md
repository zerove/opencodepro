# Sisyphus Bridge Agent Knowledge Base

## OVERVIEW
gRPC bridge service exposing the Sisyphus cognitive engine to external clients (tonic + prost).

## STRUCTURE
- `src/main.rs`: tonic server, IPv6 `[::1]:50051` bind with IPv4 fallback, health check
- `src/cognition_bridge.rs`: Bridge LLM cognition service (add_message, health check RPCs)
- `build.rs`: Proto compilation via tonic-build
- `proto/bridge.proto`: gRPC service definitions

## WHERE TO LOOK
- Server startup: `main.rs` (socket bind with IPv6→IPv4 auto-fallback)
- LLM cognition RPC: `cognition_bridge.rs` (add_message handler)
- Proto definitions: `proto/bridge.proto`
- Client side: `../sisyphus-cli/src/bridge.rs` (BridgeClient)

## CONVENTIONS
- Default address `[::1]:50051`; `Config.bridge_address` overrides
- Production entry point (CLI connects via bridge, never directly to LLM)
- Bridge tests excluded from CI workspace (OOM); add `#[ignore = "requires dedicated runner"]`

## COMMANDS
```bash
cargo check -p sisyphus-bridge      # Check (nightly-only)
cargo build -p sisyphus-bridge      # Build
cargo test -p sisyphus-bridge       # Test (requires dedicated runner)
make check-bridge                   # Make target
```
