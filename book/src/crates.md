# Crate Reference

Sisyphus is structured as a Rust workspace with the following crates (the `sisyphus-tools` monolith was split into 10 sub-crates):

| Crate | Description | Dependencies |
|-------|-------------|-------------|
| [sisyphus-types](crates/types.md) | Core data structures: SOP graph, execution context, configuration | None (standalone) |
| [sisyphus-metrics](crates/metrics.md) | Prometheus observability instrumentation | types |
| [sisyphus-sandbox](crates/sandbox.md) | WASM sandbox runtime for tool execution | types, metrics |
| [sisyphus-intel](crates/intel.md) | Cognition: simulation, reflection, SOP evolution | types, metrics |
| [sisyphus-mem](crates/mem.md) | Multi-level cache (L1 DashMap + L2 SQLite) | types, metrics |
| [sisyphus-tools](crates/tools.md) | 10 sub-crates: built-in tools, agents, executors | types |
| [sisyphus-core](crates/core.md) | Kernel: DAG executor, cognitive loop, orchestration | types, intel, mem, sandbox, tools, metrics |
| [sisyphus-cli](crates/cli.md) | Command-line interface with agent subcommands | types, tools (10 sub-crates) |
| [sisyphus-bridge](crates/bridge.md) | HTTP/gRPC API server, entry point | core, tools (all transitive) |

## Crate Dependency Graph

```
sisyphus-bridge → sisyphus-core
                    ├── sisyphus-types
                    ├── sisyphus-intel → sisyphus-types, sisyphus-metrics
                    ├── sisyphus-mem   → sisyphus-types, sisyphus-metrics
                    ├── sisyphus-tools-{10 sub-crates} → sisyphus-types
                    ├── sisyphus-sandbox → sisyphus-types, sisyphus-metrics
                    ├── sisyphus-cli   → sisyphus-types, sisyphus-tools-*
                    └── sisyphus-metrics → sisyphus-types
```
