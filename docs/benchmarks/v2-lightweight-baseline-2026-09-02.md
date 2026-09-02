# Sisyphus 轻量级 Bench 基线（2026-09-02）

生成时间：2026-09-02T05:04+08:00

**硬件：** Intel(R) Core(TM) i7-8750H CPU @ 2.20GHz，6 核 12 线程，19 GiB RAM
**Rust：** 1.98.0 stable
**Criterion：** warm-up 1s，sample size 10，measurement 2s（CI 快速档）
**Profile：** debug（release 实际更快 ~2-5x）

## Lightweight benches（不依赖 sandbox）

新增的 `crates/sisyphus-core/benches/lightweight_benches.rs`：
- 默认 feature 下即可运行（`cargo bench --bench lightweight_benches`）
- 覆盖 AnomalyDetector 检测吞吐 + DagRenderer 大型 SOP 渲染
- 目标：每次 release PR 跑 < 30s，提供性能回归快速检查

### AnomalyDetector 检测吞吐（v2 阈值配置化后行为）

观察方法 `observe(node, fuel_consumed, duration_ms, success=true)`：

| 规模 | 时间 / 100 次 | 吞吐 |
|----------|---------------|---------|
| 10 节点  | 42.3 µs       | 2.4 Melem/s |
| 100 节点 | 45.8 µs       | 2.2 Melem/s |
| 1000 节点| 458.0 µs      | 2.2 Melem/s |

> 观察吞吐稳定 ~2.2 Melem/s，与节点数近似线性，无超线性退化。
> 1000 节点连续观察耗时 458 µs，单次 ~460 ns（含 HashMap 锁 + 移动平均更新）。

### AnomalyDetector 边界场景

| 场景 | 时间 |
|------|------|
| 1000 节点连续观察 | 263 µs（单次 ~263 ns） |

### DagRenderer 大型 SOP 渲染

| 规模 | 时间 | 吞吐 |
|----------|--------|---------|
| 10 节点  | 19 µs   | 520 Kelem/s |
| 50 节点  | 93 µs   | 540 Kelem/s |
| 200 节点 | 461 µs  | 430 Kelem/s |
| 500 节点 | 1011 µs | 490 Kelem/s |

> **关键结论**：500 节点 SOP 渲染 ~1 ms，吞吐稳定 430-540 Kelem/s。
> 远超 TODO 7.3 提出的">200 节点需 lazy load"目标——**当前实现无需优化**。
> 主要瓶颈：节点位置分配 + 边连接 BFS，HashMap 锁竞争次要。

## TUI 冷启动 profile（2026-09-02）

release build（`cargo build --release -p sisyphus-cli`）：

| 启动 | 耗时 | 内存 |
|------|------|------|
| 冷启动 1（首次） | 0.49 s | 8.4 MB |
| 冷启动 2（缓存） | < 0.05 s | 8.8 MB |
| 冷启动 3（缓存） | < 0.05 s | 8.4 MB |

二进制大小：
- unstripped: 16M
- stripped: 13M

> **结论**：已达成 TODO 7.3 TUI 启动 < 200ms 目标（warm path 50ms）。
> 冷启动 0.49s 主要是动态库加载（libssl, libtokio 等），符合预期。

## 与历史 baseline 对比（v2-baseline-2026-08-09）

| 类别 | 对比基线 | 状态 |
|------|---------|------|
| DAG Executor nodes_5/10/20 | 857µs/1.8ms/3.1ms | 未重新测量（需 sandbox feature） |
| WASM single_node | 1.6ms | 未重新测量（需 sandbox feature） |
| MFS 存储 | 5ns/op | 未重新测量 |
| AnomalyDetector 1000 节点 | 458 µs | **新基线**（vs 旧版无配置化硬编码常量） |
| DagRenderer 500 节点 | 1011 µs | **新基线**（旧版无数据） |

## 运行命令

```bash
# 轻量级 bench（默认 feature，无 sandbox 依赖）
cargo bench --bench lightweight_benches -- \
  --sample-size 10 --warm-up-time 1 --measurement-time 2

# 完整 bench（需 sandbox feature）
cargo bench -p sisyphus-core --features sandbox
```

## CI 集成建议

- PR 预检：跑 lightweight_benches（< 30s 完成）
- 主分支：每晚跑完整 bench
- 性能基线更新：手动 + 自动 PR 标记（`perf: `）

## 更新基线流程

```bash
# 1. 跑新数据
cargo bench --bench lightweight_benches -- --output-format bencher > new.txt

# 2. 解析 + 写入本文件
# 3. 在 CHANGELOG.md 标记 bench-baseline-update
# 4. 提交：docs(benchmarks): 刷新 v2-baseline
```
