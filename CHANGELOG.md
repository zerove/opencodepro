# Changelog

All notable changes to Sisyphus will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — Unreleased

### Added

#### Core Engine
- **Cognitive loop v1**: Recursive observe–orient–decide–act cycle with configurable max iterations and convergence threshold.
- **Cognitive loop v2**: Adaptive depth control (confidence + convergence velocity), parallel branch prediction, and runtime anomaly detection.
- **SOP (Standard Operating Procedure)**: DAG-based procedure representation with parallel execution via topological ordering.
- **Monte Carlo simulation**: Path probability estimation and blended cost analysis over SOP graphs.
- **Structured reflection**: Deviation scoring between predicted and actual execution outcomes.
- **Adaptive fuel pricing**: Exponential backoff fuel pricer that adjusts budgets based on execution history.
- **SOP evolution**: Automatic graph mutation — retry wrappers, fuel adjustment, and subgraph restructuring.
- **Cross-graph dependency resolution**: CompositeGraph with topological execution order across multiple SOPs.
- **Self-programming loop**: Tool gap detection → generate → compile → verify → deploy → register.
- **Chaos engineering**: Configurable fault injection (delay/error), auto-repair pipeline triggered after repeated faults.
- **Strategic planning**: RoadmapManager with Project → Milestone → Phase → Goal hierarchy, strategic reflection, and autonomous milestone generation.

#### Storage & Retrieval
- **Multi-level cache**: L1 (DashMap in-memory) + L2 (SQLite persistent) with snapshot/restore and write-through.
- **Structured storage**: SQLite/PostgreSQL backends with pgvector support for semantic search.
- **Vector memory**: Cosine-similarity search over stored embeddings (text + visual modalities).
- **Experience bank**: Vector-based semantic retrieval of past execution experiences.
- **User preferences**: Persistent per-user preference storage.

#### APIs & Interfaces
- **gRPC bridge**: 17 RPCs including ExecuteSOP, ExecuteTool, DecomposeGoal, skill/plugin/workspace management, streaming execution, and meta-learning insights.
- **REST API**: 34 endpoints (health, goals, sessions, sops, skills, workspaces, roadmap, cognitive map) with OpenAPI 3.1 spec and Swagger UI.
- **CLI client**: 40+ subcommands spanning file tools, git, build, agents, engine, sessions, and dev workflows.
- **TUI**: ratatui-based interactive terminal with sessions, dashboard, DAG trace, workspace management, and skill market.
- **Web dashboard**: axum + HTMX + SSE real-time dashboard with DAG visualization, chaos panel, and live events.
- **SDKs**: TypeScript (14 tests), Python (21 tests), Go (13 tests), Rust (`sisyphus-sdk`).

#### Sandbox & Deployment
- **WASM sandbox**: Secure tool code execution via Wasmtime with fuel metering, memory limits, and module caching.
- **Sandboxd daemon**: Process-isolated WASM execution with stdio JSON-line (v1) and gRPC (v2) protocols.
- **Docker**: Multi-stage build with docker-compose (bridge + optional PostgreSQL).
- **Nightly CI**: Automated GHCR image publishing with multi-platform binaries.
- **Release pipeline**: Auto-changelog generation with multi-platform artifact upload.

#### Developer Experience
- **IDE plugins**: VS Code extension (status bar, session tree, agent panel, DAG view) and Zed extension (LSP bridge, slash commands).
- **Property-based tests**: Proptest coverage for SOPGraph roundtrip, ExecutionContext operations, and CompositeGraph correctness.
- **Supply chain audit**: cargo-deny configuration with license, ban, and advisory checking.
- **mdBook documentation**: Architecture overview, crate references, configuration guide, and key concepts.
- **Pre-commit hooks**: Automated fmt + clippy + check on every commit.
- **CI pipeline**: 14-step workflow covering fmt, clippy, full test suite (1261 tests), benchmarks, E2E smoke, and SDK tests.

### Changed
- Monolithic `sisyphus-tools` split into 10 independent sub-crates.
- `MfsStore` now auto-creates SQLite database files (fixes production startup with default config).
- REST routes unified to `{id}` parameter syntax for axum 0.8 compatibility.

### Fixed
- CLI bin tests were not compiled by CI (`--lib` only); now 52 tests run in full.
- Bridge integration tests excluded from CI workspace due to OOM; now isolated per-crate.
- SQLite file-backed persistence tests were `#[ignore]`d; now enabled and passing.
- Deprecated `upload-release-asset` GitHub Action replaced with `upload-artifact` + `gh-release`.

## [0.0.1] — 2025-06-01

### Added
- Initial project scaffolding with cognitive loop prototype.
- WASM sandbox with fuel metering and memory limits.
