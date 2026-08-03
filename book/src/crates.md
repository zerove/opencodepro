# Crate Reference

Sisyphus is structured as a Rust workspace with 29 members: 28 crates under `crates/` (the former `sisyphus-tools` monolith was split into 10 sub-crates) plus `webui` and `evals`.

## 基础层 (Foundation)

| Crate | Description | Dependencies |
|-------|-------------|-------------|
| [sisyphus-types](crates/types.md) | Core data structures: SOP graph, execution context, configuration | None (standalone) |
| [sisyphus-metrics](crates/metrics.md) | Prometheus/OpenTelemetry observability instrumentation | types |
| [sisyphus-bridge-api](crates/bridge-api.md) | gRPC API definitions (prost/tonic) | types |

## 认知层 (Cognition)

| Crate | Description | Dependencies |
|-------|-------------|-------------|
| [sisyphus-core](crates/core.md) | Kernel: DAG executor, cognitive loop v1/v2, orchestration | types, intel, mem, sandbox, tools, metrics |
| [sisyphus-intel](crates/intel.md) | Cognition: simulation, reflection, SOP evolution, formal verification | types, metrics |
| [sisyphus-roadmap](crates/roadmap.md) | Strategic planning: Project → Milestone → Phase → Goal | types, intel, mem, metrics |

## 存储层 (Storage)

| Crate | Description | Dependencies |
|-------|-------------|-------------|
| [sisyphus-mem](crates/mem.md) | Multi-level cache (L1 DashMap + L2 SQLite/PostgreSQL + pgvector) | types, metrics |
| [sisyphus-knowledge](crates/knowledge.md) | Knowledge graph & cognitive asset management | types, mem |
| [sisyphus-visual](crates/visual.md) | Visual perception & embedding | types, mem |

## 工具层 (Tools, 10 个子 crate)

| Crate | Description | Dependencies |
|-------|-------------|-------------|
| [sisyphus-tools-core](crates/tools-core.md) | Core tool infrastructure (error, feedback, hooks) | types, metrics |
| [sisyphus-tools-llm](crates/tools-llm.md) | LLM client abstraction (multi-provider, budget, circuit breaker) | types, metrics |
| [sisyphus-tools-llm-tools](crates/tools-llm-tools.md) | High-level LLM tools (librarian, oracle, momus, metis, scribe) | types, llm, intel |
| [sisyphus-tools-agent](crates/tools-agent.md) | Agent registry, delegation, team orchestration | types, llm, intel |
| [sisyphus-tools-build](crates/tools-build.md) | Build/test/LSP tools | types, llm |
| [sisyphus-tools-system](crates/tools-system.md) | File, bash, web, explore, permissions, scaffold tools | types |
| [sisyphus-tools-mcp](crates/tools-mcp.md) | MCP server integration (stdio/HTTP) | types |
| [sisyphus-tools-session](crates/tools-session.md) | Session & todo management | types |
| [sisyphus-tools-executor](crates/tools-executor.md) | Task execution & delegation | types, llm |
| [sisyphus-tools-scm](crates/tools-scm.md) | SCM (git/GitHub) operations | types |

## 沙箱层 (Sandbox)

| Crate | Description | Dependencies |
|-------|-------------|-------------|
| [sisyphus-sandbox](crates/sandbox.md) | WASM sandbox runtime for tool execution (wasmtime) | types, metrics |
| [sisyphus-sandboxd](crates/sandboxd.md) | Standalone WASM sandbox daemon (stdio/gRPC dual-mode) | sandbox |
| [sisyphus-wasm-sdk](crates/wasm-sdk.md) | WASM tool SDK (guest crate) | types |

## 接口层 (Interfaces)

| Crate | Description | Dependencies |
|-------|-------------|-------------|
| [sisyphus-bridge](crates/bridge.md) | HTTP/gRPC API server, entry point | core, tools (all transitive) |
| [sisyphus-api](crates/api.md) | REST API handlers (axum + OpenAPI 3.1) | bridge, mem, types |
| [sisyphus-cli](crates/cli.md) | Command-line interface + TUI with agent subcommands | types, tools (10 sub-crates) |
| [sisyphus-sdk](crates/sdk.md) | Rust SDK client for REST API | api |
| [sisyphus-tauri](crates/tauri.md) | Tauri desktop app (archived) | api, types |

## 测试与评测 (Test & Eval)

| Crate | Description | Dependencies |
|-------|-------------|-------------|
| [sisyphus-tests](crates/tests.md) | Cross-crate integration tests | all core crates |
| evals | Agent capability evaluations (metis/momus/librarian/scribe/security) | types, llm |

## Crate Dependency Graph

```
sisyphus-bridge → sisyphus-core
                    ├── sisyphus-types
                    ├── sisyphus-intel → sisyphus-types, sisyphus-metrics
                    ├── sisyphus-mem   → sisyphus-types, sisyphus-metrics
                    ├── sisyphus-sandbox → sisyphus-types, sisyphus-metrics
                    ├── sisyphus-sandboxd → sisyphus-sandbox
                    ├── sisyphus-roadmap → sisyphus-types, sisyphus-intel, sisyphus-mem, sisyphus-metrics
                    ├── sisyphus-knowledge → sisyphus-types, sisyphus-mem
                    ├── sisyphus-visual → sisyphus-types, sisyphus-mem
                    ├── sisyphus-tools-{10 sub-crates} → sisyphus-types
                    ├── sisyphus-cli   → sisyphus-types, sisyphus-tools-*
                    ├── sisyphus-bridge-api → sisyphus-types
                    ├── sisyphus-api   → sisyphus-bridge, sisyphus-mem, sisyphus-types
                    ├── sisyphus-sdk   → sisyphus-api
                    ├── sisyphus-wasm-sdk → sisyphus-types
                    ├── sisyphus-webui → sisyphus-bridge, sisyphus-types
                    └── sisyphus-metrics → sisyphus-types
```
