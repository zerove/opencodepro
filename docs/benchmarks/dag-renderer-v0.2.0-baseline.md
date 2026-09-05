# DagRenderer Benchmark Baseline (v0.2.0)

Generated: 2026-09-05

**Hardware:** Intel(R) Core(TM) i7-8750H CPU @ 2.20GHz, 19.5 GiB RAM
**Rust:** nightly (via rust-override)
**Criterion:** warm-up 1s, measurement 2-3s, sample size 10-15

## 优化摘要

**变更**: `crates/sisyphus-core/src/dag_renderer.rs::compute_layout`
- 原: `queue.sort()` 在 BFS 每轮（O(N² log N)）
- 优: `queue.sort_unstable()` 替代 `sort`（更快、不保证等值元素相对顺序；节点 ID 唯一性保证输出确定性）
- 性能改进：8-32%（统计显著）

## DagRenderer (sort_unstable 后)

| 节点数 | Mean      | Throughput  | vs 优化前 |
|--------|-----------|-------------|----------|
| 10     | 12.5 µs   | 800 Kelem/s | -32% ✅  |
| 50     | 92.9 µs   | 538 Kelem/s | -2.6% (无显著) |
| 200    | 414 µs    | 483 Kelem/s | -11.5% ✅ |
| 500    | 838 µs    | 597 Kelem/s | -9.0% ✅  |
| 1000   | 1.79 ms   | 559 Kelem/s | -8.7% ✅  |
| 2000   | 3.82 ms   | 524 Kelem/s | (-1.5% 无显著) |
| 5000   | 8.86 ms   | 564 Kelem/s | -22% ✅  |

**关键发现**：throughput 在 483-800 Kelem/s 之间波动，**接近 O(N) 线性扩展**。
即使 5000 节点也只 8.86 ms，完全可接受（TUI 渲染 60fps 预算 = 16.67 ms）。

## 算法复杂度分析

实际观察到的复杂度接近 **O(N)**，而非理论最坏 O(N² log N)。原因：
- 平均 degree < 2（bench 用稀疏 graph，每节点 1-2 子节点）
- sort_unstable 是 introsort（O(N log N) 平均）
- HashMap 操作 O(1) 平均

最坏情况（密集 DAG，每个节点所有前置都已完成）会触发 O(N²) sort，但实践中不常见。

## 使用方式

```bash
# 跑全部 bench
cargo bench -p sisyphus-core --bench lightweight_benches -- dag_renderer

# 仅特定规模
cargo bench -p sisyphus-core --bench lightweight_benches -- dag_renderer/1000

# 与 baseline 对比
cargo bench -p sisyphus-core --bench lightweight_benches -- dag_renderer --baseline v0_2_0_dag_renderer
```

baseline 已保存：`target/criterion/dag_renderer/v0_2_0_dag_renderer/`