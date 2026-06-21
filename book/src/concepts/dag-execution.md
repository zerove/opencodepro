# DAG Execution

Sisyphus executes SOP graphs using a **parallel topological executor**.

## Algorithm

1. **Compute in-degrees** for all nodes in the graph.
2. **Seed** the ready set with nodes whose in-degree is zero.
3. **Spawn** ready nodes concurrently via `tokio::task::JoinSet`.
4. **Collect** results as they complete → decrement in-degree of downstream nodes.
5. **Add** newly-ready nodes to the next batch.
6. **Repeat** until all nodes complete or an error occurs.

## Parallelism

Nodes with no dependency relationship execute simultaneously:

```
Time ─────────────────────────────────────►
┌──────┐  ┌──────┐  ┌──────┐
│Node A│  │Node B│  │Node C│  (parallel)
└──┬───┘  └──────┘  └──┬───┘
   │                    │
   └──────┬─────────────┘
          ▼
     ┌────────┐
     │Node D  │  (sequential — awaits A & C)
     └────────┘
```

## Error Handling

- If any node returns an error, the error is propagated up to the caller.
- The cognitive loop catches node errors and evolves the SOP to add retry wrappers.
- Cycle detection: If adding an edge creates a cycle, the operation is rejected with an error.

## Fuel & Memory

Each node executes with its configured fuel budget and memory limit. Nodes that exceed limits are aborted with a descriptive error, which the cognitive loop uses to adjust future executions.
