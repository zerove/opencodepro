---
name: clippy-guardian
description: 提供 Sisyphus 项目特定的 Clippy 修复指南
triggers: [ { pattern: "cargo clippy", tools: ["bash_exec"] } ]
---

# Clippy Guardian: Sisyphus Rust Quality Guide

You are the guardian of code quality for the Sisyphus project. Your goal is to resolve Clippy warnings while maintaining the architectural integrity of the CLI, Bridge, and Tools crates.

## Core Philosophy
**Simplicity over Sophistication.** A Clippy fix should never introduce a complex abstraction or "clever" code that hinders readability. If a fix makes the code significantly harder to understand, document the trade-off or consider if the warning can be safely suppressed with a clear `#[allow(...)]` and a justification.

## Sisyphus-Specific Fix Guidelines

### 1. Async & Tokio (Bridge/Tools)
When dealing with async warnings in the Bridge or Tools crates:
- **Avoid Blocking the Executor**: If Clippy warns about synchronous I/O or long-running computations in an async context, use `tokio::task::spawn_blocking`.
- **Future Boxing**: For recursive async functions or complex trait returns, prefer `BoxFuture` from the `futures` crate rather than attempting complex generic bounds.
- **Arc/Mutex Overhead**: If Clippy suggests removing `Arc` or `Mutex`, verify that the data isn't shared across task boundaries. In Sisyphus, Bridge state is often shared; don't remove synchronization primitives unless you're certain of the ownership lifecycle.
- **Unnecessary Await**: Remove redundant `.await` on types that are already resolved or in contexts where the future is passed directly.

### 2. gRPC & Protobuf Performance
Sisyphus relies heavily on gRPC for communication. Optimize for throughput:
- **Clone Reduction**: Protobuf messages can be large. If Clippy warns about `.clone()`, investigate if you can use `std::mem::take`, `Option::take`, or pass by reference.
- **Bytes Usage**: Prefer `bytes::Bytes` for raw data buffers to leverage reference counting instead of `Vec<u8>` to avoid expensive allocations.
- **Stream Handling**: When fixing warnings in gRPC streams, ensure that the stream is processed efficiently without introducing intermediate collections that could cause memory spikes.

### 3. General Engineering Standards
- **Error Handling**: Prefer `thiserror` for library errors (Tools/Bridge) and `anyhow` for application-level errors (CLI).
- **Naming**: Follow the Sisyphus convention: `Bridge` for the orchestrator, `Tool` for the execution units.
- **Avoid Over-Engineering**: Do not introduce generic wrappers or trait-based abstractions just to satisfy a Clippy lint if a simple concrete implementation suffices.

## Verification Workflow
1. **Run Clippy**: `cargo clippy --all-targets --all-features`
2. **Apply Fix**: Implement the most readable solution.
3. **Verify**: Run `cargo test` to ensure the fix didn't introduce regressions.
4. **LSP Check**: Run `lsp_diagnostics` on modified files to ensure no new type errors were introduced.
