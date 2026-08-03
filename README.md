# Sisyphus Cognitive Engine

A DAG-based cognitive execution engine with predict-execute-reflect-evolve loops.

## Architecture

```
sisyphus/
├── crates/           # 28 Rust crates
│   ├── sisyphus-core/        # DAG execution, cognitive loop, SOP orchestration
│   ├── sisyphus-cli/         # TUI client (ratatui + crossterm)
│   ├── sisyphus-bridge/      # gRPC + REST bridge service (tonic + axum)
│   ├── sisyphus-types/       # Shared types, Config, traits
│   ├── sisyphus-mem/         # Persistence/storage (SQLite/Postgres + vector)
│   ├── sisyphus-intel/       # Intelligence analysis (simulation, reflection, evolution)
│   ├── sisyphus-tools-*/     # Tool crates (agent, build, executor, llm, mcp, scm, session, system)
│   ├── sisyphus-roadmap/     # Strategic planning (Project → Milestone → Phase → Goal)
│   ├── sisyphus-api/         # REST API handlers (axum + OpenAPI 3.1)
│   ├── sisyphus-sdk/         # Rust SDK client
│   ├── sisyphus-knowledge/   # Knowledge graph & cognitive asset storage
│   ├── sisyphus-visual/      # Visual perception & embedding
│   ├── sisyphus-wasm-sdk/    # WASM tool SDK (guest crate)
│   ├── sisyphus-sandboxd/    # WASM sandbox daemon (process isolation)
│   ├── sisyphus-tests/       # Integration tests
│   └── ...
├── webui/            # Web dashboard (axum + HTMX + SSE)
├── evals/            # Agent capability evaluations
├── ide/              # IDE extensions
│   ├── typescript-sdk/       # TypeScript client SDK
│   ├── python-sdk/           # Python client SDK
│   ├── go-sdk/               # Go client SDK
│   ├── vscode/               # VS Code extension
│   └── zed/                  # Zed extension
├── desktop/tauri/    # Tauri desktop app (archived)
└── book/             # Documentation (mdbook)
```

## Quick Start

```bash
# Install
cargo install sisyphus-cli

# Run TUI
sisyphus

# Run with custom bridge address
sisyphus --bridge http://localhost:50051
```

## Features

### Core Engine
- **DAG Execution**: Parallel node dispatch with topological ordering
- **Cognitive Loop**: Predict -> Execute -> Reflect -> Evolve
- **SOP Orchestration**: Standard Operating Procedures as executable graphs
- **Sub-graph Recursion**: Nested SOP execution with context isolation
- **Speculative Execution**: Pre-compute successor node predictions

### TUI
- **Slash Commands**: Type `/` for command completion popup
- **@file References**: Reference files with `@path/to/file.rs`
- **Fuzzy Search**: Score-based fuzzy matching across all commands
- **Session Management**: Create, switch, fork, undo sessions
- **Command Palette**: Ctrl+K for quick navigation
- **DAG Visualization**: ASCII art DAG rendering with status colors

### Web Dashboard
- **SSE Real-time**: Server-Sent Events for live updates
- **WebSocket**: Bidirectional communication
- **DAG Viewer**: Interactive SVG visualization
- **Session Detail**: Message history with send-message form
- **SOP Execution**: Run SOPs from the browser
- **Skill Market**: Browse and install skills
- **Prometheus Metrics**: /metrics endpoint

### IDE Extensions
- **VS Code**: Status bar, DAG viewer, Agent panel, LSP diagnostics
- **Zed**: Extension scaffold with installation message

### SDKs
- **TypeScript**: `@sisyphus/sdk` - Full typed REST client
- **Python**: `sisyphus_sdk` - Dataclass-based client
- **Go**: `sisyphus` - Idiomatic Go client

### Performance
- **Pipeline Batching**: Adaptive batch size for node execution
- **Output Dedup Cache**: Content-hash deduplication (1000 entries)
- **Context Compression**: Truncation for large values
- **Execution Profiler**: Cache hit rate, slowest nodes

### Security
- **API Key Auth**: Bearer token authentication
- **External Directory Permissions**: Configurable file access control
- **RBAC Model**: Role-based access control (planned)

### Observability
- **Prometheus Metrics**: Request counts, session stats, uptime
- **Tracing**: Structured logging with tracing-subscriber
- **SSE Events**: Real-time execution event streaming

## Configuration

See `config.example.toml` for all options. Key settings:

```toml
[bridge]
address = "[::1]:50051"

[llm]
provider = "openai"
model = "gpt-4"

[tools]
shell_enabled = true
file_access_root = "."
```

Environment variables can be substituted with `{env:VAR}` or `{env:VAR|default}`.

## Development

```bash
cargo check-all          # Check all crates
cargo test-all           # Test all crates
cargo build --release -p sisyphus-cli  # Build release TUI
make test-core           # Full core tests
cargo bench -p sisyphus-core  # Benchmarks
```

## License

MIT
