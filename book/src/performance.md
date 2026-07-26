# Performance

Sisyphus includes several performance optimizations.

## Pipeline Batching

Nodes are executed in adaptive batches based on historical execution times.

- Fast nodes (<50ms): Larger batches (1.5x)
- Slow nodes (>500ms): Smaller batches (0.7x)
- Medium nodes: Maintain current batch size

## Output Deduplication Cache

The `OutputCache` stores content-hash indexed results to avoid re-executing identical computations.

- 1000 entry capacity
- Content-hash based (deterministic)
- Automatic eviction on capacity

## Context Compression

Large context values are automatically compressed:

- Strings > threshold: Truncated with marker
- Objects > 10 keys: Kept first 10 keys

## Execution Profiler

Tracks per-node execution metrics:

- Average duration by node type
- Cache hit rate
- Slowest nodes list

## Benchmarks

```bash
cargo bench -p sisyphus-core
```

Benchmark results are stored in `target/criterion/`.
