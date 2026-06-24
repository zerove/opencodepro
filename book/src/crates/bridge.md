# sisyphus-bridge — Entry Point & API

The binary crate that ties all components together into a runnable gRPC service.

## Entry Point

### `main.rs` (gRPC Server)

A Tonic gRPC server listening on `[::1]:50051`:

```protobuf
service SisyphusBridge {
  rpc ExecuteSOP (ExecuteSOPRequest) returns (SOPResponse);
  rpc ExecuteTool (ExecuteToolRequest) returns (ExecuteToolResponse);
  rpc HealthCheck (HealthCheckRequest) returns (HealthCheckResponse);
  rpc ListSOPs (ListSOPsRequest) returns (ListSOPsResponse);
  rpc StreamExecution (ExecuteSOPRequest) returns (stream ExecutionEvent);
}
```

**`ExecuteSOP`** handles two modes:
- **Goal execution**: If `sop_id` starts with `"goal:"`, the bridge synthesizes, executes, and runs the cognitive loop
- **Direct execution**: Otherwise, loads a saved SOP by ID and executes it

### Request/Response

```protobuf
message ExecuteSOPRequest {
  string sop_id = 1;
  map<string, string> args = 2;
}

message SOPResponse {
  string status = 1;   // "SUCCESS" or error string
  string result = 2;   // Execution result + cognition report
}
```

## Initialization Flow

```
Config (TOML + env vars)
    │
    ▼
MfsStore ← SQLite connection
    │
    ▼
WasmToolExecutor ← tools/ directory
    │
    ▼
SisyphusIntel ← simulator + reflector + evolver
    │
    ▼
GoalDrivenSynthesizer ← tool executor
    │
    ▼
SisyphusKernel ← state_store + tool_executor + intel + synthesizer
    │
    ▼
MySisyphusBridge ← wraps kernel
    │
    ▼
gRPC server ← tonic on [::1]:50051
```

## Configuration

The bridge reads `config.toml` (or `$SISYPHUS_CONFIG` path) and calls `config.apply_env_overrides()` to merge environment variables. Metrics initialized on the port from `config.metrics.prometheus_port`.

### ExecuteTool — Direct Tool Execution

Calls any built-in or agent tool directly via the `ExecutorRouter`. Useful for remote access to `librarian`, `metis`, `momus`, and all file/shell/web tools.

```protobuf
message ExecuteToolRequest {
  string tool_id = 1;      // e.g. "file_read", "librarian/research", "metis/analyse"
  string args_json = 2;    // JSON-encoded arguments (see crates/tools.md for schemas)
  uint64 fuel_limit = 3;   // max execution units (0 = default)
}

message ExecuteToolResponse {
  string status = 1;       // "SUCCESS" or error message
  string result_json = 2;  // JSON-encoded result
}
```

Usage via the CLI's `BridgeClient`:

```rust
let mut client = BridgeClient::connect("http://[::1]:50051").await?;
let resp = client.execute_tool(
    "librarian/research",
    r#"{"query": "Rust async", "scope": "web", "num": 5}"#,
).await?;
println!("{}", resp.result_json);
```

## Running

```bash
# Build
cargo build --release -p sisyphus-bridge

# Run with default config
./target/release/sisyphus-bridge

# Custom config
SISYPHUS_CONFIG=/etc/sisyphus/prod.toml ./target/release/sisyphus-bridge
```
