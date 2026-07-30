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

## Performance Tuning

### Cache Tuning

The `OutputCache` eviction policy and capacity can be tuned for different workloads.

| Parameter        | Default | Description                          | Recommendation                     |
|------------------|---------|--------------------------------------|------------------------------------|
| Cache capacity   | 1000    | Max entries before eviction          | Increase for repetitive workloads  |
| Context threshold| 4096    | Bytes before compression triggers    | Decrease for memory-constrained    |
| Batch base size  | 4       | Initial pipeline batch size          | Increase for high-throughput       |

**Trade-offs**: A larger cache reduces re-execution at the cost of memory. Monitor the cache hit rate via the execution profiler. If hit rate is below 60%, consider increasing the capacity. If above 95%, reducing it can free memory without impact.

### Sandboxd Mode Selection

The sandbox daemon (`sandboxd`) can run in two modes:

| Mode       | Latency | Isolation | Use case                     |
|------------|---------|-----------|------------------------------|
| **Embedded** | ~50µs  | Process   | Development, trusted workloads |
| **Daemon**   | ~500µs | Container | Production, untrusted code     |

- **Embedded mode** instantiates a Wasm runtime in-process. Lowest latency but no container-level isolation. Suitable for development and CI.
- **Daemon mode** communicates with a separate `sandboxd` process via gRPC. Higher latency per invocation but provides container-level sandboxing.

Switch modes by configuring the tool executor in `Config.tool_executor`. The daemon mode is the default for production deployments.

### Concurrent Task Tuning

The cognitive loop supports configurable concurrency for parallel branch evaluation.

```toml
# config.toml
[cognitive_loop]
max_concurrent_tasks = 4      # Parallel branches in ParallelBranchPredictor
max_iterations = 30           # Upper bound on cognitive iterations
convergence_threshold = 0.15  # Stop when deviation drops below this
```

Tuning guidelines:

- **max_concurrent_tasks**: Set to the number of CPU cores for CPU-bound workloads. For I/O-bound (LLM calls), higher values (8-16) can improve throughput.
- **max_iterations**: Lower values reduce worst-case latency. Higher values improve convergence quality.
- **convergence_threshold**: Tighter (lower) thresholds improve output quality at the cost of more iterations.

Monitor cognitive loop performance using the tracing spans (`predict`, `execute`, `reflect`, `evolve`) via `RUST_LOG=sisyphus_core=info`.
