# Sisyphus 26.0 — Cognitive Execution Engine

Asynchronous, memory-first cognitive engine for predictive SOP (Standard Operating Procedure) execution.
Built in Rust with async-reactive architecture.

## Project Overview

*   **Status:** Active development
*   **Purpose:** Cognitive execution engine with predictive simulation, reflection-based learning, and autonomous SOP evolution
*   **Target Stack:** Rust (async, tokio), Wasmtime (sandbox), SQLite (persistence), gRPC (bridge)

## Architecture

```
SisyphusBridge (gRPC)
       ↓
SisyphusKernel — orchestrator
  ├── Executor — DAG-based async SOP execution with JoinSet
  ├── MfsStore — Memory-First State (DashMap L1 + SQLite L2)
  ├── ToolExecutor — Wasmtime sandbox (fuel-limited, module cached)
  └── SisyphusIntel — cognitive loop
       ├── Simulator — Monte Carlo / Experience-driven prediction
       ├── Reflector — trace analysis & deviation scoring
       └── Evolver — autonomous SOP mutation (fuel, retry logic)
```

**Core Loop**: `Synthesize → Predict → Execute → Reflect → Evolve`

---

## Build & Run Commands

### Build & Package
*   Build all crates: `cargo build`
*   Production build: `cargo build --release`
*   Check (fast): `cargo check`

### Run & Dev
*   Bridge server: `cargo run -p sisyphus-bridge`
*   Integration stress: `cargo run -p sisyphus-core --bin integration_stress`
*   Bridge client: `cargo run -p sisyphus-bridge --bin client`

### Testing
*   All tests: `cargo test`
*   Specific crate tests: `cargo test -p sisyphus-mem`
*   Specific test: `cargo test -- -t test_name`

### Formatting & Linting
*   Lint: `cargo clippy`
*   Format: `cargo fmt`

---

## Development Conventions

As a general guideline, all code developed in this workspace should adhere to the following principles:

### 1. Code Quality & Architecture
*   **Type Safety:** Prefer strong typing (TypeScript, Rust, Go, Python with type hints) where applicable. Avoid `any` or disabling type-checker warnings.
*   **Explicit Composition:** Prioritize explicit composition and delegation over deep inheritance patterns.
*   **No Hidden Logic:** Avoid prototype manipulation, reflection hacks, or hidden state transitions. Keep data flows traceable and pure.

### 2. Testing Standards
*   **Test-First Approach:** Prefer Test-Driven Development (TDD) for core logic and business rules.
*   **Verification:** Always add automated tests for new features, bug fixes, or structural changes. Validate locally before pushing or marking work complete.
*   **Integration Tests:** Write end-to-end or integration tests to verify the boundaries between major modules.

### 3. Version Control (Conventional Commits)
When staging and committing changes, use Conventional Commits format for clarity:
*   `feat(...)`: New feature
*   `fix(...)`: Bug fix
*   `docs(...)`: Documentation changes
*   `style(...)`: Formatting/styling (no logic change)
*   `refactor(...)`: Code change that neither fixes a bug nor adds a feature
*   `test(...)`: Adding missing tests or correcting existing tests
*   `chore(...)`: Changes to build process, auxiliary tools, or libraries

### 4. Token-Optimized Development
*   **RTK Integration:** This workspace is optimized using the **Rust Token Killer (rtk)** CLI proxy.
*   Always use `rtk` commands directly where applicable to optimize token consumption during development tasks.

---

## Workspace Directory Structure

*Below is the starting layout. As directories are created, describe their roles here.*

```
/home/zeron/dev/opencodepro/
└── GEMINI.md            # This instruction context file
```
