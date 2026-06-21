# Changelog

All notable changes to Sisyphus will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — Unreleased

### Added

- **Core cognitive loop**: Recursive observe–orient–decide–act cycle with configurable max iterations and convergence threshold.
- **SOP (Standard Operating Procedure)**: DAG-based procedure representation with parallel execution via topological ordering.
- **WASM sandbox**: Secure tool code execution via Wasmtime with fuel metering, memory limits, and module caching.
- **Cognitive loop (recursive)**: Recursive cognition with error categorization, adaptive retry, and convergence detection.
- **Monte Carlo simulation**: Path probability estimation and blended cost analysis over SOP graphs.
- **Structured reflection**: Deviation scoring between predicted and actual execution outcomes.
- **Adaptive fuel pricing**: Exponential backoff fuel pricer that adjusts budgets based on execution history.
- **SOP evolution**: Automatic graph mutation — retry wrappers, fuel adjustment, and subgraph restructuring.
- **Cross-graph dependency resolution**: CompositeGraph with topological execution order across multiple SOPs.
- **Multi-level cache**: L1 (DashMap in-memory) + L2 (SQLite persistent) with snapshot/restore.
- **Prometheus metrics**: Counter, Histogram, and Gauge instrumentation for all cognitive operations.
- **gRPC API**: Tonic-based server (`sisyphus-bridge`) with `ExecuteGoal` and `ExecuteSop` endpoints.
- **CLI client**: gRPC client binary for server interaction (`client.rs`).
- **In-process stress test**: Benchmark binary (`stress_test.rs`) for throughput measurement.
- **Integration stress test**: End-to-end binary (`integration_stress`) exercising the full cognitive pipeline.
- **Property-based tests**: Proptest coverage for SOPGraph roundtrip, ExecutionContext operations, and CompositeGraph correctness.
- **Supply chain audit**: cargo-deny configuration with license, ban, and advisory checking.
- **mdBook documentation**: Architecture overview, crate references, and key concepts.

### Infrastructure

- **CI pipeline**: GitHub Actions workflow — cargo fmt, clippy, build, test, and cargo-deny.
- **Developer tooling**: `rust-toolchain` setup, pre-commit hooks (optional).

[0.1.0]: https://github.com/zeronhq/sisyphus-wasm/releases/tag/v0.1.0
