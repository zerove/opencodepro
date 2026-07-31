# Sisyphus Project Dashboard

> **Auto-generated at:** <!-- GENERATED -->
> **Commits:** 125+
> **Workspace:** 21 crates + webui

## Crate Health

| Crate | Status | Tests | Key Metrics |
|-------|--------|-------|-------------|
| **sisyphus-core** | ✅ Production | 200+ | Cognitive loop v1+v2, DAG executor |
| **sisyphus-bridge** | ✅ Production | 37 integration | gRPC (17 RPCs) + REST (34 endpoints) |
| **sisyphus-cli** | ✅ Production | 37 unit | clap CLI + ratatui TUI |
| **sisyphus-types** | ✅ Production | 29 unit | Config, traits, shared types |
| **sisyphus-webui** | ✅ Production | - | axum + HTMX + SSE dashboard |
| **sisyphus-sandbox** | ✅ Production | - | wasmtime + fuel + memory limits |
| **sisyphus-sandboxd** | ✅ Production | 6 | stdio + gRPC dual-mode daemon |
| **sisyphus-mem** | ✅ Production | - | L1 (DashMap) + L2 (SQLite) + Vector |
| **sisyphus-intel** | ✅ Production | - | Simulation, synthesis, planners |
| **sisyphus-tools-*** (10) | ✅ Production | - | 40+ tool modules |
| **sisyphus-tests** | ✅ Production | - | E2E cross-project integration |

## CI Pipeline (14 Steps)

```mermaid
graph LR
    FMT[fmt] --> LINT[clippy]
    LINT --> TEST[unit tests]
    TEST --> BRIDGE[bridge 37]
    BRIDGE --> E2E[e2e smoke]
    E2E --> BENCH[benchmarks]
    BENCH --> BUILD[release build]
    BUILD --> SDK[sdk tests]
    SDK --> DOCKER[docker]
    DOCKER --> RELEASE[GitHub Release]
```

## Quality Gates

| Gate | Status | Detail |
|------|--------|--------|
| `cargo fmt --check` | ✅ | Passes cleanly |
| `cargo clippy -D warnings` | ✅ | All crates clean |
| `cargo test` | ✅ | 37 bridge + 37 CLI + 29 types |
| SDK TS | ✅ | 14 tests pass |
| SDK Python | ✅ | 21 tests pass |
| SDK Go | ✅ | 13 tests pass |

## Infrastructure

| Component | Status | Detail |
|-----------|--------|--------|
| **Docker** | ✅ | Multi-stage build, docker-compose |
| **Nightly CI** | ✅ | GHCR + multi-platform |
| **Release CI** | ✅ | Auto-changelog, all platforms |
| **IDE Plugins** | ✅ | VSCode 640 lines + Zed 395 lines |

## Cognitive Loop Maturity

```
v1: Predict → Execute → Reflect → Evolve  [PRODUCTION]
v2: + AdaptiveDepthController              [INTEGRATED]
    + AnomalyDetector                      [INTEGRATED]
    + ParallelBranchPredictor              [INTEGRATED]
    + Simulation-based verification        [INTEGRATED]
```

## Key ADRs

| ADR | Decision | Status |
|-----|----------|--------|
| ADR-002 | Multi-agent team architecture | ✅ Accepted |
| ADR-003 | Monolithic-to-modular restructure | ✅ Accepted |
| ADR-004 | Workspace isolation strategy | ✅ Accepted |
| ADR-005 | Sandboxd daemon architecture | ✅ Accepted |
| ADR-006 | Configuration system (TOML + env) | ✅ Accepted |

## Getting Started

```bash
# Build and run
make check-all
cargo run -p sisyphus-bridge    # gRPC + REST
cargo run -p sisyphus-cli -i    # TUI

# Test
cargo test -p sisyphus-bridge --test bridge_integration_test  # 37 tests
```

## Next Development Areas

- **REST API endpoint coverage**: Add missing gRPC → HTTP mappings
- **Sandboxd cluster mode**: Multi-daemon pool for concurrent WASM
- **Multi-architecture builds**: ARM64 Linux, macOS x86_64
- **Otel/distributed tracing**: Replace Prometheus-only with OpenTelemetry
