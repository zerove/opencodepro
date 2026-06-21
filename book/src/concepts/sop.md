# Standard Operating Procedure (SOP)

An SOP is a **directed acyclic graph (DAG)** that represents a procedure to achieve a specific goal.

## Structure

```
           ┌──────┐
           │ Goal │
           └──┬───┘
              │
    ┌─────────┼─────────┐
    ▼         ▼         ▼
┌──────┐ ┌──────┐ ┌──────┐
│Node 1│ │Node 2│ │Node 3│
│ WASM │ │ WASM │ │ WASM │
└──┬───┘ └──────┘ └──┬───┘
   │                  │
   └──────┬───────────┘
          ▼
     ┌────────┐
     │ Result │
     └────────┘
```

Each node contains:
- **Name**: Unique identifier within the SOP
- **Description**: What this step does (used for synthesis and reflection)
- **Tool Type**: Which WASM tool binary to invoke
- **Fuel Budget**: WASM instruction limit for this node
- **Timeout**: Maximum wall-clock time for execution
- **Retry Policy**: Max retries and current retry count

## Lifecycle

1. **Synthesize**: The `StandardReflector` generates an SOP graph from a natural-language goal using the synthesizer module.
2. **Execute**: The `DagExecutor` runs the SOP with topological ordering.
3. **Evolve**: After reflection, the SOP may be mutated — new nodes added, fuel budgets adjusted, or subgraphs restructured.

## Evolution Operations

| Operation | Description |
|-----------|-------------|
| **Retry wrapper** | Wrap a failed node with retry logic |
| **Fuel increase** | Boost fuel budget for nodes that exhausted fuel |
| **Subgraph restructure** | Reorder or split nodes for better parallelism |
