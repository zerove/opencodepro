# Contributing

## Development Setup

1. **Prerequisites**: Rust 1.96+, Cargo
2. **Clone**: `git clone https://github.com/zeronhq/sisyphus-wasm`
3. **Build**: `cargo build --workspace`
4. **Test**: `cargo test --workspace`

## Code Quality

The CI pipeline enforces:

- **Formatting**: `cargo fmt --all --check`
- **Linting**: `cargo clippy --workspace --all-targets` (zero warnings required)
- **Tests**: `cargo test --workspace` (all must pass)
- **Supply chain**: `cargo deny check` (licenses, bans, advisories)

## Project Structure

```
crates/
  sisyphus-types/    — Core data structures (no deps)
  sisyphus-metrics/  — Prometheus observability
  sisyphus-sandbox/  — WASM sandbox runtime
  sisyphus-intel/    — Cognition & simulation
  sisyphus-mem/      — Multi-level cache
  sisyphus-core/     — Kernel & orchestration
  sisyphus-bridge/   — Entry point & API
book/                — mdBook documentation
```

## Guidelines

- **No `unwrap()` in production code** — use `expect("descriptive message")` or propagate with `?`.
- **No `as any` or `@ts-ignore` equivalents** — maintain type safety.
- **Test coverage** — add tests for new functionality, especially edge cases.
- **Metrics** — instrument significant operations via `sisyphus_metrics`.

## WASM Tool Development

Tool WASM binaries go in `wasm_tools/` at the workspace root. Each binary should export a `run` function that accepts and returns byte arrays.
