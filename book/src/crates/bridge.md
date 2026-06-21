# sisyphus-bridge — Entry Point & API

The binary crate that ties all components together into a runnable application.

## Entry Points

### `main.rs` (Server)

A Tonic gRPC server that exposes the cognitive loop as a network service:

- **`ExecuteGoal`**: Submit a natural-language goal path for execution. The bridge calls `SisyphusKernel.execute_goal()` and returns the cognitive report.
- **`ExecuteSop`**: Execute a pre-synthesized SOP by ID, with optional evolution loop.

### `client.rs` (CLI Client)

A gRPC client binary for interacting with the server:

```bash
cargo run --bin client -- execute-goal --goals /path/to/goal.json
cargo run --bin client -- sop --sop-id my_sop
```

### `stress_test.rs` (Benchmark)

An in-process stress test that synthesizes and executes SOPs in a loop, measuring throughput and latency.

## Configuration

The bridge reads configuration from TOML files and environment variables, constructing the `sisyphus_types::Config` struct and initializing all subsystems.
