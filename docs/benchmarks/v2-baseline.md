# Cognitive Loop v2 Benchmark Baseline

Generated: 2026-07-30T02:02:13+08:00

**Hardware:** Intel(R) Core(TM) i7-8750H CPU @ 2.20GHz, 15.6 GiB RAM
**Rust:** nightly (via rust-override)
**Criterion:** warm-up 2s, sample size 50, measurement 5s

## AdaptiveDepthController

| Bench                     | Mean      | StdDev (range)      |
|---------------------------|-----------|---------------------|
| convergence_ramp/5        | 367.14 ns | 356.52 – 378.29 ns  |
| convergence_ramp/10       | 660.78 ns | 639.68 – 687.79 ns  |
| convergence_ramp/20       | 1.397 µs  | 1.312 – 1.499 µs    |
| plateau_early_stop        | 483.87 ns | 453.07 – 522.53 ns  |

## AnomalyDetector

| Bench                       | Mean      | StdDev (range)       |
|-----------------------------|-----------|----------------------|
| observe_bulk/100            | 38.822 µs | 35.603 – 42.112 µs   |
| observe_bulk/1000           | 424.83 µs | 408.08 – 446.03 µs   |
| observe_bulk/10000          | 4.237 ms  | 4.046 – 4.473 ms     |
| report_generation           | 250.94 µs | 237.75 – 264.78 µs   |

## ParallelBranchPredictor

| Bench     | Mean     | StdDev (range)      |
|-----------|----------|---------------------|
| new_4_4   | 2.666 ns | 2.614 – 2.716 ns    |
| default   | 2.739 ns | 2.642 – 2.844 ns    |

> **Note:** The `ParallelBranchPredictor` benchmarks only measure construction time, as the `predict_parallelism` method was removed during the v2 API migration. The main operation `evaluate_branches` is an async method requiring simulator setup and is not yet benchmarked.

## Comparison with Previous Baseline (if available)

No previous baseline exists for the v2 components. This document serves as the initial baseline record.

## Run Command

```
cargo bench -p sisyphus-core --bench core_benches --features sandbox -- "v2_"
```
