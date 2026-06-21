# Sisyphus — Cognitive Computation Engine

Sisyphus is a **cognitive computation engine** for building autonomous AI agents that plan, execute, and improve their behavior through a recursive **observe–orient–decide–act** (OODA) loop.

## What It Does

At its core, Sisyphus takes a high-level goal (expressed in natural language) and:

1. **Synthesizes** a Standard Operating Procedure (SOP) — a directed acyclic graph (DAG) of executable sub-steps.
2. **Executes** the SOP in parallel across available nodes, respecting dependencies.
3. **Simulates** outcomes via Monte Carlo tree search to predict success probability.
4. **Reflects** on execution results — computing a *deviation score* between predicted and actual outcomes.
5. **Evolves** the SOP — retrying failed nodes, increasing fuel budgets, or restructuring the graph.

This loop continues until the goal converges (deviation below threshold) or a maximum iteration limit is reached.

## Why Sisyphus?

- **Self-improving**: The engine learns from every execution, evolving its SOPs without human intervention.
- **Safe by design**: WASM sandboxing for tool code execution with fuel limits and memory bounds.
- **Observable**: Built-in Prometheus metrics for every cognitive operation.
- **Composable**: Each crate is independent; use just the parts you need.

## Project Status

Sisyphus is in **active development** (v0.1.0). The core cognitive loop is functional and tested, with WASM tool execution, multi-level caching, and Prometheus metrics integration.

## Next Steps

- Read the [Architecture Overview](architecture.md) for a high-level system design.
- Dive into the [Key Concepts](concepts.md) to understand SOPs, cognitive loops, and DAG execution.
- Check the [Crate Reference](crates.md) for detailed API documentation.
