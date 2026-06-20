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
       ├── Synthesizer — GoalDrivenSynthesizer (verb parsing → DAG)
       ├── Simulator — MonteCarloSimulator (experience-weighted random walk)
       ├── Reflector — StandardReflector (trace analysis + ErrorCategory)
       └── Evolver — StandardEvolver (fuel pricing + retry wrapping)
```

**Core Loop**: `Synthesize → Predict → Execute → Reflect → Evolve`

### Recursive Cognitive Loop

The kernel's `execute_with_cognition()` implements an iterative feedback loop:

```
                 ┌──────────────────────────────┐
                 │        Start                  │
                 │  (SOPGraph + start_node)      │
                 └──────────┬───────────────────┘
                            ↓
                 ┌──────────────────────────────┐
                 │  1. Predict (Simulator)       │
                 │  Monte Carlo + history prior  │
                 └──────────┬───────────────────┘
                            ↓
                 ┌──────────────────────────────┐
                 │  2. Execute (Executor)        │
                 │  DAG-based JoinSet execution  │
                 └──────────┬───────────────────┘
                            ↓
                 ┌──────────────────────────────┐
                 │  3. Reflect (Reflector)       │
                 │  Trace analysis + deviation   │
                 └──────────┬───────────────────┘
                            ↓
                 ┌──────────────────────────────┐
                 │  4. Evolve (Evolver)          │
                 │  FuelPricer → node mutation   │
                 └──────────┬───────────────────┘
                            ↓
                  ┌─────────┴─────────┐
                  │  deviation < 0.3  │  ← CONVERGENCE_THRESHOLD
                  │  OR max iter (3)  │  ← MAX_COGNITIVE_ITERATIONS
                  └─────────┬─────────┘
                   YES ↓          ↑ NO (evolve → re-execute)
             ┌──────────────┐
             │   Complete   │
             │ (CognitiveReport) │
             └──────────────┘
```

**Constants:**
- `MAX_COGNITIVE_ITERATIONS = 3` — maximum loop executions
- `CONVERGENCE_THRESHOLD = 0.3` — deviation below which stops early
- `Cit` structure: tracks `(iteration, prediction, deviation_score, suggestions, evolved)` per round

### CognitiveReport

Returned after loop completion:

```rust
pub struct CognitiveReport {
    pub iterations: usize,       // actual iterations used
    pub converged: bool,         // true if deviation < threshold
    pub final_deviation: f64,    // final deviation score
    pub history: Vec<CognitiveIteration>,  // per-iteration details
}
```

### Fuel Pricing Architecture

Granular per-node fuel adjustment using simulation predictions.

```
                        SimulationResult
                              │
                              ▼
   ┌───────────────────────────────────────────┐
   │              FuelPricer trait              │
   │  price_fuel(node_name, current_fuel,       │
   │             prediction) -> new_fuel        │
   └───────────────────────────────────────────┘
          ▲                       ▲
          │                       │
   ┌──────┴──────┐     ┌─────────┴─────────┐
   │ SimpleFuel  │     │  AdaptiveFuelPricer │
   │ Pricer      │     │                     │
   │ (×2 always) │     │  estimated_cost >   │
   └─────────────┘     │  current_fuel:      │
                       │    cost × 1.5       │
                       │  estimated_cost <   │
                       │  current_fuel/2:    │
                       │    current × 0.8    │
                       │  otherwise: keep    │
                       │  clamped to          │
                       │  [min_fuel, max_fuel]│
                       └─────────────────────┘
```

**Strategy:**
- **Inflation** (`estimated_cost > current_fuel`): `new = estimated_cost × 1.5` (headroom)
- **Shrink** (`estimated_cost < current_fuel / 2`): `new = current × 0.8` (avoid over-provision)
- **Hold** (otherwise): keep current fuel
- **Clamp**: always within `[100_000, 10_000_000]` (configurable via `with_bounds()`)

### ErrorCategory Classification

Reflection layer categorizes execution errors for targeted evolution:

| Category | Keywords | Severity | Evolution Action |
|---|---|---|---|
| `ResourceExhausted` | fuel, out of memory | 0.8 | Increase fuel / split nodes |
| `Timeout` | timeout, deadline | 0.5 | Shorten path / raise ceiling |
| `TypeError` | TypeError, mismatch | 0.6 | Verify input/output contracts |
| `Crash` | panic, crash, segfault | 1.0 | Review tool implementation |
| `Unknown` | (other) | 0.7 | Add structured error handling |

Deviation formula: `failure_rate × 0.6 + avg_severity × 0.3`

### Experience-weighted Monte Carlo Simulation

`MonteCarloSimulator` combines random walk with historical experience:

- **With `state_store`**: queries past execution records (`ExecutionHistory`) from MFS to bias transition probabilities (`0.9 × hist_success_rate + 0.1 × 0.9`) and cost estimates
- **Without history**: falls back to uniform random walk
- **Parallel**: uses Rayon for independent iterations, reduces to best path by `score = probability / cost`
- **Output**: `SimulationResult { path_id, success_probability, estimated_cost, predicted_outcome }`

### GoalDrivenSynthesizer

Parses natural-language goals into typed action DAGs:

| Verb Keywords | Action Type |
|---|---|
| fetch, load, get, read, retrieve | `Fetch` |
| process, transform, convert, parse, compute | `Process` |
| store, save, write, sink, persist | `Sink` |
| search, find, query, lookup, discover | `Search` |
| validate, check, verify, test, inspect | `Validate` |

- **Single action** → single node
- **Multiple actions** → linear pipeline in detection order
- **"parallel" in goal** → fan-out first two actions into merge node

### StandardEvolver

Mutates SOP nodes based on reflection feedback:

- **`needs_fuel`** (ResourceExhausted detected): applies `FuelPricer::price_fuel()` to each node
- **`needs_retry`** (retry logic suggested): wraps nodes in `RetrySOPNode { max_retries: 3 }`
- Both can apply simultaneously
- Edge topology preserved from original graph

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

```
/home/zeron/dev/opencodepro/
├── Cargo.toml                          # Workspace root
├── GEMINI.md                           # This instruction context file
├── crates/
│   ├── sisyphus-types/                 # Shared types: SOPGraph, SOPNode traits, ToolExecutor
│   ├── sisyphus-mem/                   # Memory-First State: MfsStore (DashMap L1 + SQLite L2)
│   ├── sisyphus-intel/                 # Cognitive loop: flow synthesis, MC simulation, reflection, evolution
│   │   ├── src/lib.rs                  # GoalDrivenSynthesizer, MonteCarloSimulator, StandardReflector,
│   │   │                               #   FuelPricer (Simple/Adaptive), StandardEvolver, ErrorCategory
│   │   └── tests/                      # Unit tests inline in lib.rs
│   ├── sisyphus-core/                  # Kernel orchestrator: DAG executor, recursive cognitive loop
│   │   ├── src/lib.rs                  # Executor, SisyphusKernel, execute_with_cognition, CognitiveReport
│   │   └── tests/
│   │       └── cognitive_loop_test.rs  # 4 integration tests for recursive cognitive loop
│   ├── sisyphus-sandbox/               # Wasmtime sandbox for tool execution (fuel-limited)
│   └── sisyphus-bridge/                # gRPC bridge server
│       ├── src/main.rs                 # Server entry point
│       └── src/client.rs               # Client example
```
