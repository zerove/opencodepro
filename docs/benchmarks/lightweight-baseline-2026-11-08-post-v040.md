# Sisyphus 轻量级 Bench 基线刷新（2026-11-08，post-v0.4.0）

生成时间：2026-11-08（Asia/Shanghai 上下文）

**目的**：对照 TODO 7.3「基准基线刷新」与 `docs/benchmarks/v2-lightweight-baseline-2026-09-02.md`，
在 v0.4.0 联邦学习与经验聚合（Phase A–G + 槽位对账）落地后，对 `sisyphus-core` 的轻量基准做一次
可复现快照。

**命令**（与 2026-09-02 快速档一致的 Criterion 参数）：

```bash
env -u RUSTUP_TOOLCHAIN cargo bench -p sisyphus-core --bench lightweight_benches -- \
  --warm-up-time 1 --measurement-time 2 --sample-size 10
```

**说明**：
- `cargo bench` 使用 bench profile（优化构建）；Criterion 输出显示运行自
  `target/release/build/.../out/lightweight_benches-*`。
- 结果受 CPU/负载/温控影响；本次与 2026-09-02 **不是同一台机器/同一时刻**的严格对比，
  仅用于量级确认与回归信号。严格对比请用 `make perf-analyze` + `critcmp` 在同机保存 baseline。
- `dag_renderer/5000` 在 2s 目标窗口内采样未完整结束，本次未纳入结论（非关键路径）。

## AnomalyDetector 检测吞吐

| 规模 | 时间（中位区间） | 吞吐 |
|------|------------------|------|
| 10 节点   | 2.76–3.34 µs   | ~3.0–3.6 Melem/s |
| 100 节点  | 44.5–54.1 µs   | ~1.85–2.25 Melem/s |
| 1000 节点 | 508–652 µs     | ~1.53–1.97 Melem/s |

边界场景 `observe_1000`：204–230 µs。

## DagRenderer 渲染

| 规模 | 时间（中位区间） | 吞吐 |
|------|------------------|------|
| 10 节点   | 12.5–14.5 µs  | ~691–800 Kelem/s |
| 50 节点   | 148–184 µs    | ~271–338 Kelem/s |
| 200 节点  | 345–364 µs    | ~549–579 Kelem/s |
| 500 节点  | 1.16–1.32 ms  | ~378–431 Kelem/s |
| 1000 节点 | 2.74–3.65 ms  | ~274–364 Kelem/s |
| 2000 节点 | 12.0–17.0 ms  | ~118–167 Kelem/s |

## 结论

- **500 节点 SOP 渲染仍在 ~1.2–1.3 ms 量级**，与 2026-09-02「~1 ms、无需 lazy load」的结论一致；
  不触发 TODO 7.3 所关注的 `>200 节点` 性能悬崖。
- 本次样本量小（10 samples）且跨机器/状态，绝对值与 2026-09-02 基线存在波动；
  Criterion 相对本机已保存 baseline 报告了回归提示，但**不应据此判定代码回归**——
  需在同机固定 baseline 后用 `critcmp` 复核。
- **后续**：发布前或性能敏感改动后，在同一 runner 上执行
  `make perf-analyze BASELINE=main-baseline` 并将 JSON/摘要归档。

## 证据

- 本次运行输出由 `cargo bench -p sisyphus-core --bench lightweight_benches` 产生（见上文命令）；
  关键行已摘录入本文。
