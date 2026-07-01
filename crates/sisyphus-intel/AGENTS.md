# Sisyphus Intel Agent Knowledge Base

## OVERVIEW
Intelligence and synthesis module for goal-driven SOP generation and adaptive evolution.

## WHERE TO LOOK
- Flow Synthesis: `synthesis.rs` (keyword-based), `llm_synthesizer.rs` (LLM-driven)
- Monte Carlo Simulation: `simulation.rs` (experience-weighted random walks)
- Deviation / Reflection: `reflect.rs` (error categorization, StandardReflector)
- Adaptive Fuel Pricing: `fuel.rs` (cost-based fuel adjustment)
- SOP Evolution: `reflect.rs` (StandardEvolver), `topological_evolver.rs` (structural evolution)
- Formal Verification: `formal_verification.rs` (Kani, Loom, Proptest stubs)
- IntelError: `error.rs`
- SisyphusIntel orchestrator: `lib.rs` (re-exports only)

## CONVENTIONS
- Use `async_trait` for all synthesis, simulation, and reflection interfaces
- Parallelize Monte Carlo iterations using `rayon` for high-throughput sampling
- Evolve SOPs via `EvolutionOp` to maintain DAG invariants and prevent cycles
- Propagate errors using `IntelError` enum; avoid `unwrap()` in production paths
- Use `SOPGraph` as the canonical representation for synthesized plans
- Goal classification relies on verb-mapping for keyword-based synthesis

## COMMANDS
```bash
cargo test -p sisyphus-intel
cargo bench -p sisyphus-intel
```
