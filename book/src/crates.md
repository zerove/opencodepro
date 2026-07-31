# Crate Reference

Sisyphus is structured as a Rust workspace with the following crates (the `sisyphus-tools` monolith was split into 10 sub-crates):

| Crate | Description | Dependencies |
|-------|-------------|-------------|
| [sisyphus-types](crates/types.md) | Core data structures: SOP graph, execution context, configuration | None (standalone) |
| [sisyphus-metrics](crates/metrics.md) | Prometheus observability instrumentation | types |
| [sisyphus-sandbox](crates/sandbox.md) | WASM sandbox runtime for tool execution | types, metrics |
| [sisyphus-sandboxd](crates/sandboxd.md) | Standalone WASM sandbox daemon (stdio/gRPC) | sandbox |
| [sisyphus-bridge-api](crates/bridge-api.md) | gRPC API definitions (prost/tonic) | types |
| [sisyphus-intel](crates/intel.md) | Cognition: simulation, reflection, SOP evolution | types, metrics |
| [sisyphus-mem](crates/mem.md) | Multi-level cache (L1 DashMap + L2 SQLite/PostgreSQL + pgvector) | types, metrics |
| [sisyphus-tools-core](crates/tools-core.md) | Core tool infrastructure (error, feedback, hooks) | types, metrics |
| [sisyphus-tools-llm](crates/tools-llm.md) | LLM client abstraction (multi-provider, budget, circuit breaker) | types, metrics |
| [sisyphus-tools-llm-tools](crates/tools-llm-tools.md) | High-level LLM tools (librarian, oracle, momus, metis) | types, llm, intel |
| [sisyphus-tools-agent](crates/tools-agent.md) | Agent registry, delegation, team orchestration | types, llm, intel |
| [sisyphus-tools-build](crates/tools-build.md) | Build/test/LSP tools | types, llm |
| [sisyphus-tools-system](crates/tools-system.md) | File, bash, web, explore, permissions, scaffold tools | types |
| [sisyphus-tools-mcp](crates/tools-mcp.md) | MCP server integration (stdio/HTTP) | types |
| [sisyphus-tools-session](crates/tools-session.md) | Session & todo management | types |
| [sisyphus-tools-executor](crates/tools-executor.md) | Task execution & delegation | types, llm |
| [sisyphus-tools-llm-tools](crates/tools-llm-tools.md) | High-level LLM tools (oracle, momus, metis, librarian) | types, llm, intel |
| [sisyphus-tools-mcp](crates/tools-mcp.md) | MCP server integration (stdio/HTTP) | types |
| [sisyphus-tools-scm](crates/tools-scm.md) | SCM (git) operations | types |
| [sisyphus-tools-session](crates/tools-session.md) | Session & todo management | types |
| [sisyphus-tools-system](crates/tools-system.md) | File, bash, web, explore, permissions, scaffold | types |
| [sisyphus-tools-executor](crates/tools-executor.md) | Task execution & delegation | types, llm |
| [sisyphus-core](crates/core.md) | Kernel: DAG executor, cognitive loop, orchestration | types, intel, mem, sandbox, tools, metrics |
| [sisyphus-cli](crates/cli.md) | Command-line interface with agent subcommands | types, tools (10 sub-crates) |
| [sisyphus-bridge](crates/bridge.md) | HTTP/gRPC API server, entry point | core, tools (all transitive) |
| [sisyphus-roadmap](crates/roadmap.md) | Strategic planning: Project → Milestone → Phase → Goal | types, intel, mem, metrics |
| [sisyphus-bridge-api](crates/bridge-api.md) | gRPC API protobuf definitions (prost/tonic) | types |
| [sisyphus-api](crates/api.md) | REST API handlers (axum + OpenAPI 3.1) | bridge, mem, types |
| [sisyphus-sdk](crates/sdk.md) | Rust SDK client for REST API | api |
| [sisyphus-knowledge](crates/knowledge.md) | Knowledge management & asset storage | types, mem |
| [sisyphus-visual](crates/visual.md) | Visual perception & embedding | types, mem |
| [sisyphus-wasm-sdk](crates/wasm-sdk.md) | WASM tool SDK (guest crate) | types |
| [sisyphus-webui](crates/webui.md) | Web dashboard (axum + HTMX + SSE) | bridge, types |
| [sisyphus-sandboxd](crates/sandboxd.md) | Standalone WASM sandbox daemon (stdio/gRPC dual-mode) | sandbox |

## Crate Dependency Graph

```
sisyphus-bridge → sisyphus-core
                    ├── sisyphus-types
                    ├── sisyphus-intel → sisyphus-types, sisyphus-metrics
                    ├── sisyphus-mem   → sisyphus-types, sisyphus-metrics
                    ├── sisyphus-sandbox → sisyphus-types, sisyphus-metrics
                    ├── sisyphus-sandboxd → sisyphus-sandbox
                    ├── sisyphus-tools-{10 sub-crates} → sisyphus-types
                    ├── sisyphus-cli   → sisyphus-types, sisyphus-tools-*
                    ├── sisyphus-intel → sisyphus-types, sisyphus-metrics
                    ├── sisyphus-mem   → sisyphus-types, sisyphus-metrics
                    ├── sisyphus-roadmap → sisyphus-types, sisyphus-intel, sisyphus-mem, sisyphus-metrics
                    ├── sisyphus-bridge-api → sisyphus-types
                    ├── sisyphus-api   → sisyphus-bridge, sisyphus-mem, sisyphus-types
                    ├── sisyphus-sdk   → sisyphus-api
                    ├── sisyphus-knowledge → sisyphus-types, sisyphus-mem
                    ├── sisyphus-visual → sisyphus-types, sisyphus-mem
                    ├── sisyphus-wasm-sdk → sisyphus-types
                    ├── sisyphus-webui → sisyphus-bridge, sisyphus-types
                    ├── sisyphus-sandboxd → sisyphus-sandbox
                    └── sisyphus-metrics → sisyphus-types
```