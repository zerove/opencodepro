# Getting Started

This guide walks you through setting up Sisyphus, running the cognitive engine, and executing your first SOP.

## Environment Requirements

Before you begin, ensure your system meets the following requirements:

| Component | Requirement | Notes |
|-----------|-------------|-------|
| **Rust** | nightly (1.98.0+) | Required by wasmtime/cranelift; stable has ICE on bridge (1.96.0). A `rust-toolchain.toml` override is provided. |
| **Go** | 1.21+ | Required for building the Go SDK (`crates/sisyphus-sdk-go`). |
| **Node.js** | 20+ | Required for the TypeScript SDK and MCP tool servers (`workers/ts-tools-mcp`). |
| **Operating System** | Linux / macOS | Windows is not currently supported for the bridge or sandbox. |

To install Rust nightly:

```bash
rustup toolchain install nightly
rustup default nightly
```

Verify your setup:

```bash
rustc --version  # should show nightly-2026-xx-xx or similar
cargo --version
```

## Quick Start

### 1. Clone and Build

```bash
git clone https://github.com/zeronhq/sisyphus-wasm.git
cd sisyphus-wasm

# Check the workspace (excludes heavy bridge/sandbox crates for speed)
make check-all

# Build the full workspace
make build-all
```

> **Note:** Full `cargo check --workspace` may OOM on machines with less than 16 GB RAM because `sisyphus-sandbox` and `sisyphus-bridge` pull in wasmtime/cranelift. Use `make check-all` (which excludes heavy crates) for day-to-day development.

### 2. Configure

Copy the example configuration and adjust as needed:

```bash
cp config.example.toml config.toml
```

The default configuration works out of the box with an LLM endpoint at `http://localhost:8000/v1/chat/completions`. Set `SISYPHUS_LLM_API_KEY` and `SISYPHUS_LLM_ENDPOINT` to point to your LLM provider:

```bash
export SISYPHUS_LLM_API_KEY="your-api-key"
export SISYPHUS_LLM_ENDPOINT="https://api.openai.com/v1/chat/completions"
export SISYPHUS_LLM_MODEL="gpt-4o"
```

See the [Configuration](./configuration.md) page for all available options.

### 3. Run the Bridge

The gRPC bridge is the main entry point for the engine. It exposes both gRPC (port 50051) and HTTP REST (port 8080) APIs:

```bash
cargo run -p sisyphus-bridge
```

By default, the bridge listens on `http://[::1]:50051` (IPv6 with automatic IPv4 fallback). You can override this with:

```bash
cargo run -p sisyphus-bridge -- --address http://127.0.0.1:50051
```

You should see log output confirming the bridge has started:

```
[2026-07-30T11:40:00Z INFO  sisyphus_bridge] Starting gRPC bridge on [::1]:50051
[2026-07-30T11:40:00Z INFO  sisyphus_bridge] HTTP REST API on [::1]:8080
```

### 4. Connect the TUI

In a separate terminal, launch the Terminal User Interface:

```bash
cargo run -p sisyphus-cli
```

If the bridge is running on a non-default address, pass the `--bridge` flag:

```bash
cargo run -p sisyphus-cli -- --bridge http://127.0.0.1:50051
```

The TUI connects to the bridge and presents the main console. Use `Ctrl+N` to create a new session, then type a goal in natural language.

See the [TUI Guide](./tui-guide.md) for keyboard shortcuts and features.

### 5. Verify with a Health Check

Once the bridge is running, verify connectivity with a quick REST API call:

```bash
curl http://[::1]:8080/health
```

Expected response:

```json
{"status":"ok","version":"0.1.0"}
```

## Simple Example: Execute a SOP

Once the bridge is running and the TUI is connected, follow these steps:

1. **Start a new session** — Press `Ctrl+N` in the TUI.
2. **Enter a goal** — Type a natural-language goal such as:
   ```
   Create a Markdown file named "hello-sisyphus.md" with the content "# Hello, Sisyphus!"
   ```
3. **Observe the cognitive loop** — The engine will:
   - Synthesize a SOP (a DAG of sub-steps)
   - Execute each step using WASM tools or LLM calls
   - Reflect on results and evolve the SOP if needed
   - Converge when the goal is achieved
4. **View the result** — The completed session shows the steps taken, the deviation score, and the final output.

You can also interact via the REST API:

```bash
# Submit a goal
curl -X POST http://[::1]:8080/v1/goals \
  -H "Content-Type: application/json" \
  -d '{"goal": "Create a Markdown file named hello-sisyphus.md with content: # Hello, Sisyphus!"}'

# Check goal status (replace {id} with the returned goal ID)
curl http://[::1]:8080/v1/goals/{id}

# List all goals
curl http://[::1]:8080/v1/goals
```

### Running a SOP Programmatically (Rust)

If you are integrating Sisyphus as a library:

```rust
use sisyphus_core::kernel::SisyphusKernel;
use sisyphus_types::config::Config;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_file("config.toml")?;
    let mut kernel = SisyphusKernel::new(config).await?;

    let result = kernel.execute_goal("Create a file named hello.md").await?;
    println!("Result: {:?}", result);

    Ok(())
}
```

## Running Tests

Sisyphus has a comprehensive test suite. Use these commands for different scopes:

```bash
make test-all          # Run all tests (CI equivalent)
make test-core         # Full sisyphus-core test suite (includes wasmtime)
make test-core-quick   # Lib-only sisyphus-core tests (~2 seconds)
make test-tools        # Test all 10 tool sub-crates (lib only, fast)
cargo test -p sisyphus-mem  # Storage layer tests
cargo bench -p sisyphus-core  # Cognitive loop benchmarks
```

## Next Steps

- [Architecture Overview](./architecture.md) — Understand the system design and cognitive loop
- [Configuration](./configuration.md) — Learn about all configuration options and environment variables
- [TUI Guide](./tui-guide.md) — Master the terminal interface with keyboard shortcuts and slash commands
- [Web Dashboard](./web-dashboard.md) — Real-time monitoring and DAG visualization in your browser
- [SDK Reference](./sdk-reference.md) — Build integrations with TypeScript, Python, or Go SDKs
- [REST API](./rest-api.md) — Programmatic control of the engine via HTTP endpoints
- [Security](./security.md) — Authentication, sandboxing, and deployment hardening
- [Deployment](./deployment.md) — Production deployment guides and best practices
- [Performance](./performance.md) — Tuning the cognitive loop, caching, and sandbox configuration
